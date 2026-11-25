//! A repository double for tests: records what the surface asked it to do.

use std::cell::RefCell;
use std::rc::Rc;

use commitsmith::domain::errors::GitError;
use commitsmith::domain::history::{PastCommit, ScopeInfo};
use std::path::{Path, PathBuf};

use commitsmith::domain::errors::ConfigError;
use commitsmith::domain::ports::{
    BranchInfo, ChangedFile, ConfigDocument, ConfigStore, FileState, GitRepository,
};

/// A repository that records what the surface asked it to do.
pub struct RecordingGit {
    pub branches: RefCell<Vec<BranchInfo>>,
    calls: Rc<RefCell<Vec<String>>>,
    pub merge_fails: bool,
    pub create_fails: bool,
    /// Files the fake reports, with their real index/worktree state.
    pub files: RefCell<Vec<ChangedFile>>,
}

impl RecordingGit {
    pub fn new(names: &[&str], head: &str) -> Self {
        Self {
            branches: RefCell::new(
                names
                    .iter()
                    .map(|name| BranchInfo::local(*name, *name == head))
                    .collect(),
            ),
            calls: Rc::new(RefCell::new(Vec::new())),
            merge_fails: false,
            create_fails: false,
            files: RefCell::new(Vec::new()),
        }
    }

    /// A handle on the call log that outlives `set_git`, which boxes the fake.
    pub fn calls(&self) -> Rc<RefCell<Vec<String>>> {
        Rc::clone(&self.calls)
    }

    pub fn set_head(&self, name: &str) {
        for branch in self.branches.borrow_mut().iter_mut() {
            branch.is_head = branch.name == name;
        }
    }
}

impl GitRepository for RecordingGit {
    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
        Ok(self.branches.borrow().iter().find(|b| b.is_head).cloned())
    }

    fn branches(&self) -> Result<Vec<BranchInfo>, GitError> {
        Ok(self.branches.borrow().clone())
    }

    fn diff(&self, path: &str, staged: bool) -> Result<String, GitError> {
        self.calls
            .borrow_mut()
            .push(format!("diff {path} staged={staged}"));
        Ok(format!("diff --git a/{path} b/{path}\n+added\n-removed\n"))
    }

    fn changes(&self) -> Result<Vec<ChangedFile>, GitError> {
        Ok(self.files.borrow().clone())
    }

    fn recent_commits(&self, _limit: usize) -> Result<Vec<PastCommit>, GitError> {
        Ok(Vec::new())
    }

    fn tags(&self) -> Result<Vec<String>, GitError> {
        Ok(Vec::new())
    }

    fn scopes(&self, _configured: &[String], _depth: usize) -> Result<ScopeInfo, GitError> {
        Ok(ScopeInfo::default())
    }

    fn stage(&self, path: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("stage {path}"));
        let mut files = self.files.borrow_mut();
        match files.iter_mut().find(|file| file.path == path) {
            Some(file) => {
                file.index = Some(FileState::Modified);
                file.worktree = None;
            }
            None => files.push(ChangedFile {
                path: path.to_string(),
                index: Some(FileState::Added),
                worktree: None,
            }),
        }
        Ok(())
    }

    fn unstage(&self, path: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("unstage {path}"));
        let mut files = self.files.borrow_mut();
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.index = None;
            file.worktree = Some(FileState::Modified);
        }
        Ok(())
    }

    fn commit(&self, message: &str) -> Result<String, GitError> {
        self.calls.borrow_mut().push(format!("commit {message}"));
        Ok("abc1234".to_string())
    }

    fn checkout(&self, branch: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("checkout {branch}"));
        self.set_head(branch);
        Ok(())
    }

    fn create_branch(&self, name: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("create {name}"));
        if self.create_fails {
            return Err(GitError::invalid_state(format!(
                "a branch called `{name}` already exists"
            )));
        }
        self.branches
            .borrow_mut()
            .push(BranchInfo::local(name, false));
        Ok(())
    }

    fn merge(&self, branch: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("merge {branch}"));
        if self.merge_fails {
            return Err(GitError::conflict(vec!["src/ui.rs".to_string()]));
        }
        Ok(())
    }

    fn delete_branch(&self, branch: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("delete {branch}"));
        let mut branches = self.branches.borrow_mut();
        if branches.iter().any(|b| b.name == branch && b.is_head) {
            return Err(GitError::invalid_state(
                "cannot delete the branch that is checked out".to_string(),
            ));
        }
        branches.retain(|b| b.name != branch);
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
}

/// A config store that keeps what was written, so tests can read the file text
/// without touching the disk.
#[derive(Default)]
pub struct RecordingConfig {
    pub written: Rc<RefCell<String>>,
}

impl ConfigStore for RecordingConfig {
    fn user_document(&self) -> Result<Option<ConfigDocument>, ConfigError> {
        Ok(None)
    }

    fn project_document(&self, _start: &Path) -> Result<Option<ConfigDocument>, ConfigError> {
        Ok(None)
    }

    fn write_user(&self, text: &str) -> Result<PathBuf, ConfigError> {
        *self.written.borrow_mut() = text.to_string();
        Ok(PathBuf::from("/tmp/commitsmith-test-config.toml"))
    }
}
