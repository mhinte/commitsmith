//! `commitsmith hooks` — install and remove the commit-msg hook.

use std::path::PathBuf;
use std::process::Command;

/// The line that makes a hook ours. The bare word "commitsmith" was not enough:
/// a hand-written hook that calls `commitsmith check` counted as ours and was
/// overwritten. Every script this module ever wrote carries this line.
const MARKER: &str = "# commitsmith";

/// Where a foreign hook goes while ours is installed. Ours runs it first and
/// `uninstall` puts it back.
const SIDECAR: &str = ".pre-commitsmith";

/// The hooks this module writes, with the line that makes each one ours.
struct Hook {
    name: &'static str,
    script: &'static str,
}

const HOOKS: [Hook; 2] = [
    Hook {
        name: "commit-msg",
        script: SCRIPT_COMMIT_MSG,
    },
    Hook {
        name: "prepare-commit-msg",
        script: SCRIPT_PREPARE_COMMIT_MSG,
    },
];

/// The hook itself.
///
/// It looks for the binary instead of assuming it is on `PATH`: a GUI client
/// starts a hook with a different environment than a terminal, and a missing
/// binary used to abort the commit with "command not found". A colleague who has
/// no commitsmith installed gets one line and their commit, never a broken
/// workflow — the hook is a convenience, not a gate.
const SCRIPT_COMMIT_MSG: &str = "#!/bin/sh
# commitsmith commit-msg hook - checks the message before git records it.
# Looks for the binary first; if it is missing, the commit goes through.

# A hook that was here before commitsmith runs first; its verdict stands.
previous=\"$(dirname \"$0\")/commit-msg.pre-commitsmith\"
if [ -x \"$previous\" ]; then
    \"$previous\" \"$@\" || exit $?
fi

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

/// Prefill: when a human opens an editor for a new message, the first line is
/// already started (`feat(TICKET-12): `).
///
/// Three silences are deliberate. Any `$2` means git supplied the message
/// itself — merge, squash, `--amend`, `-m`, `-F`, template — and none of those
/// want a guess in front. A missing binary says nothing at all, because a
/// convenience the user does not have should not talk on every commit. And a
/// branch that names no kind produces no prefix, rather than a wrong one.
const SCRIPT_PREPARE_COMMIT_MSG: &str = "#!/bin/sh
# commitsmith prepare-commit-msg hook - starts a new message with type(ticket): .

# A hook that was here before commitsmith runs first; its verdict stands.
previous=\"$(dirname \"$0\")/prepare-commit-msg.pre-commitsmith\"
if [ -x \"$previous\" ]; then
    \"$previous\" \"$@\" || exit $?
fi

# $2 says where the message comes from. Only an editor opened by a person has
# none; everything else is left exactly as git wrote it.
if [ -n \"$2\" ]; then
    exit 0
fi

usable() {
    [ -n \"$1\" ] && [ -x \"$1\" ]
}

binary=$(git config --get commitsmith.binary 2>/dev/null)
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

# No binary: no suggestion, and not a word about it.
if ! usable \"$binary\"; then
    exit 0
fi

exec \"$binary\" prepare --file \"$1\"
";

/// Where git puts hooks for the current repository (works for worktrees and
/// for repositories with `core.hooksPath` set).
fn hooks_dir() -> Result<PathBuf, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", "hooks"])
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err("not inside a git repository".to_string());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

/// Remember where this binary lives, so the hook does not have to guess.
///
/// The hook checks `git config commitsmith.binary` first. Nothing wrote that
/// value, so an unusual install path (a zip unpacked into `~/tools`, a portable
/// build) relied on the fallback list. The value is repo-local and never
/// committed; a colleague on another machine has a different one and the hook
/// falls back to `PATH` when the stored path is not executable.
fn remember_binary() {
    let Ok(binary) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new("git")
        .args(["config", "commitsmith.binary"])
        .arg(binary)
        .output();
}

/// Hook managers that rebuild their hook directory on every install. A hook we
/// put there disappears silently with the next `npm install` or `lefthook
/// install`, so the user has to say `--force` knowing that.
const MANAGERS: [(&str, &str); 3] = [
    (
        "husky",
        "add `commitsmith check --file \"$1\"` to .husky/commit-msg",
    ),
    (
        "lefthook",
        "add `commitsmith check --file {1}` under commit-msg in lefthook.yml",
    ),
    (
        "pre-commit",
        "add a commitsmith hook with `stages: [commit-msg]`",
    ),
];

/// Which hook manager owns this directory or this hook, if any.
fn manager_of(dir: &std::path::Path, existing: &str) -> Option<(&'static str, &'static str)> {
    let in_husky = dir
        .components()
        .any(|part| part.as_os_str().to_string_lossy().starts_with(".husky"));
    if in_husky {
        return Some(MANAGERS[0]);
    }
    let lower = existing.to_lowercase();
    if lower.contains("lefthook") {
        return Some(MANAGERS[1]);
    }
    if lower.contains("generated by pre-commit") || lower.contains("pre-commit.com") {
        return Some(MANAGERS[2]);
    }
    None
}

/// What `install` did, in words for the terminal.
#[derive(Debug, PartialEq, Eq)]
pub struct Installed {
    pub paths: Vec<String>,
    /// Foreign hooks that now run before ours, if there were any.
    pub chained: Vec<String>,
}

/// Install our hooks where git reads hooks — `.git/hooks`, or wherever
/// `core.hooksPath` points (ticket E8-2, variant B).
///
/// A foreign hook is never replaced: it moves to `<name>.pre-commitsmith` and
/// our script runs it first, passing its exit code on. A directory a hook manager
/// regenerates (husky, lefthook, pre-commit) needs `--force`, because our hook
/// would vanish with that tool's next install.
pub fn install(force: bool) -> Result<Installed, String> {
    let dir = hooks_dir()?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;

    let mut done = Installed {
        paths: Vec::new(),
        chained: Vec::new(),
    };
    for hook in &HOOKS {
        let path = dir.join(hook.name);
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let ours = existing.contains(MARKER);
        if !ours && !force {
            if let Some((tool, advice)) = manager_of(&dir, &existing) {
                return Err(format!(
                    "{} is managed by {tool}\n\
                     {tool} rewrites it on its next install and drops our hook\n\
                     instead: {advice}\n\
                     or pass --force to install anyway",
                    dir.display()
                ));
            }
        }

        let sidecar = dir.join(format!("{}{SIDECAR}", hook.name));
        if path.exists() && !ours {
            if sidecar.exists() {
                return Err(format!(
                    "{} and {} both exist\nmove one of them away first, \
                     commitsmith will not overwrite a hook",
                    path.display(),
                    sidecar.display()
                ));
            }
            std::fs::rename(&path, &sidecar)
                .map_err(|error| format!("cannot move {}: {error}", path.display()))?;
            done.chained.push(sidecar.display().to_string());
        }

        std::fs::write(&path, hook.script).map_err(|error| error.to_string())?;
        set_executable(&path)?;
        done.paths.push(path.display().to_string());
    }
    remember_binary();
    Ok(done)
}

/// Remove our hooks and put back the ones they were chained to, if any.
pub fn uninstall() -> Result<Vec<String>, String> {
    let dir = hooks_dir()?;
    let mut removed = Vec::new();
    let mut found = false;
    for hook in &HOOKS {
        let path = dir.join(hook.name);
        if !path.exists() {
            continue;
        }
        found = true;
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if !existing.contains(MARKER) {
            // Someone else's hook: not ours to remove.
            continue;
        }
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        let sidecar = dir.join(format!("{}{SIDECAR}", hook.name));
        if sidecar.exists() {
            std::fs::rename(&sidecar, &path)
                .map_err(|error| format!("cannot restore {}: {error}", sidecar.display()))?;
            removed.push(format!("{} (your previous hook is back)", path.display()));
        } else {
            removed.push(path.display().to_string());
        }
    }
    if removed.is_empty() {
        return Err(match found {
            true => format!("no commitsmith hook in {}", dir.display()),
            false => format!("no hook at {}", dir.display()),
        });
    }
    Ok(removed)
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
