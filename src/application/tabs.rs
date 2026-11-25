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

    /// The help overlay: only what this tab can actually do.
    ///
    /// An empty key means the line is a note, not a shortcut. Advertising a key
    /// that does nothing here would be worse than showing nothing at all.
    pub fn help(self, commit_mode: bool) -> Vec<(&'static str, &'static str)> {
        match self {
            Self::Commit => vec![
                ("Tab, ↓ ↑", "next or previous field"),
                (
                    "← →",
                    "move the caret · Shift+←/→ changes field · empty field: next field",
                ),
                ("Enter", "next field, or a new line in body and breaking"),
                ("Space, Ctrl+T", "type picker: 1-9 jumps, 0 takes the last"),
                ("Ctrl+N", "scopes from config and history"),
                ("Ctrl+P", "switch the type profile"),
                ("Ctrl+R", "reuse a recent commit"),
                ("Ctrl+K", "clear this field, the type stays"),
                ("S", "setup: author, ticket prefix, template, profile"),
                (
                    "Ctrl+S",
                    if commit_mode {
                        "commit the message and exit"
                    } else {
                        "render the message and exit"
                    },
                ),
                ("", "checks show the same rules as `commitsmith check`"),
            ],
            Self::Branches => vec![
                ("j k, ↓ ↑", "move through the branches"),
                (
                    "← →",
                    "in the wizard: kind, otherwise next field · typing: caret",
                ),
                ("g G", "jump to the first or the last branch"),
                ("Enter", "check out the selected branch"),
                ("m", "merge the selected branch into the current one"),
                ("d", "delete locally: press twice, Esc keeps it"),
                (
                    "",
                    "in the wizard: Tab next field, Esc cancels, Enter creates",
                ),
                ("r", "reload the list"),
                ("n", "new branch from kind, ticket and description"),
                ("", "HEAD and remote-only branches cannot be deleted"),
            ],
            Self::History => vec![
                ("", "the conventional log, its filters and the release"),
                ("", "toolbox arrive with Epic 5"),
            ],
        }
    }

    /// Keys that work on every tab.
    pub fn global_help() -> Vec<(&'static str, &'static str)> {
        vec![
            ("1-3", "switch tab"),
            ("Ctrl+PgUp/PgDn", "previous or next tab"),
            ("?", "close this help"),
            ("Esc, Ctrl+Q", "quit without output"),
        ]
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
    fn the_help_lists_only_what_the_tab_can_do() {
        let commit = Tab::Commit.help(false);
        assert!(commit.iter().any(|(key, _)| *key == "Ctrl+S"));
        assert!(
            !commit.iter().any(|(_, text)| text.contains("check out")),
            "the composer cannot check out branches: {commit:?}"
        );

        let branches = Tab::Branches.help(false);
        for expected in ["check out", "merge", "delete"] {
            assert!(
                branches.iter().any(|(_, text)| text.contains(expected)),
                "{expected} missing: {branches:?}"
            );
        }
        assert!(
            !branches.iter().any(|(key, _)| *key == "Ctrl+S"),
            "committing is not a key of this tab: {branches:?}"
        );

        let history = Tab::History.help(false);
        assert!(history.iter().all(|(key, _)| key.is_empty()));
        assert!(history.iter().any(|(_, text)| text.contains("Epic 5")));
    }

    #[test]
    fn the_global_help_names_the_tab_keys_and_quitting() {
        let globals = Tab::global_help();
        for expected in ["1-3", "?", "Esc, Ctrl+Q"] {
            assert!(
                globals.iter().any(|(key, _)| *key == expected),
                "{expected} missing: {globals:?}"
            );
        }
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
