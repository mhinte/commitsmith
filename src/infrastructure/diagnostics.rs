//! Startup diagnostics: what happened when the program died before it could say so.
//!
//! A TUI that exits without a word is impossible to debug from the outside. Every
//! failure during startup is therefore appended to a small log next to the config,
//! and the panic hook writes there too. `commitsmith --doctor` prints the same
//! facts on demand.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::infrastructure::config_toml::Config;

/// Where the log lives, if a config directory can be worked out at all.
pub fn log_path() -> Option<PathBuf> {
    Config::user_config_path().map(|path| path.with_file_name("startup.log"))
}

/// Append one line with a timestamp; never fails loudly, because it runs while
/// something else is already going wrong.
pub fn record(stage: &str, message: &str) {
    if let Some(path) = log_path() {
        let _ = record_at(&path, stage, message);
    }
}

/// Same as [`record`], for a caller that knows where the log belongs (tests).
pub fn record_at(path: &Path, stage: &str, message: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    writeln!(file, "{stamp} [{stage}] {message}")
}

/// The last lines of the log, newest last.
pub fn recent(limit: usize) -> Option<String> {
    let path = log_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(limit);
    Some(lines[start..].join("\n"))
}

/// What `--doctor` prints: enough to tell a path problem from a repository problem.
pub fn doctor(start: &Path) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "commitsmith {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "os:        {}", std::env::consts::OS);
    let _ = writeln!(out, "arch:      {}", std::env::consts::ARCH);
    let _ = writeln!(out, "cwd:       {}", start.display());

    for (name, value) in [
        ("HOME", std::env::var("HOME").ok()),
        ("USERPROFILE", std::env::var("USERPROFILE").ok()),
        ("APPDATA", std::env::var("APPDATA").ok()),
        ("LOCALAPPDATA", std::env::var("LOCALAPPDATA").ok()),
        ("XDG_CONFIG_HOME", std::env::var("XDG_CONFIG_HOME").ok()),
    ] {
        let _ = writeln!(
            out,
            "{name:<14}{}",
            value.unwrap_or_else(|| "<unset>".to_string())
        );
    }

    match Config::user_config_path() {
        Some(path) => {
            let _ = writeln!(out, "user config: {}", path.display());
            let _ = writeln!(out, "  exists:    {}", path.exists());
        }
        None => {
            let _ = writeln!(out, "user config: <cannot be worked out>");
        }
    }
    if let Some(log) = log_path() {
        let _ = writeln!(out, "startup log: {}", log.display());
    }

    match crate::infrastructure::gix_repo::GixRepo::open(start) {
        Ok(repo) => {
            let _ = writeln!(out, "repository:  {}", repo.workdir().display());
            match repo.current_branch() {
                Ok(Some(branch)) => {
                    let _ = writeln!(out, "  branch:    {}", branch.name);
                }
                Ok(None) => {
                    let _ = writeln!(out, "  branch:    <detached or unborn>");
                }
                Err(error) => {
                    let _ = writeln!(out, "  branch:    error: {error}");
                }
            }
            match repo.branches() {
                Ok(branches) => {
                    let _ = writeln!(out, "  branches:  {}", branches.len());
                }
                Err(error) => {
                    let _ = writeln!(out, "  branches:  error: {error}");
                }
            }
            match repo.recent_commits(5) {
                Ok(commits) => {
                    let _ = writeln!(out, "  commits:   {} (read up to 5)", commits.len());
                }
                Err(error) => {
                    let _ = writeln!(out, "  commits:   error: {error}");
                }
            }
        }
        Err(error) => {
            let _ = writeln!(out, "repository:  none");
            let _ = writeln!(out, "  reason:    {error}");
        }
    }

    let _ = writeln!(
        out,
        "git:         {}",
        match super::process::run(None, &["--version"], &[]) {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            Ok(output) => format!("exit {}", output.status),
            Err(error) => format!("not usable: {error}"),
        }
    );

    match recent(10) {
        Some(lines) if !lines.is_empty() => {
            let _ = writeln!(out, "\nlast startup problems:\n{lines}");
        }
        _ => {
            let _ = writeln!(out, "\nlast startup problems: none recorded");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-diag-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_recorded_failure_is_readable_again() {
        let dir = temp("record");
        let log = dir.join("nested").join("startup.log");
        record_at(&log, "repository", "gix could not read HEAD").unwrap();
        record_at(&log, "terminal", "poll failed").unwrap();
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.contains("[repository] gix could not read HEAD"),
            "{text}"
        );
        assert!(text.contains("[terminal] poll failed"), "{text}");
        assert_eq!(text.lines().count(), 2, "one line per failure");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recording_creates_the_directory() {
        let dir = temp("mkdir");
        let log = dir.join("a").join("b").join("startup.log");
        assert!(!log.parent().unwrap().exists());
        record_at(&log, "config", "no HOME").unwrap();
        assert!(log.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_doctor_reports_the_facts_a_bug_report_needs() {
        let dir = temp("doctor");
        let text = doctor(&dir);
        for expected in [
            "commitsmith ",
            "os:",
            "cwd:",
            "user config:",
            "repository:",
            "git:",
        ] {
            assert!(text.contains(expected), "{expected} missing in:\n{text}");
        }
        // The temp dir is not a repository, and the report says so instead of failing.
        assert!(text.contains("repository:  none"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
