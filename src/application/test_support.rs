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
    /// What `ahead_behind` answers; `None` means "no upstream".
    pub sync: Option<(usize, usize)>,
    /// When set, pull and push fail with this message.
    pub sync_error: Option<String>,
    /// Per-branch ahead/behind, as `branch_sync` reports it.
    pub sync_map: std::collections::HashMap<String, (usize, usize)>,
    /// When set, `create_tag` fails with this message.
    pub tag_error: Option<String>,
    /// What `commits_since` hands out.
    pub commits: RefCell<Vec<PastCommit>>,
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
            sync: Some((0, 0)),
            sync_error: None,
            tag_error: None,
            sync_map: std::collections::HashMap::new(),
            commits: RefCell::new(Vec::new()),
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
            None => files.push(ChangedFile::new(path, Some(FileState::Added), None)),
        }
        Ok(())
    }

    fn unstage_move(&self, origin: &str, path: &str) -> Result<(), GitError> {
        self.calls
            .borrow_mut()
            .push(format!("unstage_move {origin} {path}"));
        let mut files = self.files.borrow_mut();
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.index = None;
            file.worktree = Some(FileState::Renamed);
        }
        Ok(())
    }

    fn stage_move(&self, origin: &str, path: &str) -> Result<(), GitError> {
        self.calls
            .borrow_mut()
            .push(format!("stage_move {origin} {path}"));
        let mut files = self.files.borrow_mut();
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.index = Some(FileState::Renamed);
            file.worktree = None;
        }
        Ok(())
    }

    fn discard(&self, path: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("discard {path}"));
        let mut files = self.files.borrow_mut();
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.worktree = None;
            file.index = None;
        }
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
        match &self.sync_error {
            Some(message) => Err(GitError::remote("pull", message.clone())),
            None => Ok(()),
        }
    }

    fn push(&self) -> Result<(), GitError> {
        self.calls.borrow_mut().push("push".to_string());
        match &self.sync_error {
            Some(message) => Err(GitError::remote("push", message.clone())),
            None => Ok(()),
        }
    }

    fn commits_since(
        &self,
        _since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PastCommit>, GitError> {
        Ok(self.commits.borrow().iter().take(limit).cloned().collect())
    }

    fn branch_overview(&self) -> Result<commitsmith::domain::ports::BranchOverview, GitError> {
        self.calls.borrow_mut().push("branch_overview".to_string());
        Ok(commitsmith::domain::ports::BranchOverview {
            branches: self.branches.borrow().clone(),
            sync: self.branch_sync()?,
        })
    }

    fn branch_sync(&self) -> Result<Vec<(String, usize, usize)>, GitError> {
        Ok(self
            .sync_map
            .iter()
            .map(|(name, (ahead, behind))| (name.clone(), *ahead, *behind))
            .collect())
    }

    fn commits_not_on(
        &self,
        base: &str,
        _limit: usize,
    ) -> Result<Vec<commitsmith::domain::history::PastCommit>, GitError> {
        // A read, not an action: it stays out of the call log.
        let _ = base;
        Ok(self.commits.borrow().clone())
    }

    fn fetch(&self) -> Result<(), GitError> {
        self.calls.borrow_mut().push("fetch".to_string());
        Ok(())
    }

    fn create_tag(&self, tag: &str, _message: &str) -> Result<(), GitError> {
        self.calls.borrow_mut().push(format!("tag {tag}"));
        match &self.tag_error {
            Some(message) => Err(GitError::remote("tag", message.clone())),
            None => Ok(()),
        }
    }

    fn ahead_behind(&self) -> Result<Option<(usize, usize)>, GitError> {
        // A read: it must not show up in the call log that tests compare against.
        Ok(self.sync)
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
