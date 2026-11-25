//! The `git` command line, behind the ports.
//!
//! Writes go through `git` itself: a tool that reimplements `commit`, `merge` or
//! `push` eventually corrupts a repository. Reads reuse [`GixRepo`], so listing
//! does not spawn a process.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::errors::GitError;
use crate::domain::history::{PastCommit, ScopeInfo};
use crate::domain::ports::{BranchInfo, ChangedFile, GitRepository};
use crate::infrastructure::gix_repo::{GixRepo, file_state};

/// A repository that reads with `gix` and writes with `git`.
pub struct GitBackend {
    reader: GixRepo,
    workdir: PathBuf,
}

impl GitBackend {
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let reader = GixRepo::open(path)?;
        let workdir = reader.workdir().to_path_buf();
        Ok(Self { reader, workdir })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Run `git` and hand back stdout, or a [`GitError`] carrying stderr.
    fn git(&self, args: &[&str]) -> Result<String, GitError> {
        let output = Command::new("git")
            .current_dir(&self.workdir)
            .args(args)
            .output()
            .map_err(|error| GitError::io(format!("git {}", args.first().unwrap_or(&"")), error))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(classify(args, &stderr, &self.workdir))
    }
}

/// Map git's answer onto the error the surface should show.
/// `%(upstream:track)` says `[ahead 2, behind 1]`, `[ahead 3]`, `[behind 4]` or
/// nothing at all. The numbers are all that matter here.
fn parse_track(track: &str) -> (usize, usize) {
    let body = track.trim_matches(|c| c == '[' || c == ']');
    let mut ahead = 0;
    let mut behind = 0;
    for part in body.split(',') {
        let mut words = part.split_whitespace();
        match (words.next(), words.next()) {
            (Some("ahead"), Some(count)) => ahead = count.parse().unwrap_or(0),
            (Some("behind"), Some(count)) => behind = count.parse().unwrap_or(0),
            _ => {}
        }
    }
    (ahead, behind)
}

fn classify(args: &[&str], stderr: &str, workdir: &Path) -> GitError {
    let operation = args.join(" ");
    let lower = stderr.to_lowercase();
    if lower.contains("not fully merged") || lower.contains("unmerged") {
        return GitError::conflict(Vec::<String>::new());
    }
    if lower.contains("no upstream") || lower.contains("no configured push destination") {
        return GitError::invalid_state(format!("no upstream for this branch: {stderr}"));
    }
    if lower.contains("would be overwritten") || lower.contains("local changes") {
        return GitError::invalid_state(format!("the working tree is in the way: {stderr}"));
    }
    if lower.contains("not a git repository") {
        return GitError::not_found(workdir.display().to_string());
    }
    GitError::remote(operation, stderr.to_string())
}

impl GitRepository for GitBackend {
    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
        self.reader.current_branch()
    }

    fn branches(&self) -> Result<Vec<BranchInfo>, GitError> {
        self.reader.branches()
    }

    fn changes(&self) -> Result<Vec<ChangedFile>, GitError> {
        // `git status` is the authority for untracked, ignored and conflict
        // states; reimplementing that with `gix` would only add a second truth.
        let output = self.git(&["status", "--porcelain=v1", "--untracked-files=all"])?;
        let mut changes = Vec::new();
        for line in output.lines() {
            if line.len() < 4 {
                continue;
            }
            let index = line.as_bytes()[0] as char;
            let worktree = line.as_bytes()[1] as char;
            let path = line[3..].trim().to_string();
            // `??` is untracked: nothing in the index, and the file is new in
            // the worktree. Treating it as staged made `Space` unstage it, which
            // failed with "pathspec did not match".
            let state = |code: char| match code {
                ' ' | '?' => None,
                other => Some(file_state(other)),
            };
            let untracked = index == '?' && worktree == '?';
            changes.push(ChangedFile {
                path,
                index: state(index),
                worktree: if untracked {
                    Some(crate::domain::ports::FileState::Untracked)
                } else {
                    state(worktree)
                },
            });
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    fn recent_commits(&self, limit: usize) -> Result<Vec<PastCommit>, GitError> {
        self.reader.recent_commits(limit)
    }

    fn tags(&self) -> Result<Vec<String>, GitError> {
        self.reader.tags()
    }

    fn scopes(&self, configured: &[String], depth: usize) -> Result<ScopeInfo, GitError> {
        self.reader.scopes(configured, depth)
    }

    fn commits_since(
        &self,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::domain::history::PastCommit>, GitError> {
        self.reader.commits_since(since, limit)
    }

    fn diff(&self, path: &str, staged: bool) -> Result<String, GitError> {
        // No pager, no colour: the pane decides how to paint the hunks.
        let args: Vec<&str> = if staged {
            vec![
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "--",
                path,
            ]
        } else {
            vec!["diff", "--no-color", "--no-ext-diff", "--", path]
        };
        self.git(&args)
    }

    fn stage(&self, path: &str) -> Result<(), GitError> {
        self.git(&["add", "--", path]).map(|_| ())
    }

    fn unstage(&self, path: &str) -> Result<(), GitError> {
        // `restore --staged` works in a repository without HEAD as well.
        self.git(&["restore", "--staged", "--", path]).map(|_| ())
    }

    fn commit(&self, message: &str) -> Result<String, GitError> {
        let output = self.git(&["commit", "-m", message])?;
        short_id(&output).ok_or_else(|| GitError::invalid_state("git printed no commit id"))
    }

    fn checkout(&self, branch: &str) -> Result<(), GitError> {
        self.git(&["checkout", branch]).map(|_| ())
    }

    fn create_branch(&self, name: &str) -> Result<(), GitError> {
        self.git(&["checkout", "-b", name]).map(|_| ())
    }

    fn merge(&self, branch: &str) -> Result<(), GitError> {
        self.git(&["merge", "--no-edit", branch]).map(|_| ())
    }

    fn delete_branch(&self, branch: &str) -> Result<(), GitError> {
        self.git(&["branch", "-d", branch]).map(|_| ())
    }

    fn pull(&self) -> Result<(), GitError> {
        self.git(&["pull", "--ff-only"]).map(|_| ())
    }

    fn push(&self) -> Result<(), GitError> {
        self.git(&["push"]).map(|_| ())
    }

    fn branch_sync(&self) -> Result<Vec<(String, usize, usize)>, GitError> {
        let output = self.git(&[
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:track)",
            "refs/heads",
        ])?;
        Ok(output
            .lines()
            .filter_map(|line| {
                let (name, track) = line.split_once('\0')?;
                let track = track.trim();
                if track.is_empty() || track == "[gone]" {
                    return None;
                }
                let (ahead, behind) = parse_track(track);
                Some((name.to_string(), ahead, behind))
            })
            .collect())
    }

    fn fetch(&self) -> Result<(), GitError> {
        self.git(&["fetch", "--prune", "--quiet"]).map(|_| ())
    }

    /// `git rev-list --left-right --count HEAD...@{upstream}` answers both numbers
    /// in one call: left is ahead, right is behind. A missing upstream is not an
    /// error — it is a state the caller has to explain to the user.
    fn ahead_behind(&self) -> Result<Option<(usize, usize)>, GitError> {
        let has_upstream = self
            .git(&[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ])
            .is_ok();
        if !has_upstream {
            return Ok(None);
        }
        let output = self.git(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])?;
        let mut parts = output.split_whitespace();
        let ahead: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let behind: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        Ok(Some((ahead, behind)))
    }
}

/// Point a git alias at this binary: `git smith` then opens the composer.
///
/// `!` makes git run the command through a shell. That is why the path needs
/// forward slashes and quotes: a shell reads `C:\Users\marc` as `C:Usersmarc`,
/// which is exactly the broken alias a Windows user reported.
pub fn install_alias(name: &str, binary: &str) -> Result<(), GitError> {
    install_alias_in(name, binary, None)
}

/// Same, but with an explicit global config file (used by tests).
pub fn install_alias_in(
    name: &str,
    binary: &str,
    global_config: Option<&Path>,
) -> Result<(), GitError> {
    let key = format!("alias.{name}");
    let value = shell_command(binary);
    let mut command = Command::new("git");
    command.args(["config", "--global", &key, &value]);
    if let Some(path) = global_config {
        command.env("GIT_CONFIG_GLOBAL", path);
    }
    let output = command
        .output()
        .map_err(|error| GitError::io("git config --global", error))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::remote(
            "git config",
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

/// Quote a binary path for git's `!` aliases.
///
/// Windows paths become `!"C:/Program Files/commitsmith.exe"`: forward slashes
/// because the shell strips backslashes, quotes because the path may contain
/// spaces.
fn shell_command(binary: &str) -> String {
    let path = binary.replace('\\', "/");
    if path.contains(' ') || path.contains('"') {
        format!("!\"{}\"", path.replace('"', "\\\""))
    } else {
        format!("!{path}")
    }
}

/// The `[main abc1234] subject` line, reduced to the id.
///
/// A root commit prints `[main (root-commit) abc1234]`, so the id is the last
/// token inside the brackets, not the second one.
fn short_id(output: &str) -> Option<String> {
    let line = output.lines().next()?;
    let inner = line.split('[').nth(1)?.split(']').next()?;
    let id = inner.split_whitespace().last()?;
    Some(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_repo(name: &str) -> (PathBuf, GitBackend) {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-cli-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            let status = Command::new("git")
                .current_dir(&dir)
                .args(&args)
                .status()
                .unwrap();
            assert!(status.success(), "{args:?}");
        }
        fs::write(dir.join("a.txt"), "hello\n").unwrap();
        let backend = GitBackend::open(&dir).unwrap();
        (dir, backend)
    }

    #[test]
    fn an_untracked_file_is_not_staged() {
        let (dir, repo) = temp_repo("untracked");
        std::fs::write(dir.join("new.txt"), "x").unwrap();
        let changes = repo.changes().unwrap();
        let file = changes
            .iter()
            .find(|file| file.path == "new.txt")
            .expect("new.txt");
        assert!(!file.is_staged(), "?? must not count as staged");
        assert_eq!(
            file.worktree,
            Some(crate::domain::ports::FileState::Untracked)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stage_commit_and_read_back() {
        let (dir, repo) = temp_repo("stage");
        repo.stage("a.txt").unwrap();
        assert!(
            repo.changes()
                .unwrap()
                .iter()
                .any(|c| c.is_staged() && c.path == "a.txt")
        );
        let id = repo.commit("feat: add a file").unwrap();
        assert_eq!(id.len(), 7, "short id: {id}");
        let commits = repo.recent_commits(5).unwrap();
        assert_eq!(commits[0].summary, "add a file");
        assert_eq!(repo.current_branch().unwrap().unwrap().name, "main");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn branches_can_be_created_and_deleted() {
        let (dir, repo) = temp_repo("branches");
        repo.stage("a.txt").unwrap();
        repo.commit("feat: base").unwrap();
        repo.create_branch("feat/mmu/T-1/x").unwrap();
        assert_eq!(
            repo.current_branch().unwrap().unwrap().name,
            "feat/mmu/T-1/x"
        );
        repo.checkout("main").unwrap();
        assert!(repo.delete_branch("feat/mmu/T-1/x").is_ok());
        let names: Vec<String> = repo
            .branches()
            .unwrap()
            .into_iter()
            .map(|b| b.name)
            .collect();
        assert_eq!(names, vec!["main"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn push_and_pull_against_a_local_remote() {
        let (dir, repo) = temp_repo("remote");
        let remote = dir.join("remote.git");
        let status = Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&remote)
            .status()
            .unwrap();
        assert!(status.success());
        repo.stage("a.txt").unwrap();
        repo.commit("feat: first").unwrap();
        let remote_str = remote.to_str().unwrap().to_string();
        repo.git(&["remote", "add", "origin", &remote_str]).unwrap();

        // Without an upstream git refuses; that has to be a readable error.
        let error = repo.push().unwrap_err().to_string();
        assert!(error.contains("push"), "{error}");

        repo.git(&["push", "-q", "-u", "origin", "main"]).unwrap();
        assert!(repo.pull().is_ok(), "pull after setting the upstream");

        // A second clone sees the commit the push created.
        let clone_dir = dir.join("clone");
        // `-b main` on purpose: a bare repository's HEAD points at whatever
        // `init.defaultBranch` was, and on a CI runner that is still `master`,
        // so a plain clone would check out an unborn branch and see no log.
        let status = Command::new("git")
            .args(["clone", "-q", "-b", "main"])
            .arg(&remote)
            .arg(&clone_dir)
            .status()
            .unwrap();
        assert!(status.success());
        let log = Command::new("git")
            .args(["log", "--format=%s"])
            .current_dir(&clone_dir)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "feat: first");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_track_string_becomes_two_numbers() {
        assert_eq!(parse_track("[ahead 2, behind 1]"), (2, 1));
        assert_eq!(parse_track("[ahead 3]"), (3, 0));
        assert_eq!(parse_track("[behind 4]"), (0, 4));
        assert_eq!(parse_track(""), (0, 0));
        assert_eq!(parse_track("[gone]"), (0, 0));
    }

    #[test]
    fn ahead_behind_counts_what_is_missing_on_either_side() {
        let (dir, repo) = temp_repo("ahead-behind");
        let remote = dir.join("remote.git");
        assert!(
            Command::new("git")
                .args(["init", "-q", "--bare"])
                .arg(&remote)
                .status()
                .unwrap()
                .success()
        );
        repo.stage("a.txt").unwrap();
        repo.commit("feat: first").unwrap();
        let remote_str = remote.to_str().unwrap().to_string();
        repo.git(&["remote", "add", "origin", &remote_str]).unwrap();

        // No upstream yet: a state, not an error.
        assert_eq!(repo.ahead_behind().unwrap(), None);

        repo.git(&["push", "-q", "-u", "origin", "main"]).unwrap();
        assert_eq!(repo.ahead_behind().unwrap(), Some((0, 0)));

        // One local commit: ahead by one.
        fs::write(dir.join("a.txt"), "second\n").unwrap();
        repo.stage("a.txt").unwrap();
        repo.commit("feat: second").unwrap();
        assert_eq!(repo.ahead_behind().unwrap(), Some((1, 0)));

        // A commit from elsewhere, fetched: behind by one.
        let clone = dir.join("clone");
        assert!(
            Command::new("git")
                .args(["clone", "-q", "-b", "main"])
                .arg(&remote)
                .arg(&clone)
                .status()
                .unwrap()
                .success()
        );
        for args in [
            vec!["config", "user.email", "other@example.com"],
            vec!["config", "user.name", "Other"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(&clone)
                .status()
                .unwrap();
        }
        fs::write(clone.join("b.txt"), "remote\n").unwrap();
        Command::new("git")
            .args(["add", "b.txt"])
            .current_dir(&clone)
            .status()
            .unwrap();
        Command::new("git")
            .args(["commit", "-q", "-m", "feat: remote"])
            .current_dir(&clone)
            .status()
            .unwrap();
        Command::new("git")
            .args(["push", "-q", "origin", "main"])
            .current_dir(&clone)
            .status()
            .unwrap();
        repo.git(&["fetch", "-q", "origin"]).unwrap();

        let (ahead, behind) = repo.ahead_behind().unwrap().unwrap();
        assert_eq!((ahead, behind), (1, 1), "diverged: one each way");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_command_becomes_a_error_not_a_panic() {
        let (dir, repo) = temp_repo("failing");
        let error = repo.checkout("does-not-exist").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("does-not-exist"), "{message}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn windows_paths_are_quoted_with_forward_slashes() {
        // git runs `!` aliases through a shell, which eats the backslashes and
        // ends up with `C:Usersmarc...` — the bug from issue #1.
        assert_eq!(
            shell_command(r"C:\Users\marc.hinte\commitsmith.exe"),
            "!C:/Users/marc.hinte/commitsmith.exe"
        );
        assert_eq!(
            shell_command(r"C:\Program Files\commitsmith.exe"),
            "!\"C:/Program Files/commitsmith.exe\""
        );
        assert_eq!(
            shell_command("/usr/local/bin/commitsmith"),
            "!/usr/local/bin/commitsmith"
        );
    }

    #[test]
    fn the_alias_is_stored_in_the_form_a_shell_can_run() {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-alias-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("gitconfig");
        fs::write(&config, "").unwrap();

        install_alias_in("smith", r"C:\Users\marc\commitsmith.exe", Some(&config)).unwrap();
        let stored = fs::read_to_string(&config).unwrap();
        assert!(stored.contains("[alias]"), "{stored}");
        assert!(
            stored.contains("smith = !C:/Users/marc/commitsmith.exe"),
            "{stored}"
        );
        assert!(!stored.contains(r"\Users"), "backslashes survive: {stored}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_short_id_parser_survives_other_output() {
        assert_eq!(
            short_id("[main 1a2b3c4] feat: x\n 1 file changed").as_deref(),
            Some("1a2b3c4")
        );
        assert_eq!(
            short_id("[main (root-commit) abc1234] feat: first").as_deref(),
            Some("abc1234")
        );
        assert_eq!(short_id("nothing here"), None);
    }
}
