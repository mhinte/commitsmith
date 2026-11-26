//! The file list of tab 2: what changed, what is staged, and the diff of the
//! highlighted file.
//!
//! The list holds no git knowledge; everything goes through the port.

use std::cell::RefCell;

use commitsmith::domain::errors::GitError;
use commitsmith::domain::ports::{ChangedFile, GitRepository};

/// A destructive action waiting for a second keypress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// Throw away the changes of this path.
    Discard(String),
    /// Take the newest stash entry back.
    StashPop,
}

#[derive(Debug, Default, Clone)]
pub struct Staging {
    pub items: Vec<ChangedFile>,
    pub selected: usize,
    /// Unified diff of the selected file, loaded on demand. Written through
    /// [`Staging::set_diff`] so the row cache below knows it is stale.
    pub diff: String,
    /// Wrapped-row counts of `diff` at the width they were measured at. One
    /// unicode-width pass over a ten-thousand-line diff costs about a
    /// millisecond, which is felt on every redraw, so it happens once per
    /// (diff, width) instead.
    rows: RefCell<RowCache>,
    pub show_diff: bool,
    /// First diff line on screen; reset whenever another file is shown.
    pub diff_offset: usize,
    /// Set when the next keypress has to confirm a destructive action.
    pub confirm: Option<Confirm>,
    /// True when the selected file is staged (the diff then shows the index).
    pub selected_staged: bool,
}

/// Row counts for the diff as it was last measured.
///
/// `starts[i]` is the first screen row of diff line `i`, so the last entry is
/// the total: the pane then answers "where does the thumb go" with two index
/// reads instead of a pass over ten thousand lines.
#[derive(Debug, Default, Clone)]
struct RowCache {
    width: usize,
    starts: Vec<usize>,
}

impl Staging {
    /// Replace the diff and drop the row cache with it.
    pub fn set_diff(&mut self, diff: String) {
        self.diff = diff;
        *self.rows.borrow_mut() = RowCache::default();
    }

    /// How many rows the whole diff occupies at `width`, and how many of them
    /// lie above row `offset`. Measured once per width.
    pub fn row_span(&self, width: usize, offset: usize) -> (usize, usize) {
        let mut cache = self.rows.borrow_mut();
        if cache.width != width {
            let mut starts = Vec::with_capacity(16);
            let mut row = 0;
            for line in self.diff.lines() {
                starts.push(row);
                row += commitsmith::domain::scroll::line_rows(line, width);
            }
            starts.push(row);
            cache.starts = starts;
            cache.width = width;
        }
        let total = cache.starts.last().copied().unwrap_or(0);
        let before = cache
            .starts
            .get(offset.min(cache.starts.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        (total, before)
    }

    pub fn reload(&mut self, repo: &dyn GitRepository) -> Result<(), GitError> {
        let keep = self.selected_path();
        let mut items = repo.changes()?;
        // Stable order: by path, so a stage toggle does not shuffle the list.
        items.sort_by(|a, b| a.path.cmp(&b.path));
        self.items = items;
        self.selected = keep
            .and_then(|path| self.items.iter().position(|file| file.path == path))
            .unwrap_or_else(|| self.selected.min(self.items.len().saturating_sub(1)));
        // The diff is not read here: `git diff` on a large file is the most
        // expensive part and nobody sees it until the pane is opened.
        self.set_diff(String::new());
        Ok(())
    }

    /// How many files are fully staged — the number the pane title shows.
    pub fn staged_count(&self) -> usize {
        self.items.iter().filter(|file| file.is_staged()).count()
    }

    /// Move the diff window; `height` is what the pane can show.
    pub fn scroll_diff(&mut self, delta: isize, height: usize) {
        let total = self.diff.lines().count();
        let max = total.saturating_sub(height.max(1));
        let next = self.diff_offset as isize + delta;
        self.diff_offset = next.clamp(0, max as isize) as usize;
    }

    /// True when the selected file has something in the index.
    pub fn selected_is_staged(&self) -> bool {
        self.items
            .get(self.selected)
            .map(commitsmith::domain::ports::ChangedFile::is_staged)
            .unwrap_or(false)
    }

    /// True when the selected file has changes in the working tree.
    pub fn selected_has_worktree_changes(&self) -> bool {
        self.items
            .get(self.selected)
            .map(|file| file.worktree.is_some())
            .unwrap_or(false)
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
        // The footer already names the file in full; the status only says what
        // happened to it.
        // A move needs both of its paths: one alone leaves the other end in the
        // index as a deletion, so the rename would never leave the index.
        let origin = file.origin.clone();
        if file.is_staged() {
            match &origin {
                Some(origin) => repo.unstage_move(origin, &file.path)?,
                None => repo.unstage(&file.path)?,
            }
            let _ = self.reload(repo);
            Ok("unstaged".to_string())
        } else {
            match &origin {
                Some(origin) => repo.stage_move(origin, &file.path)?,
                None => repo.stage(&file.path)?,
            }
            let _ = self.reload(repo);
            Ok("staged".to_string())
        }
    }

    /// Read the diff for the highlighted file; the index if it is staged.
    pub fn refresh_diff(&mut self, repo: &dyn GitRepository) -> Result<(), GitError> {
        self.diff_offset = 0;
        let Some(file) = self.selected_file().cloned() else {
            self.set_diff(String::new());
            self.selected_staged = false;
            return Ok(());
        };
        self.selected_staged = file.is_staged();
        let diff = repo.diff(&file.path, self.selected_staged)?;
        self.set_diff(diff);
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
            ChangedFile::new("b.txt", Some(FileState::Modified), None),
            ChangedFile::new("a.txt", None, Some(FileState::Modified)),
        ]);
        repo
    }

    #[test]
    fn a_move_goes_through_stage_move() {
        // The index needs the old path too, or the move never lands.
        let repo = RecordingGit::new(&["main"], "main");
        repo.files
            .borrow_mut()
            .push(ChangedFile::new("b.txt", None, Some(FileState::Renamed)).with_origin("a.txt"));
        let log = repo.calls();
        let mut staging = Staging::default();
        staging.reload(&repo).unwrap();
        staging.toggle(&repo).unwrap();
        assert_eq!(
            *log.borrow(),
            vec!["stage_move a.txt b.txt"],
            "both paths, no plain stage"
        );
        // The move is now staged, so the same key takes it back out — with both
        // paths again, or the old one stays in the index as a deletion.
        staging.reload(&repo).unwrap();
        staging.toggle(&repo).unwrap();
        assert_eq!(
            *log.borrow(),
            vec!["stage_move a.txt b.txt", "unstage_move a.txt b.txt"]
        );
    }

    #[test]
    fn the_diff_span_counts_wrapped_rows_and_follows_the_diff() {
        let mut staging = Staging::default();
        staging.set_diff("short\n".to_string() + &"x".repeat(30) + "\nthird\n");
        // At width 10 the long line needs three rows: 1 + 3 + 1.
        assert_eq!(staging.row_span(10, 0), (5, 0));
        assert_eq!(staging.row_span(10, 2), (5, 4), "two rows above the offset");
        // The same text at a width where everything fits on one row.
        assert_eq!(staging.row_span(80, 0), (3, 0));
        // A new diff is measured again instead of reusing the old heights.
        staging.set_diff("one\ntwo\n".to_string());
        assert_eq!(staging.row_span(10, 0), (2, 0));
        assert_eq!(staging.row_span(10, 1), (2, 1));
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
                .map(commitsmith::domain::ports::ChangedFile::is_staged)
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
        // Reading the list does not read a single diff: that costs a process and
        // nobody sees the result until the pane opens.
        assert!(staging.diff.is_empty(), "{}", staging.diff);
        assert!(
            calls.borrow().iter().all(|call| !call.starts_with("diff ")),
            "{:?}",
            calls.borrow()
        );

        // Opening it reads exactly the selected file.
        staging.refresh_diff(&repo).unwrap();
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

#[cfg(test)]
mod scroll_tests {
    use super::*;

    fn staging_with_diff(lines: usize) -> Staging {
        Staging {
            diff: (0..lines)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            ..Staging::default()
        }
    }

    #[test]
    fn the_diff_window_moves_within_its_bounds() {
        let mut staging = staging_with_diff(50);
        staging.scroll_diff(3, 10);
        assert_eq!(staging.diff_offset, 3);
        staging.scroll_diff(-10, 10);
        assert_eq!(staging.diff_offset, 0, "never before the first line");
        staging.scroll_diff(isize::MAX / 2, 10);
        assert_eq!(staging.diff_offset, 40, "stops where the last page ends");
    }

    #[test]
    fn a_shorter_diff_than_the_window_does_not_scroll() {
        let mut staging = staging_with_diff(3);
        staging.scroll_diff(5, 10);
        assert_eq!(staging.diff_offset, 0);
    }
}
