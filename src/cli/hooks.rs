use std::process::ExitCode;

use crate::HookAction;
use crate::hooks;
use crate::ui;

pub(crate) fn run_hooks(action: Option<HookAction>, yes: bool) -> ExitCode {
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

pub(crate) fn run_hooks_install(force: bool) -> ExitCode {
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
pub(crate) fn run_hooks_wizard() -> ExitCode {
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

pub(crate) fn run_hints() -> ExitCode {
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
pub(crate) fn run_prepare(file: &std::path::Path) -> ExitCode {
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
    let branch = commitsmith::infrastructure::process::here(&["symbolic-ref", "--short", "HEAD"]);
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

/// `commitsmith branch-hint`: one line about the branch just checked out.
///
/// Read by the `post-checkout` hook and printed to stderr, because that hook runs
/// before the shell prompt returns and its stdout belongs to git. Always exit 0:
/// a hook that fails after a successful checkout makes the checkout look broken.
///
/// Silent when the branch says nothing usable (`main`), so the common case is no
/// output at all.
pub(crate) fn run_branch_hint() -> ExitCode {
    let branch = commitsmith::infrastructure::process::here(&["symbolic-ref", "--short", "HEAD"]);
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
