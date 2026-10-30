mod config;
mod git;
mod lang;
mod lexer;
mod scan;
mod todo;

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

/// Exit codes: 0 success, 1 check failed, 2 usage or config error.
#[derive(Clone, Copy)]
#[allow(dead_code)]
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

#[derive(Subcommand)]
enum Command {
    /// List every TODO comment (the default)
    List(Common),
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
    let _config = config::load(&cli.dir)?;
    match cli
        .command
        .as_ref()
        .unwrap_or(&Command::List(Common { json: false }))
    {
        // Task 5 prints the result; until then the scan only has to succeed.
        Command::List(_) => {
            let repo = scan::open(&cli.dir)?;
            scan::scan(&repo)?;
            Ok(Exit::Ok)
        }
        // Later tasks fill these in; for now they succeed silently.
        Command::Check(_) | Command::Init(_) => Ok(Exit::Ok),
    }
}
