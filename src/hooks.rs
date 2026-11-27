//! `commitsmith hooks` — install and remove the commit-msg hook.

use std::path::PathBuf;
use std::process::Command;

const MARKER: &str = "commitsmith";

/// The hook itself.
///
/// It looks for the binary instead of assuming it is on `PATH`: a GUI client
/// starts a hook with a different environment than a terminal, and a missing
/// binary used to abort the commit with "command not found". A colleague who has
/// no commitsmith installed gets one line and their commit, never a broken
/// workflow — the hook is a convenience, not a gate.
const SCRIPT: &str = "#!/bin/sh
# commitsmith commit-msg hook - checks the message before git records it.
# Looks for the binary first; if it is missing, the commit goes through.
hint='commitsmith not found - the message was not checked (git config commitsmith.binary <path>)'

from_config() {
    git config --get commitsmith.binary 2>/dev/null
}

usable() {
    [ -n \"$1\" ] && [ -x \"$1\" ]
}

binary=$(from_config)
if ! usable \"$binary\"; then
    binary=$(command -v commitsmith 2>/dev/null)
fi
if ! usable \"$binary\"; then
    for candidate in \"$HOME/.local/bin/commitsmith\" \"$HOME/.cargo/bin/commitsmith\" \"$LOCALAPPDATA/Programs/commitsmith/commitsmith.exe\"; do
        if usable \"$candidate\"; then
            binary=$candidate
            break
        fi
    done
fi

if ! usable \"$binary\"; then
    echo \"$hint\" >&2
    exit 0
fi

exec \"$binary\" check --file \"$1\"
";

/// Where git puts hooks for the current repository (works for worktrees and
/// for repositories with `core.hooksPath` set).
fn hook_path() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", "hooks"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("not inside a git repository".to_string());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(path).join("commit-msg"))
}

pub fn install(force: bool) -> Result<String, String> {
    let path = hook_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    if path.exists() {
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if existing.contains(MARKER) {
            // Our own hook: rewriting it is the update path.
        } else if force {
            // Back up before replacing someone else's hook.
            let backup = path.with_extension("bak");
            std::fs::copy(&path, &backup)
                .map_err(|error| format!("cannot back up {}: {error}", path.display()))?;
        } else {
            return Err(format!(
                "{} already exists and is not a commitsmith hook; pass --force to back it up and replace it",
                path.display()
            ));
        }
    }

    std::fs::write(&path, SCRIPT).map_err(|error| error.to_string())?;
    set_executable(&path)?;
    Ok(path.display().to_string())
}

pub fn uninstall() -> Result<String, String> {
    let path = hook_path()?;
    if !path.exists() {
        return Err(format!("no hook at {}", path.display()));
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if !existing.contains(MARKER) {
        return Err(format!(
            "{} does not belong to commitsmith, leaving it alone",
            path.display()
        ));
    }
    std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    Ok(path.display().to_string())
}

#[cfg(unix)]
fn set_executable(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn set_executable(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}
