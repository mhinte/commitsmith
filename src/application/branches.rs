//! The branch list of tab 2.
//!
//! The list itself holds no git knowledge: it is filled through the port, and
//! the actions live in [`crate::application::App`], which owns the repository.

use commitsmith::domain::errors::GitError;
use commitsmith::domain::ports::{BranchInfo, GitRepository};

/// Branches in the explorer: locals first with `HEAD` on top, then remotes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BranchList {
    pub items: Vec<BranchInfo>,
    pub selected: usize,
    /// Set by the first `d`: the second press deletes this branch.
    pub confirming: Option<String>,
}

impl BranchList {
    /// Read the branches from the repository.
    ///
    /// The selection follows the branch it was on, not the row number: after a
    /// checkout the list is re-sorted, and jumping to whatever happens to be at
    /// the old index would feel like the list scrolled on its own.
    pub fn reload(&mut self, repo: &dyn GitRepository) -> Result<(), GitError> {
        let keep = self.current().map(|branch| branch.name.clone());
        let mut items = repo.branches()?;
        items.sort_by(|a, b| {
            (a.is_remote, !a.is_head, a.name.as_str()).cmp(&(
                b.is_remote,
                !b.is_head,
                b.name.as_str(),
            ))
        });
        self.items = items;
        self.selected = keep
            .and_then(|name| self.items.iter().position(|branch| branch.name == name))
            .or_else(|| self.items.iter().position(|branch| branch.is_head))
            .unwrap_or(0);
        self.confirming = None;
        Ok(())
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    pub fn jump_to_start(&mut self) {
        self.selected = 0;
    }

    pub fn jump_to_end(&mut self) {
        self.selected = self.items.len().saturating_sub(1);
    }

    pub fn current(&self) -> Option<&BranchInfo> {
        self.items.get(self.selected)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
