mod check;
mod config;
mod git;
mod init;
mod lang;
mod lexer;
mod output;
mod scan;
mod todo;

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

/// Exit codes: 0 success, 1 check failed, 2 usage or config error.
#[derive(Clone, Copy)]
pub enum Exit {
    Ok = 0,
    CheckFailed = 1,
    Usage = 2,
}

impl From<Exit> for ExitCode {
    fn from(e: Exit) -> Self {
        ExitCode::from(e as u8)
    }
}

#[derive(Parser)]
#[command(
    name = "td2do",
    version,
    about = "List TODO comments and check that new ones link to issues"
)]
struct Cli {
    /// Run as if started in this directory
    #[arg(short = 'C', global = true, default_value = ".")]
    dir: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Args, Clone, Copy)]
struct InitArgs {
    /// Machine-readable output
    #[arg(long)]
    json: bool,
    /// Write everything without asking
    #[arg(long)]
    yes: bool,
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum ColorWhen {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Args, Default)]
struct ListArgs {
    /// Machine-readable output
    #[arg(long)]
    json: bool,
    /// Only TODOs with no REF
    #[arg(long)]
    untracked: bool,
    /// Only TODOs with this REF (e.g. gh#12)
    #[arg(long = "ref", value_name = "REF")]
    reference: Option<String>,
    /// Only files whose repo-relative path matches this glob
    #[arg(long, value_name = "GLOB")]
    path: Option<String>,
    /// Only this tag (repeatable)
    #[arg(long, value_name = "TAG")]
    tag: Vec<String>,
    #[arg(long, value_enum, default_value_t, hide = true)]
    color: ColorWhen,
}

#[derive(Args)]
struct CheckArgs {
    /// Machine-readable output
    #[arg(long)]
    json: bool,
    /// Compare with this ref [default: check.base, else the first of origin/HEAD's branch, origin/main, origin/master, main, master]
    #[arg(long, value_name = "REF")]
    base: Option<String>,
    /// Compare with the index instead of the working tree (for a pre-commit hook)
    #[arg(long)]
    staged: bool,
    /// Check every TODO, not only new ones
    #[arg(long)]
    all: bool,
}

#[derive(Subcommand)]
enum Command {
    /// List every TODO comment (the default)
    List(ListArgs),
    /// Check that new TODOs link to issues
    Check(CheckArgs),
    /// Set up the config, hook and CI step
    Init(InitArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(exit) => exit.into(),
        Err(e) => {
            eprintln!("td2do: {e:#}");
            Exit::Usage.into()
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<Exit> {
    let repo = scan::open(&cli.dir)?;
    match &cli.command {
        None => list(&repo, &ListArgs::default()),
        Some(Command::List(args)) => list(&repo, args),
        Some(Command::Check(args)) => check(&repo, args),
        Some(Command::Init(args)) => init::run(&cli.dir, args.yes, args.json),
    }
}

fn list(repo: &scan::Repo, args: &ListArgs) -> anyhow::Result<Exit> {
    let mut result = scan::scan(repo)?;
    let glob = args
        .path
        .as_deref()
        .map(|g| globset::Glob::new(g).map(|g| g.compile_matcher()))
        .transpose()?;
    result.todos.retain(|t| {
        (!args.untracked || t.reference.is_none())
            && args.reference.as_deref().is_none_or(|want| {
                t.reference.as_ref().map(output::ref_string).as_deref() == Some(want)
            })
            && glob
                .as_ref()
                .is_none_or(|g| g.is_match(output::path_string(&t.path)))
            // Case-insensitive, so `--tag todo` and a custom `@todo` both work (ruling R15).
            && (args.tag.is_empty() || args.tag.iter().any(|w| w.eq_ignore_ascii_case(&t.tag)))
    });
    let text = if args.json {
        let todos: Vec<_> = result.todos.iter().map(output::todo_json).collect();
        let doc = serde_json::json!({ "schema": 1, "todos": todos, "skipped_unknown_lang": result.skipped_unknown_lang });
        format!("{}\n", serde_json::to_string_pretty(&doc)?)
    } else {
        let tty = match args.color {
            ColorWhen::Always => true,
            ColorWhen::Never => false,
            ColorWhen::Auto => std::io::stdout().is_terminal(),
        };
        if tty {
            output::render_terminal(
                &result.todos,
                std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
            )
        } else {
            output::render_plain(&result.todos)
        }
    };
    output::print(&text)?;
    Ok(Exit::Ok)
}

fn check(repo: &scan::Repo, args: &CheckArgs) -> anyhow::Result<Exit> {
    let base = args
        .base
        .clone()
        .or_else(|| repo.config.check.base.clone())
        .unwrap_or_else(|| git::default_base(&repo.root));
    let mut todos = scan::scan(repo)?.todos;
    // `--all` alone needs no diff, so it works without the base's history.
    let added = if args.all && !args.staged {
        None
    } else {
        // Before the first commit everything is added; there is no base to need.
        let from = if git::resolves(&repo.root, "HEAD") {
            git::merge_base(&repo.root, &base)?
        } else {
            git::empty_tree(&repo.root)?
        };
        Some(git::added_lines(&repo.root, &from, args.staged)?)
    };
    if args.staged
        && let Some(added) = &added
    {
        // Files the index changes are checked as staged, not as on disk.
        todos.retain(|t| !added.contains_key(&t.path));
        for path in added.keys().filter(|p| lang::for_path(p).is_some()) {
            let src = git::index_content(&repo.root, path)?;
            todos.extend(scan::scan_source(repo, path, &src));
        }
        todos.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
    }
    let is_new = |t: &todo::Todo| match &added {
        Some(added) if !args.all => added
            .get(&t.path)
            .is_some_and(|lines| lines.range(t.line..=t.last_line).next().is_some()),
        _ => true,
    };
    let violations = check::run(&todos, &is_new, &repo.config.check);
    let failed = violations
        .iter()
        .any(|v| v.severity == check::Severity::Fail);
    let checked = if args.all { "all" } else { "new" };
    let text = if args.json {
        let list: Vec<_> = violations.iter().map(check::Violation::to_json).collect();
        let doc = serde_json::json!({
            "schema": 1,
            "passed": !failed,
            "checked": checked,
            "base": base,
            "violations": list,
        });
        format!("{}\n", serde_json::to_string_pretty(&doc)?)
    } else {
        let mut text = String::new();
        for v in &violations {
            let place = output::render_plain(std::slice::from_ref(&v.todo));
            text.push_str(&format!(
                "{}  [{} {}] {}\n",
                place.trim_end(),
                v.severity.name(),
                v.rule.name(),
                v.message()
            ));
        }
        let count = |s| violations.iter().filter(|v| v.severity == s).count();
        let scope = if args.all { "TODOs" } else { "new TODOs" };
        text.push_str(&format!(
            "td2do check: {} {scope}, {} failed, {} warnings (base {base})\n",
            todos.iter().filter(|t| is_new(t)).count(),
            count(check::Severity::Fail),
            count(check::Severity::Warn),
        ));
        text
    };
    output::print(&text)?;
    Ok(if failed { Exit::CheckFailed } else { Exit::Ok })
}
