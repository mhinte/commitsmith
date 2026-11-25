//! The staging list of tab 1: what changed, what is staged, and the diff of the
//! highlighted file.
//!
//! The list holds no git knowledge; everything goes through the port.

use commitsmith::domain::errors::GitError;
use commitsmith::domain::ports::{ChangedFile, GitRepository};

/// Which side of tab 1 has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Composer,
    Staging,
}

#[derive(Debug, Default, Clone)]
pub struct Staging {
    pub items: Vec<ChangedFile>,
    pub selected: usize,
    /// Unified diff of the selected file, loaded on demand.
    pub diff: String,
    pub show_diff: bool,
    pub side: Side,
    /// True when the selected file is staged (the diff then shows the index).
    pub selected_staged: bool,
}

impl Staging {
    pub fn reload(&mut self, repo: &dyn GitRepository) -> Result<(), GitError> {
        let keep = self.selected_path();
        let mut items = repo.changes()?;
        // Stable order: by path, so a stage toggle does not shuffle the list.
        items.sort_by(|a, b| a.path.cmp(&b.path));
        self.items = items;
        self.selected = keep
            .and_then(|path| self.items.iter().position(|file| file.path == path))
            .unwrap_or_else(|| self.selected.min(self.items.len().saturating_sub(1)));
        self.refresh_diff(repo)
    }

    pub fn selected_path(&self) -> Option<String> {
        self.items.get(self.selected).map(|file| file.path.clone())
    }

    pub fn selected_file(&self) -> Option<&ChangedFile> {
        self.items.get(self.selected)
    }

    pub fn move_by(&mut self, delta: isize, repo: &dyn GitRepository) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
        let _ = self.refresh_diff(repo);
    }

    /// Stage or unstage the highlighted file, then reload the list.
    pub fn toggle(&mut self, repo: &dyn GitRepository) -> Result<String, GitError> {
        let Some(file) = self.selected_file().cloned() else {
            return Ok("nothing selected".to_string());
        };
        if file.is_staged() {
            repo.unstage(&file.path)?;
            let _ = self.reload(repo);
            Ok(format!("unstaged {}", file.path))
        } else {
            repo.stage(&file.path)?;
            let _ = self.reload(repo);
            Ok(format!("staged {}", file.path))
        }
    }

    /// Read the diff for the highlighted file; the index if it is staged.
    pub fn refresh_diff(&mut self, repo: &dyn GitRepository) -> Result<(), GitError> {
        let Some(file) = self.selected_file().cloned() else {
            self.diff.clear();
            self.selected_staged = false;
            return Ok(());
        };
        self.selected_staged = file.is_staged();
        self.diff = repo.diff(&file.path, self.selected_staged)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::test_support::RecordingGit;
    use commitsmith::domain::ports::FileState;

    fn files() -> RecordingGit {
        let repo = RecordingGit::new(&["main"], "main");
        repo.files.borrow_mut().extend([
            ChangedFile {
                path: "b.txt".to_string(),
                index: Some(FileState::Modified),
                worktree: None,
            },
            ChangedFile {
                path: "a.txt".to_string(),
                index: None,
                worktree: Some(FileState::Modified),
            },
        ]);
        repo
    }

    #[test]
    fn the_list_is_sorted_by_path() {
        let repo = files();
        let mut staging = Staging::default();
        staging.reload(&repo).unwrap();
        let paths: Vec<&str> = staging.items.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "b.txt"], "stable order, not history order");
    }

    #[test]
    fn moving_keeps_the_cursor_in_the_list() {
        let repo = files();
        let mut staging = Staging::default();
        staging.reload(&repo).unwrap();
        staging.move_by(1, &repo);
        assert_eq!(staging.selected_path().as_deref(), Some("b.txt"));
        staging.move_by(1, &repo);
        assert_eq!(staging.selected_path().as_deref(), Some("b.txt"));
        staging.move_by(-1, &repo);
        assert_eq!(staging.selected_path().as_deref(), Some("a.txt"));
    }

    #[test]
    fn toggling_stages_and_unstages_through_the_port() {
        let repo = files();
        let calls = repo.calls();
        let mut staging = Staging::default();
        staging.reload(&repo).unwrap();
        staging.toggle(&repo).unwrap();
        assert!(calls.borrow().iter().any(|call| call == "stage a.txt"));
        staging.reload(&repo).unwrap();
        assert!(
            staging
                .selected_file()
                .map(|f| f.is_staged())
                .unwrap_or(false)
        );
        staging.toggle(&repo).unwrap();
        assert!(calls.borrow().iter().any(|call| call == "unstage a.txt"));
    }

    #[test]
    fn the_diff_is_read_for_the_selected_file_only() {
        let repo = files();
        let calls = repo.calls();
        let mut staging = Staging::default();
        staging.reload(&repo).unwrap();
        assert!(staging.diff.contains("diff --git"), "{}", staging.diff);
        let diffs: Vec<String> = calls
            .borrow()
            .iter()
            .filter(|call| call.starts_with("diff "))
            .cloned()
            .collect();
        assert_eq!(
            diffs,
            ["diff a.txt staged=false"],
            "one file, working-tree view for an unstaged file"
        );
    }
}
