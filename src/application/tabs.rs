//! The three tabs, what they are called, and which keys belong to each.
//!
//! The footer is built from this list instead of from one global key row: a
//! shortcut that does nothing on the active tab must not be advertised, and a
//! pane that grows a key only edits its own entry.

use super::Panel;
use commitsmith::domain::keymap::Keymap;

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

    /// Keys that work on this tab, most important first.
    ///
    /// `commit_mode` only changes the wording of Ctrl+S on the first tab.
    pub fn keys(self, commit_mode: bool, keymap: Keymap) -> Vec<(&'static str, &'static str)> {
        let mut keys = match self {
            Self::Commit => vec![
                ("Tab", "next"),
                ("C-k", "clear"),
                ("C-s", if commit_mode { "commit" } else { "message" }),
                ("S", "setup"),
                ("Space", "type"),
                ("C-Space", "suggest"),
                ("C-p", "profile"),
                ("C-r", "reuse"),
                ("A-p", "pull"),
                ("A-u", "push"),
                // Folding is a convenience; it yields to the keys people need.
                ("C-o", "fold"),
                // The IDE pair rides at the end: Ctrl+S already commits, so these
                // two give up their place first when the line is narrow.
                ("C-Enter", if commit_mode { "commit" } else { "message" }),
                ("C-Space", "suggest"),
                // Replacing the last commit is rare, so it rides at the end.
                ("C-a", "amend"),
            ],
            // The first four are always shown; `d` and `m` are the actions
            // people look for, so they come before the wizard.
            Self::Branches => vec![
                ("↓ ↑", "move"),
                ("Enter", "checkout"),
                ("d", "delete"),
                ("m", "merge"),
                // Pane switching and the file keys yield to the branch actions.
                ("Tab", "pane"),
                ("Space", "stage"),
                ("n", "new branch"),
                ("f", "fetch"),
                // File actions: they belong to the files pane, so they come last.
                ("r", "unstage"),
                ("x", "discard"),
                ("z", "stash"),
            ],
            Self::History => vec![
                ("↓ ↑", "move"),
                ("/", "filter"),
                ("e", "export notes"),
                ("b", "window"),
                ("c", "clear filter"),
            ],
        };
        keys.push(("1-3", "tabs"));
        keys.push(("?", "help"));
        if keymap == Keymap::Ide {
            // Same keys, different order: an editor hand looks for Ctrl+Enter
            // and Ctrl+Space first, a terminal hand for Ctrl+S.
            promote(&mut keys, "C-Enter", 2);
            promote(&mut keys, "C-Space", 4);
        }
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
                ("← →", "move the caret inside the field"),
                ("Tab, Shift+Tab", "always the next or previous field"),
                ("↑ ↓", "through the lines of body, breaking, footer"),
                ("Space, Ctrl+T", "type picker: 1-9 jumps, 0 takes the last"),
                ("Ctrl+N", "start a new, empty commit"),
                ("Ctrl+P", "switch the type profile"),
                ("Ctrl+R", "reuse a recent commit"),
                ("Ctrl+K", "clear this field, the type stays"),
                (
                    "Ctrl+Space",
                    "suggestions for this field (a terminal may send NUL)",
                ),
                ("Ctrl+Enter", "write the message, like Ctrl+S"),
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
                ("↓ ↑", "move through the branches"),
                ("← →", "move the caret, or change the kind"),
                ("Home, End", "jump to the first or the last branch"),
                ("Enter", "check out the selected branch"),
                ("m", "merge the selected branch into the current one"),
                ("d", "delete locally: press twice, Esc keeps it"),
                (
                    "",
                    "wizard: Tab/↑↓ change field, ←→ the text, Esc cancels, Enter creates",
                ),
                ("r", "reload the list"),
                ("n", "new branch from kind, ticket and description"),
                ("", "HEAD and remote-only branches cannot be deleted"),
            ],
            Self::History => vec![
                ("↓ ↑", "through the log"),
                ("/", "filter by type, scope or ticket"),
                ("c", "clear the filter"),
                (
                    "b",
                    "switch the window: this branch, then the last tag, the last 20 and the last 50 commits",
                ),
                (
                    "e",
                    "export the section as RELEASE-NOTES-<window>.md, e.g. RELEASE-NOTES-branch-main.md",
                ),
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

impl Tab {
    /// The footer on tab 2 follows the pane, not the tab: `Space` stages in the
    /// file list and does nothing in the branch list.
    pub fn pane_keys(
        self,
        panel: Panel,
        commit_mode: bool,
        keymap: Keymap,
    ) -> Vec<(&'static str, &'static str)> {
        if self != Self::Branches {
            return self.keys(commit_mode, keymap);
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
fn promote(keys: &mut Vec<(&'static str, &'static str)>, key: &str, index: usize) {
    if let Some(position) = keys.iter().position(|(name, _)| *name == key) {
        let entry = keys.remove(position);
        keys.insert(index.min(keys.len().saturating_sub(2)), entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_footer_on_tab_2_follows_the_pane() {
        let files = Tab::Branches.pane_keys(Panel::Files, false, Keymap::Default);
        assert!(
            files.iter().any(|(key, _)| *key == "r"),
            "unstage is offered here"
        );
        assert!(files.iter().any(|(key, _)| *key == "x"), "discard too");

        let branches = Tab::Branches.pane_keys(Panel::Branches, false, Keymap::Default);
        assert!(
            !branches.iter().any(|(key, _)| *key == "r"),
            "not in the branch list"
        );
        assert!(branches.iter().any(|(key, _)| *key == "m"), "merge is");

        let diff = Tab::Branches.pane_keys(Panel::Diff, false, Keymap::Default);
        assert!(
            diff.iter().any(|(key, _)| *key == "Esc"),
            "the diff closes with Esc"
        );
    }

    #[test]
    fn the_ide_profile_leads_with_its_own_pair() {
        let default = Tab::Commit.keys(false, Keymap::Default);
        assert_eq!(default[2].0, "C-s", "the terminal hand gets Ctrl+S first");
        let ide = Tab::Commit.keys(false, Keymap::Ide);
        assert_eq!(ide[2].0, "C-Enter", "the editor hand gets Ctrl+Enter first");
        assert!(
            ide.iter().any(|(key, _)| *key == "C-s"),
            "Ctrl+S stays available, it is only advertised later"
        );
        assert!(ide.iter().any(|(key, _)| *key == "C-Space"));
    }

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
        for expected in ["filter", "switch the window", "export"] {
            assert!(
                history.iter().any(|(_, text)| text.contains(expected)),
                "{expected} missing: {history:?}"
            );
        }
        assert!(
            !history.iter().any(|(key, _)| *key == "Ctrl+S"),
            "committing is not a key of this tab: {history:?}"
        );
        assert!(
            !history.iter().any(|(key, _)| *key == "y"),
            "`y` was dropped in favour of `e`: {history:?}"
        );
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
        let commit = Tab::Commit.keys(false, Keymap::Default);
        assert!(commit.iter().any(|(key, _)| *key == "C-s"));
        assert!(!commit.iter().any(|(key, _)| *key == "merge"));
        // The composer hints from 0.4.0 must not get lost in the tab rewrite.
        for key in ["Space", "C-Space", "C-p", "C-r"] {
            assert!(
                commit.iter().any(|(candidate, _)| *candidate == key),
                "{key}"
            );
        }

        let branches = Tab::Branches.keys(false, Keymap::Default);
        assert!(branches.iter().any(|(key, _)| *key == "Enter"));
        assert!(!branches.iter().any(|(key, _)| *key == "C-s"));

        let history = Tab::History.keys(false, Keymap::Default);
        assert!(history.iter().any(|(key, _)| *key == "/"));
        assert!(!history.iter().any(|(key, _)| *key == "C-k"));
    }

    #[test]
    fn the_commit_label_follows_the_mode() {
        let label = |mode| {
            Tab::Commit
                .keys(mode, Keymap::Default)
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
            let keys = tab.keys(false, Keymap::Default);
            assert!(keys.iter().any(|(key, _)| *key == "1-3"), "{tab:?}");
            assert!(keys.iter().any(|(key, _)| *key == "?"), "{tab:?}");
        }
    }
}
