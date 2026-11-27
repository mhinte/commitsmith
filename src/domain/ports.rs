//! Ports: what the application needs from the outside world.
//!
//! The traits speak in domain types only. `gix`, `std::process::Command` and
//! TOML files stay behind the implementations in `infrastructure`, which keeps
//! the use cases testable against an in-memory fake.

use std::path::{Path, PathBuf};

use crate::domain::errors::{ConfigError, GitError};
use crate::domain::history::{PastCommit, ScopeInfo};

/// A branch as the explorer and the context bridge need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    /// Short name, e.g. `feat/mmu/TICKET-1/x` or `origin/main`.
    pub name: String,
    /// True for the branch `HEAD` points at.
    pub is_head: bool,
    /// True for `refs/remotes/...` entries.
    pub is_remote: bool,
    /// Configured upstream, when one is set.
    pub upstream: Option<String>,
}

impl BranchInfo {
    pub fn local(name: impl Into<String>, is_head: bool) -> Self {
        Self {
            name: name.into(),
            is_head,
            is_remote: false,
            upstream: None,
        }
    }

    pub fn remote(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_head: false,
            is_remote: true,
            upstream: None,
        }
    }
}

/// What happened to a file, in the vocabulary `git status` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileState {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

impl FileState {
    pub fn code(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Untracked => '?',
            Self::Conflicted => 'U',
        }
    }
}

/// A changed path, with its index and work-tree state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    /// For a rename or copy: the path it came from. `None` for every other
    /// state — a path by itself is not enough to stage a move, the index needs
    /// both sides of it.
    pub origin: Option<String>,
    /// State in the index (staged), if any.
    pub index: Option<FileState>,
    /// State in the work tree (unstaged), if any.
    pub worktree: Option<FileState>,
}

impl ChangedFile {
    pub fn new(
        path: impl Into<String>,
        index: Option<FileState>,
        worktree: Option<FileState>,
    ) -> Self {
        Self {
            path: path.into(),
            origin: None,
            index,
            worktree,
        }
    }

    /// Mark this entry as a move and hand back itself, for the parser.
    #[must_use]
    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }

    /// Both sides of a move, new path last; just the path when nothing moved.
    pub fn paths(&self) -> Vec<&str> {
        match &self.origin {
            Some(origin) => vec![origin.as_str(), self.path.as_str()],
            None => vec![self.path.as_str()],
        }
    }

    /// What a row says: `old → new` for a move, the path otherwise.
    pub fn label(&self) -> String {
        match &self.origin {
            Some(origin) => format!("{origin} → {}", self.path),
            None => self.path.clone(),
        }
    }

    /// True when nothing of this file is staged yet.
    pub fn is_unstaged(&self) -> bool {
        self.index.is_none()
    }

    /// True when the index holds a change for this file.
    pub fn is_staged(&self) -> bool {
        self.index.is_some()
    }
}

/// What tab 2 needs about the branches, from one read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BranchOverview {
    pub branches: Vec<BranchInfo>,
    /// `(name, ahead, behind)` per branch with an upstream that still exists.
    pub sync: Vec<(String, usize, usize)>,
}

/// A config file as text plus where it came from.
///
/// The store does not parse: reading and writing a document is infrastructure,
/// deciding what the keys mean belongs to the layer that owns the config model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDocument {
    pub path: PathBuf,
    pub text: String,
}

/// Reading and writing the repository.
///
/// Read methods go through `gix`; the mutating ones shell out to `git`, because
/// reimplementing commit/merge/push is how a tool breaks a repository.
pub trait GitRepository {
    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError>;
    fn branches(&self) -> Result<Vec<BranchInfo>, GitError>;
    /// Branches and their sync marks together. A reload needs both, and two
    /// `for-each-ref` processes per reload are two too many; an implementation
    /// that cannot answer in one read falls back to the two calls.
    fn branch_overview(&self) -> Result<BranchOverview, GitError> {
        Ok(BranchOverview {
            branches: self.branches()?,
            sync: self.branch_sync()?,
        })
    }
    fn changes(&self) -> Result<Vec<ChangedFile>, GitError>;
    fn recent_commits(&self, limit: usize) -> Result<Vec<PastCommit>, GitError>;
    fn tags(&self) -> Result<Vec<String>, GitError>;
    /// Scopes from history, merged with the configured ones.
    fn scopes(&self, configured: &[String], depth: usize) -> Result<ScopeInfo, GitError>;

    /// Files a commit touched, as `M src/app.rs` lines. Read for one commit at a
    /// time, when someone looks at it: doing this for the whole history is what
    /// made the start in a long repository slow.
    fn commit_files(&self, id: &str) -> Result<Vec<String>, GitError> {
        let _ = id;
        Ok(Vec::new())
    }

    /// Unified diff of one path: `staged` compares the index with `HEAD`,
    /// otherwise the working tree with the index.
    fn diff(&self, path: &str, staged: bool) -> Result<String, GitError>;

    fn stage(&self, path: &str) -> Result<(), GitError>;
    /// Stage a moved file. `path` alone is not enough: `git` then keeps the old
    /// path as an unstaged deletion, so both sides of the move go into the
    /// index. Implementations that do not model moves fall back to [`Self::stage`].
    fn stage_move(&self, origin: &str, path: &str) -> Result<(), GitError> {
        let _ = origin;
        self.stage(path)
    }
    fn unstage(&self, path: &str) -> Result<(), GitError>;
    /// Take a moved file back out of the index. Both sides are needed here for
    /// the same reason as in [`Self::stage_move`].
    fn unstage_move(&self, origin: &str, path: &str) -> Result<(), GitError> {
        let _ = origin;
        self.unstage(path)
    }
    /// Throw away the working-tree changes of one path. Destructive: the caller
    /// asks first.
    fn discard(&self, path: &str) -> Result<(), GitError>;
    /// Park every change in a stash entry and take it back.
    fn stash(&self, message: &str) -> Result<(), GitError>;
    fn stash_pop(&self) -> Result<(), GitError>;
    /// Commit what is staged; returns the short id git printed.
    fn commit(&self, message: &str) -> Result<String, GitError>;
    fn checkout(&self, branch: &str) -> Result<(), GitError>;
    fn create_branch(&self, name: &str) -> Result<(), GitError>;
    fn merge(&self, branch: &str) -> Result<(), GitError>;
    fn delete_branch(&self, branch: &str) -> Result<(), GitError>;
    fn pull(&self) -> Result<(), GitError>;
    fn push(&self) -> Result<(), GitError>;
    /// `(ahead, behind)` against the upstream. `None` means the branch has no
    /// upstream, which is a different state from being in sync.
    fn ahead_behind(&self) -> Result<Option<(usize, usize)>, GitError>;
    /// Commits since `since` (a tag), newest first. `None` means the whole window.
    fn commits_since(
        &self,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::domain::history::PastCommit>, GitError>;
    /// Commits on `HEAD` that are not on `base` yet: the branch's own work,
    /// found through the merge base rather than by guesswork.
    fn commits_not_on(
        &self,
        base: &str,
        limit: usize,
    ) -> Result<Vec<crate::domain::history::PastCommit>, GitError>;
    /// Ahead/behind per local branch, against its upstream. Branches without an
    /// upstream are not in the list — that is the caller's business to show.
    fn branch_sync(&self) -> Result<Vec<(String, usize, usize)>, GitError>;
    /// Updates the remote-tracking branches; nothing in the work tree changes.
    fn fetch(&self) -> Result<(), GitError>;
    /// Creates an annotated tag, which is what a release is.
    fn create_tag(&self, tag: &str, message: &str) -> Result<(), GitError>;
}

/// The user-wide and the project config file.
pub trait ConfigStore {
    /// `None` when the file does not exist yet.
    fn user_document(&self) -> Result<Option<ConfigDocument>, ConfigError>;
    /// Searches upwards from `start`, like git does for `.git`.
    fn project_document(&self, start: &Path) -> Result<Option<ConfigDocument>, ConfigError>;
    /// Writes the user file atomically and returns where it landed.
    fn write_user(&self, text: &str) -> Result<PathBuf, ConfigError>;
}

/// A use case that only needs the port, to prove the trait is enough for the
/// application layer: which scopes the picker offers.
pub fn scope_choices(
    repository: &dyn GitRepository,
    configured: &[String],
    depth: usize,
) -> Result<Vec<String>, GitError> {
    Ok(repository.scopes(configured, depth)?.all())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fake the application tests run against; it records what was asked.
    #[derive(Default)]
    pub struct FakeRepo {
        pub branches: Vec<BranchInfo>,
        pub staged: Vec<String>,
        pub commits: Vec<PastCommit>,
        pub scopes: Vec<String>,
        pub calls: std::cell::RefCell<Vec<String>>,
    }

    impl GitRepository for FakeRepo {
        fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
            Ok(self.branches.iter().find(|b| b.is_head).cloned())
        }

        fn branches(&self) -> Result<Vec<BranchInfo>, GitError> {
            Ok(self.branches.clone())
        }

        fn changes(&self) -> Result<Vec<ChangedFile>, GitError> {
            Ok(self
                .staged
                .iter()
                .map(|path| ChangedFile::new(path.clone(), Some(FileState::Modified), None))
                .collect())
        }

        fn recent_commits(&self, limit: usize) -> Result<Vec<PastCommit>, GitError> {
            Ok(self.commits.iter().take(limit).cloned().collect())
        }

        fn tags(&self) -> Result<Vec<String>, GitError> {
            Ok(Vec::new())
        }

        fn scopes(&self, configured: &[String], _depth: usize) -> Result<ScopeInfo, GitError> {
            Ok(ScopeInfo {
                history: self.scopes.clone(),
                configured: configured.to_vec(),
            })
        }

        fn diff(&self, path: &str, staged: bool) -> Result<String, GitError> {
            self.calls
                .borrow_mut()
                .push(format!("diff {path} staged={staged}"));
            Ok(format!("diff --git a/{path} b/{path}\n+added\n-removed\n"))
        }

        fn stage(&self, path: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("stage {path}"));
            Ok(())
        }

        fn discard(&self, path: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("discard {path}"));
            Ok(())
        }

        fn stash(&self, message: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("stash {message}"));
            Ok(())
        }

        fn stash_pop(&self) -> Result<(), GitError> {
            self.calls.borrow_mut().push("stash pop".to_string());
            Ok(())
        }

        fn unstage(&self, path: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("unstage {path}"));
            Ok(())
        }

        fn commit(&self, message: &str) -> Result<String, GitError> {
            self.calls.borrow_mut().push(format!("commit {message}"));
            Ok("abc1234".to_string())
        }

        fn checkout(&self, branch: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("checkout {branch}"));
            Ok(())
        }

        fn create_branch(&self, name: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("branch {name}"));
            Ok(())
        }

        fn merge(&self, branch: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("merge {branch}"));
            Ok(())
        }

        fn delete_branch(&self, branch: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("delete {branch}"));
            Ok(())
        }

        fn pull(&self) -> Result<(), GitError> {
            self.calls.borrow_mut().push("pull".to_string());
            Ok(())
        }

        fn push(&self) -> Result<(), GitError> {
            self.calls.borrow_mut().push("push".to_string());
            Ok(())
        }

        fn ahead_behind(&self) -> Result<Option<(usize, usize)>, GitError> {
            Ok(Some((0, 0)))
        }

        fn branch_sync(&self) -> Result<Vec<(String, usize, usize)>, GitError> {
            Ok(Vec::new())
        }

        fn commits_since(
            &self,
            _since: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<crate::domain::history::PastCommit>, GitError> {
            Ok(Vec::new())
        }

        fn commits_not_on(
            &self,
            base: &str,
            _limit: usize,
        ) -> Result<Vec<crate::domain::history::PastCommit>, GitError> {
            self.calls
                .borrow_mut()
                .push(format!("commits_not_on {base}"));
            Ok(Vec::new())
        }

        fn fetch(&self) -> Result<(), GitError> {
            self.calls.borrow_mut().push("fetch".to_string());
            Ok(())
        }

        fn create_tag(&self, tag: &str, _message: &str) -> Result<(), GitError> {
            self.calls.borrow_mut().push(format!("tag {tag}"));
            Ok(())
        }
    }

    #[test]
    fn the_use_case_only_needs_the_port() {
        let repo = FakeRepo {
            scopes: vec!["api".into(), "ui".into()],
            ..Default::default()
        };
        let choices = scope_choices(&repo, &["ui".into(), "parser".into()], 50).unwrap();
        assert_eq!(choices, vec!["ui", "parser", "api"]);
    }

    #[test]
    fn the_fake_records_what_it_was_asked_to_do() {
        let repo = FakeRepo::default();
        repo.stage("src/lib.rs").unwrap();
        repo.commit("feat: x").unwrap();
        let calls = repo.calls.borrow();
        assert_eq!(*calls, vec!["stage src/lib.rs", "commit feat: x"]);
    }

    #[test]
    fn changed_files_know_whether_they_are_staged() {
        let staged = ChangedFile::new("a", Some(FileState::Added), None);
        let unstaged = ChangedFile::new("b", None, Some(FileState::Modified));
        assert!(staged.is_staged() && !staged.is_unstaged());
        assert!(unstaged.is_unstaged() && !unstaged.is_staged());
        assert_eq!(FileState::Untracked.code(), '?');
    }

    #[test]
    fn a_move_carries_both_of_its_paths() {
        let moved = ChangedFile::new("src/new.rs", None, Some(FileState::Renamed))
            .with_origin("src/old.rs");
        let plain = ChangedFile::new("src/lib.rs", Some(FileState::Added), None);
        assert_eq!(moved.paths(), ["src/old.rs", "src/new.rs"]);
        assert_eq!(moved.label(), "src/old.rs → src/new.rs");
        assert_eq!(plain.paths(), ["src/lib.rs"]);
        assert_eq!(plain.label(), "src/lib.rs");
    }

    #[test]
    fn branches_carry_their_origin() {
        let local = BranchInfo::local("main", true);
        let remote = BranchInfo::remote("origin/main");
        assert!(local.is_head && !local.is_remote);
        assert!(remote.is_remote && !remote.is_head);
    }
}
