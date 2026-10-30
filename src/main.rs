mod config;
mod git;
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
#[allow(dead_code)] // CheckFailed is used by `check` in a later task
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
struct Common {
    /// Machine-readable output
    #[arg(long)]
    json: bool,
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

#[derive(Subcommand)]
enum Command {
    /// List every TODO comment (the default)
    List(ListArgs),
    /// Check that new TODOs link to issues
    Check(Common),
    /// Set up the config, hook and CI step
    Init(Common),
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
        // Later tasks fill these in; for now they succeed silently.
        Some(Command::Check(_)) | Some(Command::Init(_)) => Ok(Exit::Ok),
    }
}

fn list(repo: &scan::Repo, args: &ListArgs) -> anyhow::Result<Exit> {
    let mut result = scan::scan(repo)?;
    let glob = args
        .path
        .as_deref()
        .map(|g| globset::Glob::new(g).map(|g| g.compile_matcher()))
        .transpose()?;
    let tags: Vec<String> = args.tag.iter().map(|t| t.to_uppercase()).collect();
    result.todos.retain(|t| {
        (!args.untracked || t.reference.is_none())
            && args.reference.as_deref().is_none_or(|want| {
                t.reference.as_ref().map(output::ref_string).as_deref() == Some(want)
            })
            && glob
                .as_ref()
                .is_none_or(|g| g.is_match(output::path_string(&t.path)))
            && (tags.is_empty() || tags.contains(&t.tag))
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
    print!("{text}");
    Ok(Exit::Ok)
}
