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
    /// `name -> (ahead, behind)` for branches that have an upstream.
    pub sync: std::collections::HashMap<String, (usize, usize)>,
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
        // A repository that cannot answer this is not a reason to lose the list.
        self.sync = repo
            .branch_sync()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, ahead, behind)| (name, (ahead, behind)))
            .collect();
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

    /// How far a local branch is from its upstream, in the shortest form that
    /// still says something: `↑2 ↓1`, `✓` or `no upstream`.
    pub fn sync_label(&self, branch: &BranchInfo) -> Option<String> {
        if branch.is_remote {
            return None;
        }
        match (branch.upstream.as_ref(), self.sync.get(&branch.name)) {
            (None, _) => Some("no upstream".to_string()),
            (Some(_), Some((0, 0))) | (Some(_), None) => Some("✓".to_string()),
            (Some(_), Some((ahead, behind))) => Some(format!("↑{ahead} ↓{behind}")),
        }
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

#[cfg(test)]
mod sync_tests {
    use super::*;

    fn list(sync: &[(&str, usize, usize)]) -> BranchList {
        BranchList {
            sync: sync
                .iter()
                .map(|(name, ahead, behind)| (name.to_string(), (*ahead, *behind)))
                .collect(),
            ..BranchList::default()
        }
    }

    fn branch(name: &str, upstream: Option<&str>, remote: bool) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            is_head: false,
            is_remote: remote,
            upstream: upstream.map(|value| value.to_string()),
        }
    }

    #[test]
    fn the_sync_label_spells_out_every_state() {
        let list = list(&[
            ("ahead", 2, 0),
            ("behind", 0, 3),
            ("both", 2, 1),
            ("level", 0, 0),
        ]);
        let label = |name: &str, upstream: Option<&str>, remote: bool| {
            list.sync_label(&branch(name, upstream, remote))
        };
        assert_eq!(
            label("ahead", Some("origin/ahead"), false).as_deref(),
            Some("↑2 ↓0")
        );
        assert_eq!(
            label("behind", Some("origin/behind"), false).as_deref(),
            Some("↑0 ↓3")
        );
        assert_eq!(
            label("both", Some("origin/both"), false).as_deref(),
            Some("↑2 ↓1")
        );
        assert_eq!(
            label("level", Some("origin/level"), false).as_deref(),
            Some("✓")
        );
        assert_eq!(label("lonely", None, false).as_deref(), Some("no upstream"));
        assert_eq!(label("origin/main", Some("origin/main"), true), None);
    }
}
