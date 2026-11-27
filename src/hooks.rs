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

/// One hook: what git calls it, one line about what it does, and the text it adds
/// to the shared prelude.
struct Hook {
    name: &'static str,
    about: &'static str,
    body: &'static str,
}

impl Hook {
    /// The whole script, assembled from the shared prelude and this hook's body.
    fn script(&self) -> String {
        script(self.name, self.about, self.body)
    }
}

/// The four hooks, in the order git runs them.
const HOOKS: [Hook; 4] = [
    Hook {
        name: "commit-msg",
        about: "checks the message before git records it",
        body: BODY_COMMIT_MSG,
    },
    Hook {
        name: "prepare-commit-msg",
        about: "starts a new message from the branch name",
        body: BODY_PREPARE_COMMIT_MSG,
    },
    Hook {
        name: "pre-commit",
        about: "hints about the index, never blocks",
        body: BODY_PRE_COMMIT,
    },
    Hook {
        name: "post-checkout",
        about: "one line about the branch after a switch",
        body: BODY_POST_CHECKOUT,
    },
];

/// What every hook script shares: the chain to a previous hook and the search for
/// our binary.
///
/// Both used to be copied into all four scripts as shell text. The search changed
/// once (E8-12, `git config commitsmith.binary` was added) and had to be corrected
/// in four places — one of them was missed on the first try. It exists once now, and
/// the scripts are assembled from it at install time.
///
/// `$previous_hook` is set by [`script`] before this text runs; the search leaves
/// the result in `$binary` and `$usable` says whether it found something.
const PRELUDE: &str = r#"# A hook that was here before commitsmith runs first; its verdict stands.
previous="$(dirname "$0")/$previous_hook.pre-commitsmith"
if [ -x "$previous" ]; then
    "$previous" "$@" || exit $?
fi

usable() {
    [ -n "$1" ] && [ -x "$1" ]
}

binary=$(git config --get commitsmith.binary 2>/dev/null)
if ! usable "$binary"; then
    binary=$(command -v commitsmith 2>/dev/null)
fi
if ! usable "$binary"; then
    for candidate in "$HOME/.local/bin/commitsmith" "$HOME/.cargo/bin/commitsmith" "$LOCALAPPDATA/Programs/commitsmith/commitsmith.exe"; do
        if usable "$candidate"; then
            binary=$candidate
            break
        fi
    done
fi
"#;

/// Assemble a hook: shebang, one line saying what it does, the marker that makes it
/// ours, the shared prelude, then the hook's own body.
///
/// The body may assume `$binary` is set and that a previous hook already ran; it
/// decides what to do when `$binary` is empty (one of the three silence rules).
fn script(name: &str, about: &str, body: &str) -> String {
    format!("#!/bin/sh\n{MARKER} {name} hook - {about}\n\nprevious_hook='{name}'\n{PRELUDE}{body}")
}

/// The hook itself.
///
/// It looks for the binary instead of assuming it is on `PATH`: a GUI client
/// starts a hook with a different environment than a terminal, and a missing
/// binary used to abort the commit with "command not found". A colleague who has
/// no commitsmith installed gets one line and their commit, never a broken
/// workflow — the hook is a convenience, not a gate.
/// Checks the message before git records it. A missing binary lets the commit
/// through with one line, because an unchecked commit is better than a blocked one
/// for someone who never installed the tool.
const BODY_COMMIT_MSG: &str = r#"
if ! usable "$binary"; then
    echo "commitsmith not found - the message was not checked (git config commitsmith.binary <path>)" >&2
    exit 0
fi

# --fix repairs what is unambiguous (a trailing period, `Feat`, spacing) and
# reports it; what is still wrong after that is refused as before.
exec "$binary" check --file "$1" --fix
"#;

/// Starts a new message a human is about to write (`feat: `), and stays out of the
/// way in every other case: any `$2` means git supplied the message itself (merge,
/// squash, `--amend`, `-m`, `-F`, template), and a missing binary says nothing at
/// all — a convenience the user does not have must not talk on every commit.
const BODY_PREPARE_COMMIT_MSG: &str = r#"
# $2 says where the message comes from. Only an editor opened by a person has
# none; everything else is left exactly as git wrote it.
if [ -n "$2" ]; then
    exit 0
fi

# No binary: no suggestion, and not a word about it.
if ! usable "$binary"; then
    exit 0
fi

exec "$binary" prepare --file "$1"
"#;

/// Hints before the message is written. It warns and never blocks — `exit 0` in every
/// path, because a pre-commit hook that refuses commits is the one people learn to
/// bypass with `--no-verify`.
const BODY_PRE_COMMIT: &str = r#"
# No binary: nothing to say, and no delay worth mentioning.
if ! usable "$binary"; then
    exit 0
fi

"$binary" hints
exit 0
"#;

/// One line about the branch just checked out, and only for a branch switch: `$3`
/// is `1` for a branch and `0` for a file checkout. Nothing prints without a binary.
const BODY_POST_CHECKOUT: &str = r#"
# Only a branch switch (flag 1). A file checkout (0) says nothing.
if [ "$3" != "1" ]; then
    exit 0
fi

if ! usable "$binary"; then
    exit 0
fi

"$binary" branch-hint
exit 0
"#;

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

/// Which of our hooks are missing in this repository, by name.
///
/// Read-only and quiet: it is asked once when the program starts, so the answer
/// has to be cheap and free of side effects. An empty list means "all three are
/// there, or there is nothing to compare with".
pub fn missing() -> Vec<&'static str> {
    let Ok(dir) = hooks_dir() else {
        return Vec::new();
    };
    HOOKS
        .iter()
        .filter(|hook| {
            let path = dir.join(hook.name);
            match std::fs::read_to_string(&path) {
                Ok(text) => !text.contains(MARKER),
                // Missing file, or unreadable: either way not ours.
                Err(_) => true,
            }
        })
        .map(|hook| hook.name)
        .collect()
}

/// Install only the named hooks, leaving the others alone.
///
/// `hooks install` writes all three; the wizard lets someone take just the one
/// that checks the message, and that must not quietly write the other two.
pub fn install_selected(names: &[&str], force: bool) -> Result<Installed, String> {
    let wanted: Vec<&Hook> = HOOKS
        .iter()
        .filter(|hook| names.contains(&hook.name))
        .collect();
    if wanted.is_empty() {
        return Err(format!(
            "no such hook · choose from {}",
            HOOKS
                .iter()
                .map(|hook| hook.name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    install_these(&wanted, force)
}

/// The directory git reads hooks from, for the wizard to show.
pub fn hooks_dir_display() -> Option<String> {
    hooks_dir().ok().map(|dir| dir.display().to_string())
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
    let all: Vec<&Hook> = HOOKS.iter().collect();
    install_these(&all, force)
}

fn install_these(hooks: &[&Hook], force: bool) -> Result<Installed, String> {
    let dir = hooks_dir()?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;

    let mut done = Installed {
        paths: Vec::new(),
        chained: Vec::new(),
    };
    for hook in hooks {
        let path = dir.join(hook.name);
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let ours = existing.contains(MARKER);
        if !ours && !force {
            if let Some((tool, advice)) = manager_of(&dir, &existing) {
                return Err(format!(
                    "{}\n\
                     is managed by {tool}, which rewrites it on its next install\n\
                     instead: {advice}\n\
                     or use `commitsmith hooks install --force`",
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

        std::fs::write(&path, hook.script()).map_err(|error| error.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::{HOOKS, MARKER, PRELUDE};

    #[test]
    fn the_binary_search_exists_once_in_the_source() {
        // The search was copied into four scripts and had to be corrected in four
        // places; the prelude is the single home it has now. The file's own test
        // section names the same strings, so it is cut off before counting —
        // otherwise the check counts its own literals.
        assert!(PRELUDE.contains("command -v commitsmith"));
        let source = include_str!("hooks.rs");
        let test_marker = concat!("#[cfg", "(test)]");
        let production = &source[..source.find(test_marker).unwrap_or(source.len())];
        assert_eq!(
            production.matches("command -v commitsmith").count(),
            1,
            "the search must not be duplicated in the source"
        );
        assert_eq!(
            production
                .matches("git config --get commitsmith.binary")
                .count(),
            1
        );
    }

    #[test]
    fn every_script_is_assembled_from_the_shared_prelude() {
        for hook in &HOOKS {
            let script = hook.script();
            assert!(script.starts_with("#!/bin/sh\n"), "{}", hook.name);
            assert!(
                script.contains(&format!("{MARKER} {} hook", hook.name)),
                "{} carries the marker",
                hook.name
            );
            assert!(
                script.contains(PRELUDE),
                "{} uses the shared search",
                hook.name
            );
            // The chain knows its own name, so a foreign hook is found next to it.
            assert!(
                script.contains(&format!("previous_hook='{}'", hook.name)),
                "{} names its predecessor",
                hook.name
            );
            // Every script decides for itself what a missing binary means.
            assert!(script.contains("usable \"$binary\""), "{}", hook.name);
        }
    }

    #[test]
    fn the_scripts_differ_only_in_their_body() {
        let bodies: Vec<&str> = HOOKS.iter().map(|hook| hook.body).collect();
        for (index, body) in bodies.iter().enumerate() {
            for (other, compare) in bodies.iter().enumerate() {
                if index != other {
                    assert_ne!(body, compare, "each hook has its own job");
                }
            }
        }
        // And the shared part is really shared, not repeated.
        for hook in &HOOKS {
            assert!(
                !hook.body.contains("command -v commitsmith"),
                "{}",
                hook.name
            );
        }
    }
}
