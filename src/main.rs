//! commitsmith — compose and lint Conventional Commits in the terminal.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod app;
mod commit;
mod config;
mod git;
mod hooks;
mod install;
mod setup;
mod ui;
mod validate;

use config::Config;
use validate::Severity;

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
    },
    /// Write a starter .commitsmith.toml in the current directory
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

    if cli.install {
        return run_install(cli.alias.as_deref());
    }
    if cli.uninstall {
        return run_uninstall();
    }

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
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Keep the terminal usable when the program panics inside the alternate screen.
///
/// Without this a panic on Windows leaves the console in raw mode with a blank
/// screen and a hidden cursor, so the message that explains the problem is never
/// read. The hook restores the terminal first, then reports as usual.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        let _ = std::io::stdout().flush();
        default(info);
    }));
}

fn run_composer(commit: bool) -> ExitCode {
    install_panic_hook();

    // First start: ask once for the name, then never again.
    if Config::needs_setup() {
        let mut terminal = ratatui::init();
        let first_run = setup::run(&mut terminal);
        ratatui::restore();
        match first_run {
            Ok(setup::Outcome::Saved(path)) => eprintln!("commitsmith: wrote {}", path.display()),
            Ok(setup::Outcome::Skipped) => {
                eprintln!(
                    "commitsmith: no name set, run `commitsmith --user \"Firstname Lastname\"` later"
                )
            }
            Err(error) => {
                eprintln!("commitsmith: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    let (config, warnings) = load_config();
    let mut app = app::App::with_config(config, warnings);
    app.set_commit_mode(commit);

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
                // stderr stays out of the way of a pipe, but a person running
                // the composer should know why nothing was committed.
                eprintln!(
                    "commitsmith: message written to stdout, nothing committed · use -c/--commit"
                );
                ExitCode::SUCCESS
            }
            None => ExitCode::SUCCESS,
        },
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Run `git commit -m <message>`, letting git ask for a passphrase or print its
/// own errors, so the output looks like a normal commit.
fn commit_with_git(message: &str) -> ExitCode {
    let output = std::process::Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg(message)
        .output();

    match output {
        Ok(output) => {
            // git already wrote its own output, but stdout/stderr were captured so
            // the message survives on terminals that clear the screen on exit.
            let _ = std::io::stdout().write_all(&output.stdout);
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().write_all(&output.stderr);
            if output.status.success() {
                return ExitCode::SUCCESS;
            }
            eprintln!();
            eprintln!("commitsmith: git did not commit; the message is kept below.");
            eprintln!("--- 8< ---");
            for line in message.lines() {
                eprintln!("{line}");
            }
            eprintln!("--- 8< ---");
            eprintln!("retry with: git commit -F .git/COMMIT_EDITMSG");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("commitsmith: cannot run git: {error}");
            eprintln!("is git installed and on PATH?");
            ExitCode::FAILURE
        }
    }
}

fn run_install(alias: Option<&str>) -> ExitCode {
    match install::install(alias) {
        Ok(report) => {
            println!("installed {}", report.binary.display());
            if let Some(alias) = &report.alias {
                println!("alias     {}", alias.display());
            }
            println!("directory {}", report.directory.display());
            if report.added_to_path {
                println!("added the directory to your PATH");
                if report.restart_shell {
                    println!("open a new terminal so it takes effect");
                }
            } else {
                println!("the directory was already on your PATH");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_uninstall() -> ExitCode {
    match install::uninstall() {
        Ok(report) => {
            println!("removed {}", report.binary.display());
            if let Some(alias) = &report.alias {
                println!("removed {}", alias.display());
            }
            println!(
                "the PATH entry stays; remove {} by hand if you want it gone",
                report.directory.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
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
            eprintln!("commitsmith: {error}");
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
                "       set one with `commitsmith --user \"Firstname Lastname\"` or on first start"
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
    let path = cwd().join(".commitsmith.toml");
    if path.exists() && !force {
        eprintln!(
            "commitsmith: {} exists, pass --force to overwrite",
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
            eprintln!("commitsmith: cannot write {}: {error}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn run_check(file: Option<PathBuf>, message: Option<String>, strict: bool) -> ExitCode {
    let text = match (file, message) {
        (Some(path), _) => match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("commitsmith: cannot read {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        },
        (None, Some(message)) => message,
        (None, None) => {
            let mut buffer = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut buffer) {
                eprintln!("commitsmith: cannot read stdin: {error}");
                return ExitCode::FAILURE;
            }
            buffer
        }
    };

    let (config, warnings) = load_config();
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    let rules = validate::rules_from_config(&config);
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
