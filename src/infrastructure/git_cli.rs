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
            let state = |code: char| match code {
                ' ' => None,
                other => Some(file_state(other)),
            };
            changes.push(ChangedFile {
                path,
                index: state(index),
                worktree: state(worktree),
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
}

/// Point a git alias at this binary: `git smith` then opens the composer.
///
/// `!` makes git run it through a shell, which is what an alias that is not a git
/// subcommand needs.
pub fn install_alias(name: &str, binary: &str) -> Result<(), GitError> {
    let key = format!("alias.{name}");
    let value = format!("!{binary}");
    let output = Command::new("git")
        .args(["config", "--global", &key, &value])
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
    fn a_failed_command_becomes_a_error_not_a_panic() {
        let (dir, repo) = temp_repo("failing");
        let error = repo.checkout("does-not-exist").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("does-not-exist"), "{message}");
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
