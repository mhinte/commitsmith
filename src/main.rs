//! commit-tui — compose and lint Conventional Commits in the terminal.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod app;
mod commit;
mod ui;
mod validate;

use validate::Severity;

#[derive(Debug, Parser)]
#[command(
    name = "commit-tui",
    version,
    about = "Compose and lint Conventional Commits in your terminal",
    long_about = "Run without arguments to open the composer. `check` validates an existing \
                  message and is meant for commit-msg hooks and CI."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Lint a commit message, for hooks and CI
    Check {
        /// Read the message from a file, e.g. .git/COMMIT_EDITMSG
        #[arg(long, conflicts_with = "message")]
        file: Option<PathBuf>,
        /// Read the message from the command line
        #[arg(long, value_name = "TEXT")]
        message: Option<String>,
        /// Fail on warnings too
        #[arg(long)]
        strict: bool,
    },
    /// List the accepted commit types
    Types,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        None => run_composer(),
        Some(Command::Types) => {
            for kind in commit::TYPES {
                println!("{kind}");
            }
            ExitCode::SUCCESS
        }
        Some(Command::Check {
            file,
            message,
            strict,
        }) => run_check(file, message, strict),
    }
}

fn run_composer() -> ExitCode {
    let mut terminal = ratatui::init();
    let mut app = app::App::default();
    let result = app::run(&mut terminal, &mut app);
    ratatui::restore();

    match result {
        Ok(()) => {
            if let Some(message) = app.output {
                println!("{message}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commit-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_check(file: Option<PathBuf>, message: Option<String>, strict: bool) -> ExitCode {
    let text = match (file, message) {
        (Some(path), _) => match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("commit-tui: cannot read {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        },
        (None, Some(message)) => message,
        (None, None) => {
            let mut buffer = String::new();
            if let Err(error) = std::io::stdin().read_line(&mut buffer) {
                eprintln!("commit-tui: cannot read stdin: {error}");
                return ExitCode::FAILURE;
            }
            buffer
        }
    };

    let issues = validate::lint(&text);
    let (errors, warnings) = validate::count(&issues);

    for issue in &issues {
        let symbol = match issue.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        eprintln!("{symbol}: [{}] {}", issue.field, issue.message);
    }

    if issues.is_empty() {
        println!("commit message is fine");
    } else {
        println!("{errors} errors, {warnings} warnings");
    }

    if errors > 0 || (strict && warnings > 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
