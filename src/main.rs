//! commitsmith — compose and lint Conventional Commits in the terminal.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod app;
mod application;
mod cli;
mod commit;
mod config;
mod git;
mod hooks;
mod install;
mod ui;
mod validate;

#[derive(Debug, Parser)]
#[command(
    name = "commitsmith",
    version,
    about = "Compose and lint Conventional Commits in your terminal",
    long_about = "Run without arguments to open the composer. `check` validates an existing \
                  message and is meant for commit-msg hooks and CI. `init` writes a starter \
                  .commitsmith.toml, `config` shows what is loaded, `hooks` manages the \
                  commit-msg hook."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Hand the finished message to `git commit` instead of printing it
    #[arg(short, long)]
    commit: bool,

    /// Set the full name used for the author handle and exit
    #[arg(long, value_name = "FULL NAME")]
    user: Option<String>,

    /// Put this binary on PATH for the current user and exit
    #[arg(long, conflicts_with = "uninstall")]
    install: bool,

    /// Print environment, config paths and repository detection, then exit
    #[arg(long)]
    doctor: bool,

    /// Remove the installed copies again
    #[arg(long)]
    uninstall: bool,

    /// Command name to install next to the binary, e.g. "smith"
    #[arg(long, value_name = "NAME", requires = "install")]
    alias: Option<String>,
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
        /// Repair what is unambiguous in the message file and report it
        #[arg(long, requires = "file", conflicts_with = "message")]
        fix: bool,
    },
    /// Write a starter .commitsmith.toml in the current directory
    Init {
        /// Do not ask; take the defaults
        #[arg(long)]
        defaults: bool,
        /// Overwrite the user config without asking
        #[arg(long)]
        force: bool,
    },
    /// Show the loaded config, its sources and the active profile
    Config {
        /// Print the name and the handle from the user config, if present
        #[arg(long)]
        identity: bool,
    },
    /// List the commit types of the active profile
    Types,
    /// Print hints about the staged index, for pre-commit hooks
    Hints,
    /// One line about the checked-out branch, for post-checkout hooks
    BranchHint,
    /// Replace this binary with the newest release
    Update {
        /// Only say whether there is a newer release
        #[arg(long)]
        check: bool,
    },
    /// Start a new message from the branch name, for prepare-commit-msg hooks
    Prepare {
        /// The message file git hands to the hook
        #[arg(long)]
        file: PathBuf,
    },
    /// Install the git hooks of this repository (a wizard when no action is given)
    Hooks {
        #[command(subcommand)]
        action: Option<HookAction>,
        /// Install all three hooks without asking; for scripts and CI
        #[arg(long, global = true)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum HookAction {
    /// Write the commit-msg hook
    Install {
        /// Install even into a directory husky, lefthook or pre-commit manages
        #[arg(long)]
        force: bool,
    },
    /// Remove the hook again
    Uninstall,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.doctor {
        print!(
            "{}",
            commitsmith::infrastructure::diagnostics::doctor(&cli::cwd())
        );
        return ExitCode::SUCCESS;
    }
    if cli.install {
        return cli::run_install(cli.alias.as_deref());
    }
    if cli.uninstall {
        return cli::run_uninstall();
    }

    if let Some(name) = cli.user.as_deref() {
        return cli::set_user(name);
    }

    match cli.command {
        None => cli::run_composer(cli.commit),
        Some(Command::Types) => {
            let (config, _) = cli::load_config();
            for kind in config.types() {
                println!("{}", kind.name);
            }
            ExitCode::SUCCESS
        }
        Some(Command::Config { identity }) => cli::show_config(identity),
        Some(Command::Init { defaults, force }) => cli::init_config(defaults, force),
        Some(Command::Hooks { action, yes }) => cli::run_hooks(action, yes),
        Some(Command::Prepare { file }) => cli::run_prepare(&file),
        Some(Command::Hints) => cli::run_hints(),
        Some(Command::BranchHint) => cli::run_branch_hint(),
        Some(Command::Update { check }) => cli::run_update(check),
        Some(Command::Check {
            file,
            message,
            strict,
            fix,
        }) => cli::run_check(file, message, strict, fix),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cli::composer::read_repository;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-main-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_plain_directory_has_no_repository_data() {
        let dir = temp("plain");
        assert!(read_repository(&dir, &[], 50, 20).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_repository_yields_a_status_line() {
        let dir = temp("repo");
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "T"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(&args)
                    .current_dir(&dir)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let data = read_repository(&dir, &["api".to_string()], 50, 20).expect("repo data");
        assert!(data.status.contains("main"), "{}", data.status);
        assert!(data.status.contains("0 staged"), "{}", data.status);
        assert!(data.scopes.all().contains(&"api".to_string()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
