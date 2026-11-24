//! commit-tui — compose and lint Conventional Commits in the terminal.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod app;
mod commit;
mod config;
mod git;
mod hooks;
mod setup;
mod ui;
mod validate;

use config::Config;
use validate::{Rules, Severity};

#[derive(Debug, Parser)]
#[command(
    name = "commit-tui",
    version,
    about = "Compose and lint Conventional Commits in your terminal",
    long_about = "Run without arguments to open the composer. `check` validates an existing \
                  message and is meant for commit-msg hooks and CI. `init` writes a starter \
                  .commit-tui.toml, `config` shows what is loaded, `hooks` manages the \
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
    /// Write a starter .commit-tui.toml in the current directory
    Init {
        /// Overwrite an existing file
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
    /// Install or remove the commit-msg hook of this repository
    Hooks {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Debug, Subcommand)]
enum HookAction {
    /// Write the commit-msg hook
    Install {
        /// Back up and replace a hook that is not ours
        #[arg(long)]
        force: bool,
    },
    /// Remove the hook again
    Uninstall,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(name) = cli.user.as_deref() {
        return set_user(name);
    }

    match cli.command {
        None => run_composer(cli.commit),
        Some(Command::Types) => {
            let (config, _) = load_config();
            for kind in config.types() {
                println!("{}", kind.name);
            }
            ExitCode::SUCCESS
        }
        Some(Command::Config { identity }) => show_config(identity),
        Some(Command::Init { force }) => init_config(force),
        Some(Command::Hooks { action }) => run_hooks(action),
        Some(Command::Check {
            file,
            message,
            strict,
        }) => run_check(file, message, strict),
    }
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn load_config() -> (Config, Vec<String>) {
    Config::load(&cwd())
}

fn run_hooks(action: HookAction) -> ExitCode {
    let result = match action {
        HookAction::Install { force } => {
            hooks::install(force).map(|path| format!("installed {path}"))
        }
        HookAction::Uninstall => hooks::uninstall().map(|path| format!("removed {path}")),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commit-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_composer(commit: bool) -> ExitCode {
    // First start: ask once for the name, then never again.
    if Config::needs_setup() {
        let mut terminal = ratatui::init();
        let first_run = setup::run(&mut terminal);
        ratatui::restore();
        match first_run {
            Ok(setup::Outcome::Saved(path)) => eprintln!("commit-tui: wrote {}", path.display()),
            Ok(setup::Outcome::Skipped) => eprintln!(
                "commit-tui: no name set, run `commit-tui --user \"Firstname Lastname\"` later"
            ),
            Err(error) => {
                eprintln!("commit-tui: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    let (config, warnings) = load_config();
    let mut app = app::App::with_config(config, warnings);

    // Scope suggestions come from history; a missing repository is fine.
    let root = cwd();
    if let Some(repo) = git::open(&root) {
        let depth = app.config.history_depth;
        let configured = app.config.scopes.clone();
        app.set_scopes(git::scope_info(&repo, &configured, depth));
        let branch = repo
            .head_name()
            .ok()
            .flatten()
            .map(|name| name.shorten().to_string())
            .unwrap_or_else(|| "detached".to_string());
        let staged = git::staged_summary(&root).len();
        app.set_repo_line(format!("{branch} · {staged} staged"));
        let depth = app.config.history_depth.min(200);
        app.set_history(git::recent_commits(&repo, depth));
    }

    let mut terminal = ratatui::init();
    let result = app::run(&mut terminal, &mut app);
    ratatui::restore();

    match result {
        Ok(()) => match app.output {
            Some(message) if commit => commit_with_git(&message),
            Some(message) => {
                println!("{message}");
                ExitCode::SUCCESS
            }
            None => ExitCode::SUCCESS,
        },
        Err(error) => {
            eprintln!("commit-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Run `git commit -m <message>`, letting git ask for a passphrase or print its
/// own errors, so the output looks like a normal commit.
fn commit_with_git(message: &str) -> ExitCode {
    match std::process::Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg(message)
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("commit-tui: cannot run git: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `--user "Full Name"`: write the identity, print the derived handle.
fn set_user(name: &str) -> ExitCode {
    match Config::save_identity(name) {
        Ok(path) => {
            let prefix = config::prefix_from_name(name).unwrap_or_default();
            println!("wrote {}", path.display());
            println!("user:   {}", name.trim());
            println!("handle: {prefix}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commit-tui: {error}");
            ExitCode::FAILURE
        }
    }
}

fn show_config(identity: bool) -> ExitCode {
    let (config, warnings) = load_config();
    if identity {
        println!("user:   {}", config.user.as_deref().unwrap_or("-"));
        println!("handle: {}", config.prefix.as_deref().unwrap_or("-"));
        if config.user.is_none() {
            println!(
                "       set one with `commit-tui --user \"Firstname Lastname\"` or on first start"
            );
        }
        return ExitCode::SUCCESS;
    }
    println!("profile: {}", config.profile);
    println!("user:    {}", config.user.as_deref().unwrap_or("-"));
    println!("types:   {}", config.type_names().join(", "));
    println!("scopes:  {}", join_or_dash(&config.scopes));
    println!(
        "limits:  subject {}/{} chars, history depth {}",
        config.recommended_subject, config.max_subject, config.history_depth
    );
    println!("emoji:   {}", if config.emoji { "on" } else { "off" });
    println!("files:");
    if config.sources.is_empty() {
        println!("  (none, using built-in defaults)");
    } else {
        for source in &config.sources {
            println!("  {}", source.display());
        }
    }
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    ExitCode::SUCCESS
}

fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

fn init_config(force: bool) -> ExitCode {
    let path = cwd().join(".commit-tui.toml");
    if path.exists() && !force {
        eprintln!(
            "commit-tui: {} exists, pass --force to overwrite",
            path.display()
        );
        return ExitCode::FAILURE;
    }
    let (config, _) = load_config();
    match std::fs::write(&path, config.to_toml()) {
        Ok(()) => {
            println!("wrote {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commit-tui: cannot write {}: {error}", path.display());
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
            if let Err(error) = std::io::stdin().read_to_string(&mut buffer) {
                eprintln!("commit-tui: cannot read stdin: {error}");
                return ExitCode::FAILURE;
            }
            buffer
        }
    };

    let (config, warnings) = load_config();
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    let rules = Rules::from_config(&config);
    let issues = validate::lint_with(&text, &rules);
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
