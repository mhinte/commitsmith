//! One table of bindings, and every surface that shows them reads it.
//!
//! Before this, five places advertised keys: the footer per tab, the help overlay,
//! the two wizards and the miniature view. They disagreed — `C-k` in one, `Ctrl+K`
//! in another — and three of the bindings collided with the terminals people
//! actually use:
//!
//! - `Ctrl+K` is "clear terminal" in VS Code,
//! - `Ctrl+S` is XOFF: it stops the output until `Ctrl+Q`,
//! - `Ctrl+Space` is autocompletion in IDEs and is swallowed by some Windows
//!   terminals,
//! - `Ctrl+Q` is XON, the counterpart that resumes the stopped output.
//!
//! Marc's decision (27.11.2026): no keymap profiles — one set that works
//! everywhere. The four keys above are gone. What replaced them:
//!
//! - `Ctrl+Enter` writes the message (it already existed and collides nowhere),
//! - `Ctrl+U` clears the field (readline's kill-line, passed by every terminal),
//! - `Ctrl+P` opens the palette, which reaches every action that lost its key.

use crate::application::tabs::Tab;

/// What a key does. The palette runs the same actions, so an action exists once
/// even when it has lost its own key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ClearField,
    Finish,
    NewCommit,
    Amend,
    Suggest,
    Reuse,
    TypePicker,
    ProfilePicker,
    ToggleCompact,
    Setup,
    Help,
    Palette,
    Quit,
}

impl Action {
    /// The word the palette shows and the help text uses.
    pub fn label(self) -> &'static str {
        match self {
            Self::ClearField => "clear the field",
            Self::Finish => "write the message",
            Self::NewCommit => "start a new, empty commit",
            Self::Amend => "amend the last commit",
            Self::Suggest => "suggestions for this field",
            Self::Reuse => "reuse a recent commit",
            Self::TypePicker => "pick the type",
            Self::ProfilePicker => "switch the type profile",
            Self::ToggleCompact => "fold or unfold the form",
            Self::Setup => "setup",
            Self::Help => "help",
            Self::Palette => "commands",
            Self::Quit => "quit",
        }
    }

    /// The value stored in a palette entry, so a picked entry can be turned back
    /// into an action without a second table.
    pub fn key(self) -> &'static str {
        match self {
            Self::ClearField => "clear",
            Self::Finish => "finish",
            Self::NewCommit => "new",
            Self::Amend => "amend",
            Self::Suggest => "suggest",
            Self::Reuse => "reuse",
            Self::TypePicker => "type",
            Self::ProfilePicker => "profile",
            Self::ToggleCompact => "fold",
            Self::Setup => "setup",
            Self::Help => "help",
            Self::Palette => "palette",
            Self::Quit => "quit",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [
            Self::ClearField,
            Self::Finish,
            Self::NewCommit,
            Self::Amend,
            Self::Suggest,
            Self::Reuse,
            Self::TypePicker,
            Self::ProfilePicker,
            Self::ToggleCompact,
            Self::Setup,
            Self::Help,
            Self::Palette,
            Self::Quit,
        ]
        .into_iter()
        .find(|action| action.key() == key)
    }

    /// Everything the palette offers, in the order it reads best.
    pub const PALETTE: [Self; 11] = [
        Self::Suggest,
        Self::Reuse,
        Self::TypePicker,
        Self::ProfilePicker,
        Self::ClearField,
        Self::NewCommit,
        Self::Amend,
        Self::ToggleCompact,
        Self::Setup,
        Self::Help,
        Self::Quit,
    ];
}

/// One advertised key: how it is written, what it does, where, and whether the
/// footer may show it.
#[derive(Debug, Clone, Copy)]
pub struct Binding {
    /// The one spelling used everywhere. `Ctrl+Enter`, never `C-Enter`.
    pub keys: &'static str,
    pub tab: Tab,
    /// How the footer names it. Short: the footer has one row.
    pub label: &'static str,
    /// False for the ones that only live in the help overlay: a footer has room
    /// for a handful, the help for all of them.
    pub in_footer: bool,
    pub help: &'static str,
}

const fn key(
    keys: &'static str,
    label: &'static str,
    tab: Tab,
    in_footer: bool,
    help: &'static str,
) -> Binding {
    Binding {
        keys,
        tab,
        label,
        in_footer,
        help,
    }
}

/// A binding without an action: navigation that needs no handler of its own.
/// `key` and `note` read the same now; the name says what the row is for.
const fn note(
    keys: &'static str,
    label: &'static str,
    tab: Tab,
    in_footer: bool,
    help: &'static str,
) -> Binding {
    Binding {
        keys,
        tab,
        label,
        in_footer,
        help,
    }
}

use Tab::{Branches, Commit, History};

/// The whole set, in the order each surface shows it.
pub const BINDINGS: &[Binding] = &[
    // Composer
    note("Tab", "next", Commit, true, "next field"),
    note("↑ ↓", "move", Commit, false, "previous or next field"),
    note(
        "← →",
        "caret",
        Commit,
        false,
        "move the caret inside the field",
    ),
    note(
        "Space",
        "type",
        Commit,
        true,
        "open the type list while the type field is active, otherwise type a space",
    ),
    note(
        "Enter",
        "ok",
        Commit,
        false,
        "open the list for the active field",
    ),
    key(
        "Ctrl+U",
        "clear",
        Commit,
        true,
        "clear this field, the type stays",
    ),
    key("Ctrl+Enter", "message", Commit, true, "write the message"),
    key(
        "Ctrl+P",
        "commands",
        Commit,
        true,
        "commands: everything without a key of its own",
    ),
    key("Ctrl+N", "new", Commit, false, "start a new, empty commit"),
    key("Ctrl+O", "fold", Commit, true, "fold or unfold the form"),
    note("S", "setup", Commit, false, "setup"),
    note("?", "help", Commit, true, "help"),
    note("1-3", "tabs", Commit, true, "tabs"),
    // Branches
    note("↑ ↓", "move", Branches, true, "move"),
    note("Enter", "checkout", Branches, true, "checkout"),
    note("d", "delete", Branches, true, "delete"),
    note("m", "merge", Branches, true, "merge"),
    note("Tab", "pane", Branches, true, "switch pane"),
    note("Space", "stage", Branches, true, "stage the file"),
    note("n", "new branch", Branches, true, "new branch"),
    note("f", "fetch", Branches, true, "fetch"),
    note("r", "unstage", Branches, false, "unstage the file"),
    note("x", "discard", Branches, false, "discard the change"),
    note("z", "stash", Branches, false, "stash"),
    key("Ctrl+P", "commands", Branches, true, "commands"),
    note("?", "help", Branches, true, "help"),
    note("1-3", "tabs", Branches, true, "tabs"),
    // History
    note("↑ ↓", "move", History, true, "move"),
    note("/", "filter", History, true, "filter"),
    note("e", "export notes", History, true, "export the notes"),
    note("b", "window", History, true, "switch the window"),
    note("c", "clear filter", History, true, "clear the filter"),
    note(
        "E",
        "edit message",
        History,
        true,
        "edit the message of a local commit",
    ),
    key("Ctrl+P", "commands", History, true, "commands"),
    note("?", "help", History, true, "help"),
    note("1-3", "tabs", History, true, "tabs"),
];

/// The footer row for a tab: the keys that fit, most important first.
pub fn footer(tab: Tab) -> Vec<(&'static str, &'static str)> {
    BINDINGS
        .iter()
        .filter(|binding| binding.tab == tab && binding.in_footer)
        .map(|binding| (binding.keys, binding.label))
        .collect()
}

/// The help overlay for a tab: everything, including the keys the footer drops.
pub fn help(tab: Tab) -> Vec<(&'static str, &'static str)> {
    BINDINGS
        .iter()
        .filter(|binding| binding.tab == tab)
        .map(|binding| (binding.keys, binding.help))
        .collect()
}

/// The palette entries: label and the value that identifies the action.
pub fn palette_entries() -> Vec<(String, String)> {
    Action::PALETTE
        .iter()
        .map(|action| (action.label().to_string(), action.key().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Action, BINDINGS, footer, help, palette_entries};
    use crate::application::tabs::Tab;

    #[test]
    fn no_binding_collides_with_a_terminal_or_an_ide() {
        // The four keys that used to freeze, swallow or hijack the screen.
        for forbidden in ["Ctrl+K", "Ctrl+S", "Ctrl+Space", "Ctrl+Q"] {
            assert!(
                !BINDINGS.iter().any(|binding| binding.keys == forbidden),
                "{forbidden} is claimed by terminals and IDEs"
            );
        }
    }

    #[test]
    fn one_spelling_everywhere() {
        for binding in BINDINGS {
            assert!(!binding.keys.starts_with("C-"), "{}", binding.keys);
            assert!(!binding.keys.contains("Up/Down"), "{}", binding.keys);
            assert!(!binding.keys.contains("Ctrl+K"), "{}", binding.keys);
        }
    }

    #[test]
    fn every_footer_entry_comes_from_the_table() {
        for tab in Tab::ALL {
            let entries = footer(tab);
            assert!(!entries.is_empty(), "{tab:?} has no footer");
            for (keys, _) in entries {
                assert!(
                    BINDINGS
                        .iter()
                        .any(|binding| binding.tab == tab && binding.keys == keys),
                    "{keys} on {tab:?} is not in the table"
                );
            }
        }
    }

    #[test]
    fn the_help_shows_at_least_what_the_footer_shows() {
        for tab in Tab::ALL {
            let help_keys: Vec<&str> = help(tab).into_iter().map(|(keys, _)| keys).collect();
            for (keys, _) in footer(tab) {
                assert!(
                    help_keys.contains(&keys),
                    "{keys} is advertised in the footer of {tab:?} but missing from its help"
                );
            }
        }
    }

    #[test]
    fn the_palette_reaches_the_actions_that_lost_their_key() {
        let entries = palette_entries();
        for action in [Action::Suggest, Action::Reuse] {
            assert!(
                entries.iter().any(|(_, key)| key == action.key()),
                "{} must stay reachable through the palette",
                action.label()
            );
        }
    }

    #[test]
    fn an_action_survives_the_round_trip_through_its_key() {
        for action in Action::PALETTE {
            assert_eq!(Action::from_key(action.key()), Some(action));
        }
        assert_eq!(Action::from_key("nonsense"), None);
    }
}
