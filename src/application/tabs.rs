//! The three tabs, what they are called, and which keys belong to each.
//!
//! The footer is built from this list instead of from one global key row: a
//! shortcut that does nothing on the active tab must not be advertised, and a
//! pane that grows a key only edits its own entry.

use super::Panel;

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
            Self::Commit => "Commit",
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

    /// Keys that work on this tab, straight from the one table.
    ///
    /// `commit_mode` is gone from the signature: the table says `Ctrl+Enter`
    /// writes the message, and whether that commits or prints is a property of
    /// the run, not of the key.
    pub fn keys(self) -> Vec<(&'static str, &'static str)> {
        crate::application::keys::footer(self)
    }

    /// The help overlay: everything from the table, including what the footer drops.
    pub fn help(self) -> Vec<(&'static str, &'static str)> {
        crate::application::keys::help(self)
    }

    /// Keys that work on every tab.
    pub fn global_help() -> Vec<(&'static str, &'static str)> {
        vec![
            ("1-3", "switch tab"),
            ("Ctrl+PgUp/PgDn", "previous or next tab"),
            ("?", "close this help"),
            ("Ctrl+C", "quit without output"),
        ]
    }
}

impl Tab {
    /// The footer on tab 2 follows the pane, not the tab: `Space` stages in the
    /// file list and does nothing in the branch list.
    pub fn pane_keys(self, panel: Panel) -> Vec<(&'static str, &'static str)> {
        if self != Self::Branches {
            return self.keys();
        }
        let mut keys = match panel {
            Panel::Branches => vec![
                ("↓ ↑", "move"),
                ("Enter", "checkout"),
                ("d", "delete"),
                ("m", "merge"),
                ("n", "new branch"),
                ("f", "fetch"),
                ("Tab", "pane"),
            ],
            Panel::Files => vec![
                ("↓ ↑", "move"),
                ("Space", "stage"),
                ("v", "diff"),
                ("r", "unstage"),
                ("x", "discard"),
                ("z", "stash"),
                ("Tab", "pane"),
            ],
            Panel::Diff => vec![
                ("↓ ↑", "scroll"),
                ("PgUp PgDn", "page"),
                ("Esc", "close"),
                ("Tab", "pane"),
            ],
        };
        keys.push(("1-3", "tabs"));
        keys.push(("?", "help"));
        keys
    }
}

/// Move the entry with this key to `index`, keeping the rest in order.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_and_arrows_agree_with_the_order() {
        assert_eq!(Tab::ALL.map(super::Tab::number), ['1', '2', '3']);
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
        let commit = Tab::Commit.help();
        assert!(commit.iter().any(|(key, _)| *key == "Ctrl+Enter"));
        assert!(
            !commit.iter().any(|(_, text)| text.contains("checkout")),
            "the composer cannot check out branches: {commit:?}"
        );

        let branches = Tab::Branches.help();
        for expected in ["checkout", "merge", "delete"] {
            assert!(
                branches.iter().any(|(_, text)| text.contains(expected)),
                "{expected} missing: {branches:?}"
            );
        }

        let history = Tab::History.help();
        for expected in ["filter", "export"] {
            assert!(
                history.iter().any(|(_, text)| text.contains(expected)),
                "{expected} missing: {history:?}"
            );
        }
    }

    #[test]
    fn no_tab_advertises_a_key_that_collides_with_a_terminal() {
        for tab in Tab::ALL {
            for (keys, _) in tab.help().into_iter().chain(tab.keys()) {
                for forbidden in ["Ctrl+S", "Ctrl+K", "Ctrl+Space", "Ctrl+Q"] {
                    assert_ne!(keys, forbidden, "{tab:?} still offers {forbidden}");
                }
            }
        }
    }

    #[test]
    fn the_global_help_names_the_tab_keys_and_quitting() {
        let globals = Tab::global_help();
        for expected in ["1-3", "?", "Ctrl+C"] {
            assert!(
                globals.iter().any(|(key, _)| *key == expected),
                "{expected} missing: {globals:?}"
            );
        }
    }

    #[test]
    fn the_tab_hint_and_help_are_on_every_tab() {
        for tab in Tab::ALL {
            let keys = tab.keys();
            assert!(keys.iter().any(|(key, _)| *key == "1-3"), "{tab:?}");
            assert!(keys.iter().any(|(key, _)| *key == "?"), "{tab:?}");
        }
    }

    #[test]
    fn the_footer_follows_the_pane_on_the_branch_tab() {
        let files = Tab::Branches.pane_keys(Panel::Files);
        let branches = Tab::Branches.pane_keys(Panel::Branches);
        let diff = Tab::Branches.pane_keys(Panel::Diff);
        assert!(files.iter().any(|(key, _)| *key == "Space"), "{files:?}");
        assert!(
            !branches.iter().any(|(key, _)| *key == "Space"),
            "{branches:?}"
        );
        assert!(!diff.is_empty(), "{diff:?}");
    }
}
