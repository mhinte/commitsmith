//! Tab 3: the log, its filter line and the detail pane.

use std::collections::BTreeMap;

use commitsmith::domain::history::PastCommit;
use commitsmith::domain::log::{self, Filter};

/// What the log pane holds: the filter, the cursor and the open filter line.
#[derive(Debug, Default, Clone)]
pub struct LogState {
    pub filter: Filter,
    /// Position within the filtered list, not within the history.
    pub selected: usize,
    /// `Some` while `/` is open; the text is what has been typed so far.
    pub input: Option<String>,
}

impl LogState {
    pub fn visible<'a>(&self, commits: &'a [PastCommit]) -> Vec<&'a PastCommit> {
        log::filter_commits(commits, &self.filter)
    }

    /// Move inside the filtered list, clamped so the cursor never leaves it.
    pub fn move_by(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        let last = len as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    /// Open the filter line, pre-filled with what is currently filtered.
    pub fn open_filter(&mut self) {
        self.input = Some(self.filter.describe());
    }

    pub fn push(&mut self, c: char) {
        if let Some(text) = &mut self.input {
            text.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(text) = &mut self.input {
            text.pop();
        }
    }

    /// Enter: apply the line. An empty line clears the filter.
    pub fn apply_filter(&mut self, known_types: &[String]) {
        if let Some(text) = self.input.take() {
            self.filter = log::parse_filter(&text, known_types);
            self.selected = 0;
        }
    }

    pub fn cancel_filter(&mut self) {
        self.input = None;
    }

    pub fn clear_filter(&mut self) {
        self.filter = Filter::default();
        self.selected = 0;
    }

    /// Keep the cursor inside the list after the history changed.
    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }
}

/// Counts per type for the pane title, e.g. `feat 4 · fix 2`.
pub fn type_counts(commits: &[PastCommit]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for commit in commits.iter().filter(|commit| commit.conventional()) {
        if let Some(kind) = &commit.kind {
            *counts.entry(kind.as_str()).or_default() += 1;
        }
    }
    counts
        .iter()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(kind: &str, scope: &str) -> PastCommit {
        PastCommit {
            short_id: "abc1234".to_string(),
            id: "abc1234".to_string(),
            author: "Marc".to_string(),
            date: "today".to_string(),
            summary: format!("a {kind} in {scope}"),
            body: String::new(),
            kind: Some(kind.to_string()),
            scope: Some(scope.to_string()),
            files: Vec::new(),
            merge: false,
        }
    }

    #[test]
    fn the_cursor_stays_inside_the_filtered_list() {
        let mut state = LogState::default();
        state.filter.kind = Some("feat".to_string());
        let commits = vec![commit("feat", "ui"), commit("fix", "ui")];
        let visible = state.visible(&commits);
        assert_eq!(visible.len(), 1);

        state.move_by(1, visible.len());
        assert_eq!(state.selected, 0, "one hit, one position");
    }

    #[test]
    fn an_empty_filter_line_clears_the_filter() {
        let mut state = LogState::default();
        state.filter.kind = Some("feat".to_string());
        state.open_filter();
        state.input = Some(String::new());
        state.apply_filter(&[]);
        assert!(state.filter.is_empty());
    }

    #[test]
    fn the_cursor_is_clamped_when_the_history_shrinks() {
        let mut state = LogState {
            selected: 9,
            ..LogState::default()
        };
        state.clamp(3);
        assert_eq!(state.selected, 2);
        state.clamp(0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn the_title_counts_the_types() {
        let commits = vec![
            commit("feat", "ui"),
            commit("feat", "api"),
            commit("fix", "ui"),
        ];
        assert_eq!(type_counts(&commits), "feat 2 · fix 1");
    }
}
