//! One place that starts `git`.
//!
//! Every adapter used to build its own `Command`: 35 calls across five files, each
//! with its own `current_dir`, its own argument handling and its own error mapping.
//! A change that has to reach all of them — a timeout, `core.quotepath=false`,
//! `GIT_TERMINAL_PROMPT=0` — had to be made 35 times, and was made in the wrong
//! number of places more than once.
//!
//! The functions here are deliberately thin: they start the process, hand back what
//! it said, and map the failure onto the [`GitError`] the surfaces already render.
//! Nothing in this module knows what a commit or a branch is.

use std::path::Path;
use std::process::{Command, Output};

use crate::domain::errors::GitError;

/// Run `git` in `dir` with `args`, giving the caller the raw [`Output`].
///
/// `dir` is `None` for the cases that run in the current directory (a hook, a
/// diagnostic) — a `Path::new(".")` would be the same thing, but saying "here" is
/// clearer at the call site than a fake path.
pub fn run(dir: Option<&Path>, args: &[&str], env: &[(&str, &str)]) -> Result<Output, GitError> {
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.args(args);
    // A git that waits for a password would hang the TUI. Everything this program
    // does is local or already authenticated by the user's own setup.
    command.env("GIT_TERMINAL_PROMPT", "0");
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .output()
        .map_err(|error| GitError::io(format!("git {}", args.first().unwrap_or(&"")), error))
}

/// Run `git` and hand back stdout, or a [`GitError`] carrying what git said.
///
/// This is the shape the port uses: success means "stdout is the answer", failure
/// means "the caller renders an error".
pub fn git(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = run(Some(dir), args, &[])?;
    if output.status.success() {
        // One copy: `from_utf8_lossy(..).to_string()` allocates twice, and a diff of
        // a large file goes through here.
        return Ok(match String::from_utf8(output.stdout) {
            Ok(text) => text,
            Err(error) => String::from_utf8_lossy(error.as_bytes()).to_string(),
        });
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(crate::infrastructure::git_cli::classify(args, &stderr, dir))
}

/// Run `git` here and hand back its trimmed stdout, or `None` when it fails.
///
/// For the read-only questions a CLI command asks itself (which branch is checked
/// out), where a failure is not an error worth rendering: the caller simply has
/// nothing to say.
pub fn here(args: &[&str]) -> Option<String> {
    let output = run(None, args, &[]).ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run `git` and answer whether it succeeded. For a question whose only useful
/// answer is yes or no (`git rev-parse --verify HEAD`).
pub fn succeeds(dir: &Path, args: &[&str]) -> bool {
    run(Some(dir), args, &[])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{git, here, run, succeeds};

    fn repo() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-process-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "p@example.com"],
            vec!["config", "user.name", "P"],
        ] {
            let _ = run(Some(&dir), &args, &[]).unwrap();
        }
        dir
    }

    #[test]
    fn a_successful_run_hands_back_stdout() {
        let dir = repo();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        git(&dir, &["add", "a.txt"]).unwrap();
        git(&dir, &["commit", "-q", "-m", "feat: add a file"]).unwrap();
        let branch = git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
        assert!(
            matches!(branch.trim(), "main" | "master"),
            "the branch name comes back: {branch:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failing_run_becomes_an_error_not_a_panic() {
        let dir = repo();
        let error = git(&dir, &["rev-parse", "--verify", "nope"]).unwrap_err();
        // Whatever git said, it is an error the caller can render.
        assert!(!error.to_string().is_empty());
        assert!(!succeeds(&dir, &["rev-parse", "--verify", "HEAD"]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn environment_variables_reach_the_process() {
        let dir = repo();
        let output = run(
            Some(&dir),
            &["rev-parse", "--git-path", "hooks"],
            &[("GIT_TERMINAL_PROMPT", "0")],
        )
        .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("hooks"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn here_answers_none_outside_a_repository() {
        // The working directory of the test binary is the crate root, which is not a
        // repository; the call must simply say nothing.
        let outside = std::env::temp_dir().join(format!("commitsmith-here-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&outside).unwrap();
        let answer = here(&["symbolic-ref", "--short", "HEAD"]);
        std::env::set_current_dir(previous).unwrap();
        assert!(answer.is_none());
        let _ = std::fs::remove_dir_all(outside);
    }
}
