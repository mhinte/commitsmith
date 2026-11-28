//! The CLI surface: one module per command.
//!
//! `main.rs` parses the arguments and dispatches; everything else lives here. The
//! file had grown to 1009 lines with twenty `run_*` functions, and every new hook
//! command made it longer.

pub(crate) mod check;
pub(crate) mod composer;
pub(crate) mod config;
pub(crate) mod hooks;
pub(crate) mod install;
pub(crate) mod update;

pub(crate) use check::run_check;
pub(crate) use composer::run_composer;
pub(crate) use config::{init_config, show_config};
pub(crate) use hooks::{run_branch_hint, run_hints, run_hooks, run_prepare};
pub(crate) use install::{run_install, run_uninstall, set_user};
pub(crate) use update::run_update;

use std::io::Write;
use std::path::PathBuf;

use crate::config::Config;
use crate::git;

pub(crate) fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub(crate) fn load_config() -> (Config, Vec<String>) {
    Config::load(&cwd())
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

/// What the composer needs from the repository, read in one guarded step.
pub(crate) struct RepoData {
    pub(crate) scopes: git::ScopeInfo,
    pub(crate) status: String,
    pub(crate) history: Vec<git::PastCommit>,
}
