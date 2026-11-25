//! The three tabs, what they are called, and which keys belong to each.
//!
//! The footer is built from this list instead of from one global key row: a
//! shortcut that does nothing on the active tab must not be advertised, and a
//! pane that grows a key only edits its own entry.

/// The top-level surfaces of the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tab {
    /// Staging, composer, preview and checks.
    Commit,
    /// Branch explorer and the conventional branch wizard.
    Branches,
    /// Conventional log, filters and the release toolbox.
    History,
}

impl Tab {
    pub const ALL: [Self; 3] = [Self::Commit, Self::Branches, Self::History];

    /// The digit that jumps here, and the number shown in the bar.
    pub fn number(self) -> char {
        match self {
            Self::Commit => '1',
            Self::Branches => '2',
            Self::History => '3',
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }

    /// Short name for the tab bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Commit => "Commit & Stage",
            Self::Branches => "Branches",
            Self::History => "Historie & Release",
        }
    }

    /// The tab a digit selects, if any.
    pub fn from_digit(digit: char) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.number() == digit)
    }

    /// Next tab to the right, wrapping at the end.
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    /// Keys that work on this tab, most important first.
    ///
    /// `commit_mode` only changes the wording of Ctrl+S on the first tab.
    pub fn keys(self, commit_mode: bool) -> Vec<(&'static str, &'static str)> {
        let mut keys = match self {
            Self::Commit => vec![
                ("Tab", "next"),
                ("C-k", "clear"),
                ("C-s", if commit_mode { "commit" } else { "message" }),
                ("S", "setup"),
                ("Space", "type"),
                ("C-n", "scope"),
                ("C-p", "profile"),
                ("C-r", "reuse"),
            ],
            // The first four are always shown; `d` and `m` are the actions
            // people look for, so they come before the wizard.
            Self::Branches => vec![
                ("j/k", "move"),
                ("Enter", "checkout"),
                ("d", "delete"),
                ("m", "merge"),
                ("n", "new branch"),
            ],
            Self::History => vec![
                ("j/k", "move"),
                ("/", "filter"),
                ("Enter", "details"),
                ("y", "changelog"),
            ],
        };
        keys.push(("1-3", "tabs"));
        keys.push(("?", "help"));
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_and_arrows_agree_with_the_order() {
        assert_eq!(Tab::ALL.map(|tab| tab.number()), ['1', '2', '3']);
        for (index, tab) in Tab::ALL.into_iter().enumerate() {
            assert_eq!(tab.index(), index);
            assert_eq!(Tab::from_digit(tab.number()), Some(tab));
            assert_eq!(tab.next().previous(), tab);
        }
        assert_eq!(Tab::History.next(), Tab::Commit);
        assert_eq!(Tab::Commit.previous(), Tab::History);
        assert_eq!(Tab::from_digit('9'), None);
    }

    #[test]
    fn every_tab_names_its_own_keys() {
        let commit = Tab::Commit.keys(false);
        assert!(commit.iter().any(|(key, _)| *key == "C-s"));
        assert!(!commit.iter().any(|(key, _)| *key == "merge"));
        // The composer hints from 0.4.0 must not get lost in the tab rewrite.
        for key in ["Space", "C-n", "C-p", "C-r"] {
            assert!(
                commit.iter().any(|(candidate, _)| *candidate == key),
                "{key}"
            );
        }

        let branches = Tab::Branches.keys(false);
        assert!(branches.iter().any(|(key, _)| *key == "Enter"));
        assert!(!branches.iter().any(|(key, _)| *key == "C-s"));

        let history = Tab::History.keys(false);
        assert!(history.iter().any(|(key, _)| *key == "/"));
        assert!(!history.iter().any(|(key, _)| *key == "C-k"));
    }

    #[test]
    fn the_commit_label_follows_the_mode() {
        let label = |mode| {
            Tab::Commit
                .keys(mode)
                .into_iter()
                .find(|(key, _)| *key == "C-s")
                .map(|(_, label)| label.to_string())
                .unwrap()
        };
        assert_eq!(label(true), "commit");
        assert_eq!(label(false), "message");
    }

    #[test]
    fn the_tab_hint_and_help_are_on_every_tab() {
        for tab in Tab::ALL {
            let keys = tab.keys(false);
            assert!(keys.iter().any(|(key, _)| *key == "1-3"), "{tab:?}");
            assert!(keys.iter().any(|(key, _)| *key == "?"), "{tab:?}");
        }
    }
}
