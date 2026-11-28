use std::io::Write;
use std::process::ExitCode;

use super::{RepoData, cwd, install_panic_hook, load_config};
use crate::app;
use crate::config::Config;
use crate::git;
use crate::hooks;
use crate::ui;

pub(crate) fn run_composer(commit: bool) -> ExitCode {
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

/// Read scopes, the status line and recent commits; `None` outside a repository.
///
/// Runs inside `catch_unwind`, so a repository that makes the reader panic
/// (odd `HEAD`, unreadable config, whatever a platform does differently) costs
/// the scope suggestions instead of the whole session.
pub(crate) fn read_repository(
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
pub(crate) fn run_setup_dialog(config: &Config) -> Result<(), String> {
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
pub(crate) fn commit_with_git(message: &str) -> ExitCode {
    git_commit(message, false)
}

/// Same as [`commit_with_git`], but the message replaces the last commit.
pub(crate) fn amend_with_git(message: &str) -> ExitCode {
    git_commit(message, true)
}

pub(crate) fn git_commit(message: &str, amend: bool) -> ExitCode {
    let mut args = vec!["commit"];
    if amend {
        args.push("--amend");
    }
    args.extend(["-m", message]);
    let output = commitsmith::infrastructure::process::run(None, &args, &[]);

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
