//! commitsmith — compose and lint Conventional Commits in the terminal.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use commitsmith::domain::ports::ConfigStore as _;

mod app;
mod application;
mod commit;
mod config;
mod git;
mod hooks;
mod install;
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
enum HookAction {
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
            commitsmith::infrastructure::diagnostics::doctor(&cwd())
        );
        return ExitCode::SUCCESS;
    }
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
        Some(Command::Init { defaults, force }) => init_config(defaults, force),
        Some(Command::Hooks { action, yes }) => run_hooks(action, yes),
        Some(Command::Prepare { file }) => run_prepare(&file),
        Some(Command::Hints) => run_hints(),
        Some(Command::BranchHint) => run_branch_hint(),
        Some(Command::Update { check }) => run_update(check),
        Some(Command::Check {
            file,
            message,
            strict,
            fix,
        }) => run_check(file, message, strict, fix),
    }
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn load_config() -> (Config, Vec<String>) {
    Config::load(&cwd())
}

fn run_hooks(action: Option<HookAction>, yes: bool) -> ExitCode {
    // No action means a person typed `commitsmith hooks` and wants guidance.
    // `--yes` keeps a script working on the same command line.
    let Some(action) = action else {
        return match yes {
            true => run_hooks_install(false),
            false => run_hooks_wizard(),
        };
    };
    let result = match action {
        HookAction::Install { force } => hooks::install(force).map(|done| {
            let mut message = done.paths.join("\n");
            for previous in done.chained {
                message.push_str(&format!(
                    "\nyour existing hook moved to {previous} and runs first"
                ));
            }
            message
        }),
        HookAction::Uninstall => hooks::uninstall().map(|paths| {
            paths
                .into_iter()
                .map(|path| format!("removed {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        }),
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

fn run_hooks_install(force: bool) -> ExitCode {
    match hooks::install(force) {
        Ok(done) => {
            let mut message = done.paths.join("\n");
            for previous in done.chained {
                message.push_str(&format!(
                    "\nyour existing hook moved to {previous} and runs first"
                ));
            }
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The wizard needs a terminal; without one it says how to do it in a script.
fn run_hooks_wizard() -> ExitCode {
    use std::io::IsTerminal;

    if !std::io::stdout().is_terminal() {
        eprintln!("commitsmith: no terminal for the wizard");
        eprintln!("run `commitsmith hooks install` or `commitsmith hooks --yes`");
        return ExitCode::FAILURE;
    }
    let mut terminal = ratatui::init();
    let outcome = ui::hooks_wizard::run(&mut terminal);
    ratatui::restore();
    match outcome {
        Ok(ui::hooks_wizard::Outcome::Done(Ok(text))) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Ok(ui::hooks_wizard::Outcome::Done(Err(error))) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
        Ok(ui::hooks_wizard::Outcome::Skipped) => {
            println!("nothing installed");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `commitsmith branch-hint`: one line about the branch just checked out.
///
/// Read by the `post-checkout` hook and printed to stderr, because that hook runs
/// before the shell prompt returns and its stdout belongs to git. Always exit 0:
/// a hook that fails after a successful checkout makes the checkout look broken.
///
/// Silent when the branch says nothing usable (`main`), so the common case is no
/// output at all.
fn run_branch_hint() -> ExitCode {
    let branch = std::process::Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
    // A detached HEAD has nothing to name.
    let Some(branch) = branch.filter(|name| !name.is_empty()) else {
        return ExitCode::SUCCESS;
    };
    let context = commitsmith::domain::prefill::context(&branch);
    if context.is_empty() {
        return ExitCode::SUCCESS;
    }
    let mut parts = vec![format!("commitsmith: branch {branch}")];
    if let Some(kind) = context.kind {
        parts.push(format!("type {kind}"));
    }
    if let Some(ticket) = context.ticket {
        parts.push(ticket);
    }
    eprintln!("{}", parts.join(" · "));
    ExitCode::SUCCESS
}

/// `commitsmith hints`: say what is worth saying about the index, then get out
/// of the way.
///
/// Always exit 0. The hints go to stderr so a hook never writes into a pipe
/// somebody parses, and a broken installation (no repository, unreadable index)
/// is not a reason to stop a commit.
/// `commitsmith update`: fetch the newest release, verify it, put it in place.
///
/// Exit codes are part of the contract: 0 means "nothing to do or done", 1 means
/// "something went wrong", 2 means "there is a newer release" — so a script can
/// ask with `--check` and act on the answer.
fn run_update(check_only: bool) -> ExitCode {
    use commitsmith::infrastructure::update::{self, Outcome};

    match update::run(check_only) {
        Ok(Outcome::UpToDate { .. }) => {
            println!(
                "commitsmith {} is the newest release",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        Ok(Outcome::Updated { from, to }) => {
            if check_only {
                println!("commitsmith {to} is available (you run {from})");
                println!("run `commitsmith update` to install it");
                return ExitCode::from(2);
            }
            let pending = std::env::current_exe()
                .ok()
                .is_some_and(|path| commitsmith::infrastructure::update::is_pending(&path));
            match pending {
                true => println!(
                    "commitsmith {from} -> {to}\nthe new binary is staged and replaces this one when it exits"
                ),
                false => println!("commitsmith {from} -> {to}"),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_hints() -> ExitCode {
    use commitsmith::domain::ports::GitRepository;
    use commitsmith::infrastructure::git_cli::GitBackend;

    let Ok(root) = std::env::current_dir() else {
        return ExitCode::SUCCESS;
    };
    let Ok(backend) = GitBackend::open(&root) else {
        return ExitCode::SUCCESS;
    };
    let staged: Vec<String> = backend
        .changes()
        .unwrap_or_default()
        .into_iter()
        .filter(commitsmith::domain::ports::ChangedFile::is_staged)
        .map(|file| file.path)
        .collect();
    if staged.is_empty() {
        return ExitCode::SUCCESS;
    }
    let diff = backend.staged_diff().unwrap_or_default();
    let branch = backend
        .current_branch()
        .ok()
        .flatten()
        .map(|branch| branch.name);
    for hint in commitsmith::domain::hints::for_staged(branch.as_deref(), &staged, &diff) {
        eprintln!("hint: {hint}");
    }
    ExitCode::SUCCESS
}

/// `commitsmith prepare --file .git/COMMIT_EDITMSG`: leave a starting point for
/// the writer, and never get in the way of the commit.
///
/// Exit code is 0 in every case that is not a broken installation: a hook that
/// refuses a commit because it could not read a file is worse than no hook.
fn run_prepare(file: &std::path::Path) -> ExitCode {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("commitsmith: cannot read {}: {error}", file.display());
            return ExitCode::SUCCESS;
        }
    };
    if !commitsmith::domain::prefill::is_free(&text) {
        return ExitCode::SUCCESS;
    }
    let branch = std::process::Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
    // A detached HEAD has no branch to read: nothing to suggest.
    let Some(branch) = branch else {
        return ExitCode::SUCCESS;
    };
    let Some(prefix) = commitsmith::domain::prefill::prefix(&branch) else {
        return ExitCode::SUCCESS;
    };
    let Some(text) = commitsmith::domain::prefill::with_prefix(&text, &prefix) else {
        return ExitCode::SUCCESS;
    };
    if let Err(error) = std::fs::write(file, text) {
        eprintln!("commitsmith: cannot write {}: {error}", file.display());
    }
    ExitCode::SUCCESS
}

/// Keep the terminal usable when the program panics inside the alternate screen.
///
/// Without this a panic on Windows leaves the console in raw mode with a blank
/// screen and a hidden cursor, so the message that explains the problem is never
/// read. The hook restores the terminal first, then reports as usual.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // A panic in a TUI leaves nothing on screen; the log is the only trace
        // a user can send.
        let message = match info.payload().downcast_ref::<&str>() {
            Some(text) => (*text).to_string(),
            None => match info.payload().downcast_ref::<String>() {
                Some(text) => text.clone(),
                None => "panic without a message".to_string(),
            },
        };
        let location = info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()))
            .unwrap_or_else(|| "unknown".to_string());
        commitsmith::infrastructure::diagnostics::record(
            "panic",
            &format!("{location}: {message}"),
        );
        ratatui::restore();
        let _ = std::io::stdout().flush();
        default(info);
    }));
}

fn run_composer(commit: bool) -> ExitCode {
    install_panic_hook();

    loop {
        // First start: ask once for the name, then never again.
        if Config::needs_setup() {
            let mut terminal = ratatui::init();
            let first_run = ui::setup::run(&mut terminal);
            ratatui::restore();
            match first_run {
                Ok(ui::setup::Outcome::Saved(path)) => {
                    eprintln!("commitsmith: wrote {}", path.display())
                }
                Ok(ui::setup::Outcome::Skipped) => {
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
        // Ctrl+O writes the folded/unfolded choice into the user config.
        app.set_config_store(Box::new(
            commitsmith::infrastructure::config_toml::TomlConfigStore::new(),
        ));
        // Tab 3 writes CHANGELOG.md into the working directory it was started in.
        app.set_changelog_root(cwd());

        // Scope suggestions come from history; a missing repository is fine, and
        // so is one that cannot be read: the composer still opens.
        let root = cwd();
        let configured = app.config.scopes.clone();
        let depth = app.config.history_depth;
        let history_depth = depth.min(200);
        let repo_data = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_repository(&root, &configured, depth, history_depth)
        }));
        match repo_data {
            Ok(Some(data)) => {
                app.set_in_repository(true);
                // A hook that is not installed is invisible until a bad commit
                // slips through; the footer says it once at startup.
                app.set_hooks_missing(&hooks::missing());
                app.set_scopes(data.scopes);
                app.set_repo_line(data.status);
                app.set_history(data.history);
                // The history preview reads a commit's files through the port
                // when it is highlighted — no `gix::discover` per keystroke, one
                // `git show`, and a failure lands in the status line.
                // Tab 2 acts on the repository: reads through gix, writes
                // through `git`, behind the port.
                match commitsmith::infrastructure::git_cli::GitBackend::open(&root) {
                    Ok(backend) => app.set_git(Box::new(backend)),
                    Err(error) => app.status = error.to_string(),
                }
            }
            // No repository: the composer still works (the message can be
            // linted and printed), but it says so instead of looking normal.
            Ok(None) => app.set_in_repository(false),
            Err(_) => {
                // Recorded by the panic hook; tell the user where to look.
                commitsmith::infrastructure::diagnostics::record(
                    "repository",
                    &format!("reading {} failed", root.display()),
                );
                app.status = "repository could not be read · see startup.log".to_string();
            }
        }

        let mut terminal = ratatui::init();
        let result = ui::run(&mut terminal, &mut app);
        ratatui::restore();

        if let Err(error) = result {
            eprintln!("commitsmith: {error}");
            return ExitCode::FAILURE;
        }

        // `S` asked for the setup dialog: run it, then start the composer again
        // so the new values are in effect without a restart.
        if app.setup_request {
            if let Err(error) = run_setup_dialog(&app.config) {
                eprintln!("commitsmith: {error}");
                return ExitCode::FAILURE;
            }
            continue;
        }

        return match app.output {
            // Ctrl+A turned the composer into an amend: the message replaces the
            // last commit instead of creating a new one.
            Some(message) if commit && app.form.amend => amend_with_git(&message),
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
        };
    }
}

/// What the composer needs from the repository, read in one guarded step.
struct RepoData {
    scopes: git::ScopeInfo,
    status: String,
    history: Vec<git::PastCommit>,
}

/// Read scopes, the status line and recent commits; `None` outside a repository.
///
/// Runs inside `catch_unwind`, so a repository that makes the reader panic
/// (odd `HEAD`, unreadable config, whatever a platform does differently) costs
/// the scope suggestions instead of the whole session.
fn read_repository(
    root: &std::path::Path,
    configured: &[String],
    depth: usize,
    history_depth: usize,
) -> Option<RepoData> {
    let repo = git::open(root)?;
    let branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string())
        .unwrap_or_else(|| "detached".to_string());
    let staged = git::staged_summary(root).len();
    // One walk fills scopes and history; file lists are not read here.
    let (scopes, history) = git::snapshot(&repo, configured, depth, history_depth);
    Some(RepoData {
        scopes,
        status: format!("{branch} · {staged} staged"),
        history,
    })
}

/// The setup dialog from inside the composer: ask, then store.
fn run_setup_dialog(config: &Config) -> Result<(), String> {
    let profiles: Vec<String> = config.profiles.keys().cloned().collect();
    let current = commitsmith::domain::setup::SetupAnswers::defaults(&profiles)
        .with_author(config.author.as_deref().unwrap_or("mmu"))
        .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles))
        .with_ticket_prefix(&config.ticket_prefix)
        .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles))
        .with_reference(&config.reference)
        .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles))
        .with_template(&config.branch_template)
        .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles));

    // The wizard asks the same questions as `commitsmith init`, so it starts from
    // the values in effect instead of from defaults.
    let mut current = current;
    current.profile.clone_from(&config.profile);
    current.emoji = config.emoji;
    current.co_author = config.co_author;
    let mut wizard = ui::wizard::Wizard::new(current, profiles);
    let mut terminal = ratatui::init();
    let outcome = ui::wizard::run(&mut terminal, &mut wizard);
    ratatui::restore();

    match outcome.map_err(|error| error.to_string())? {
        ui::wizard::Outcome::Cancelled => Ok(()),
        ui::wizard::Outcome::Saved(answers) => {
            let store = commitsmith::infrastructure::config_toml::TomlConfigStore::new();
            let path = commitsmith::infrastructure::config_toml::save_setup(&store, &answers)
                .map_err(|error| error.to_string())?;
            eprintln!("commitsmith: saved {}", path.display());
            if answers.git_alias {
                let binary = std::env::current_exe()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "commitsmith".to_string());
                if let Err(error) =
                    commitsmith::infrastructure::git_cli::install_alias("smith", &binary)
                {
                    eprintln!("commitsmith: alias not set: {error}");
                }
            }
            Ok(())
        }
    }
}

/// Run `git commit -m <message>`, letting git ask for a passphrase or print its
/// own errors, so the output looks like a normal commit.
fn commit_with_git(message: &str) -> ExitCode {
    git_commit(message, false)
}

/// Same as [`commit_with_git`], but the message replaces the last commit.
fn amend_with_git(message: &str) -> ExitCode {
    git_commit(message, true)
}

fn git_commit(message: &str, amend: bool) -> ExitCode {
    let mut command = std::process::Command::new("git");
    command.arg("commit");
    if amend {
        command.arg("--amend");
    }
    let output = command.arg("-m").arg(message).output();

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
    println!("author:  {}", config.author.as_deref().unwrap_or("-"));
    println!("tickets: {}-1234", config.ticket_prefix);
    println!("footer:  {}", config.reference);
    println!(
        "co-author: {}",
        if config.co_author {
            config
                .co_author_name
                .clone()
                .or_else(|| config.prefix.clone())
                .unwrap_or_else(|| "on, but no handle yet".to_string())
        } else {
            "off".to_string()
        }
    );
    println!("branch:  {}", config.branch_template);
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

/// `commitsmith init`: ask the setup questions and store them for this user.
///
/// The project file is still written when one exists or `--force` is given, so
/// the wizard can also provide a repository default without touching the user
/// config.
fn init_config(defaults: bool, force: bool) -> ExitCode {
    let store = commitsmith::infrastructure::config_toml::TomlConfigStore::new();
    let existing = store.user_document().ok().flatten();
    // `--defaults` skips the questions, not the confirmation: an existing file
    // is only replaced on request.
    if existing.is_some() && !force {
        eprintln!(
            "commitsmith: {} exists, pass --force to overwrite",
            existing
                .as_ref()
                .map(|doc| doc.path.display().to_string())
                .unwrap_or_default()
        );
        return ExitCode::FAILURE;
    }

    let (config, _) = load_config();
    let profiles: Vec<String> = config.profiles.keys().cloned().collect();
    let current = if existing.is_some() {
        Some(
            commitsmith::domain::setup::SetupAnswers::defaults(&profiles)
                .with_author(config.author.as_deref().unwrap_or("mmu"))
                .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles))
                .with_ticket_prefix(&config.ticket_prefix)
                .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles))
                .with_reference(&config.reference)
                .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles)),
        )
    } else {
        None
    };

    let Some(answers) = ui::init::ask(&ui::init::Options { defaults }, &profiles, current.as_ref())
    else {
        eprintln!("commitsmith: setup aborted, nothing written");
        return ExitCode::FAILURE;
    };

    match commitsmith::infrastructure::config_toml::save_setup(&store, &answers) {
        Ok(path) => {
            println!("wrote {}", path.display());
            println!("author   {}", answers.author);
            println!("tickets  {}-1234", answers.ticket_prefix);
            println!("footer   {}", answers.reference);
            println!("branches {}", answers.branch_template);
            if answers.git_alias {
                let binary = std::env::current_exe()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "commitsmith".to_string());
                match commitsmith::infrastructure::git_cli::install_alias("smith", &binary) {
                    Ok(()) => println!("git alias: git smith"),
                    Err(error) => eprintln!("commitsmith: alias not set: {error}"),
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_check(file: Option<PathBuf>, message: Option<String>, strict: bool, fix: bool) -> ExitCode {
    let file_path = file.clone();
    let mut text = match (file, message) {
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

    // `--fix` writes the repaired message back into the file the hook was given and
    // reports one line per correction. It repairs, it never decides: the lint below runs
    // on the result, so a message that is wrong for a reason the repair cannot know is
    // still refused.
    if fix {
        let Some(path) = file_path.as_deref() else {
            eprintln!("commitsmith: --fix needs --file");
            return ExitCode::FAILURE;
        };
        let repaired = commitsmith::domain::repair::repair(&text);
        for correction in &repaired.fixes {
            eprintln!("fixed: {}", correction.what);
        }
        if !repaired.fixes.is_empty() {
            if let Err(error) = std::fs::write(path, &repaired.text) {
                eprintln!("commitsmith: cannot write {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
            text = repaired.text;
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

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
