//! How arrows behave on a text field, for every tab.
//!
//! One rule instead of one per pane: `←`/`→` belong to the text while a field is
//! being typed into, so the caret can be moved without leaving the field. Only
//! where there is nothing to move through — an empty field, or a field that is
//! not editable at all (the type on tab 1, the kind in the wizard) — they fall
//! back to walking fields. Navigation that is always available stays on `Tab`,
//! `Shift+Tab` and the vi keys in panes that are lists.

use ratatui::crossterm::event::{KeyCode, KeyModifiers};

/// What the caller should do with an arrow key it received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowAction {
    /// Move the caret inside the field the user is typing into.
    Cursor(isize),
    /// Leave the field: `-1` previous, `1` next.
    Field(isize),
    /// Change the selected value of a picker field.
    Cycle(isize),
    /// Not ours.
    Ignore,
}

/// The decision for `key`, given the field's content length and whether the
/// field is a text field the user can type into.
pub fn arrow_action(key: KeyCode, editable: bool, text_len: usize, shift: bool) -> ArrowAction {
    let delta = match key {
        KeyCode::Left => -1,
        KeyCode::Right => 1,
        _ => return ArrowAction::Ignore,
    };

    if !editable {
        // A field that takes no text has nothing to move a caret through.
        return ArrowAction::Cycle(delta);
    }

    // Shift+arrow is the one way to leave the field with the arrows, for people
    // who expect to pan through a form with them.
    if shift {
        return ArrowAction::Field(delta);
    }

    if text_len == 0 {
        // Nothing to move through: walking on is the only useful answer.
        return ArrowAction::Field(delta);
    }

    ArrowAction::Cursor(delta)
}

/// True when the key carries shift.
pub fn is_shift(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::SHIFT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_move_the_caret_inside_a_filled_field() {
        assert_eq!(
            arrow_action(KeyCode::Left, true, 5, false),
            ArrowAction::Cursor(-1)
        );
        assert_eq!(
            arrow_action(KeyCode::Right, true, 5, false),
            ArrowAction::Cursor(1)
        );
    }

    #[test]
    fn arrows_leave_an_empty_field() {
        assert_eq!(
            arrow_action(KeyCode::Right, true, 0, false),
            ArrowAction::Field(1)
        );
        assert_eq!(
            arrow_action(KeyCode::Left, true, 0, false),
            ArrowAction::Field(-1)
        );
    }

    #[test]
    fn shift_arrows_always_walk_fields() {
        assert_eq!(
            arrow_action(KeyCode::Right, true, 5, true),
            ArrowAction::Field(1)
        );
        assert_eq!(
            arrow_action(KeyCode::Left, true, 0, true),
            ArrowAction::Field(-1)
        );
    }

    #[test]
    fn a_field_without_text_cycles_its_value() {
        assert_eq!(
            arrow_action(KeyCode::Left, false, 0, false),
            ArrowAction::Cycle(-1)
        );
        assert_eq!(
            arrow_action(KeyCode::Right, false, 3, false),
            ArrowAction::Cycle(1)
        );
    }

    #[test]
    fn other_keys_are_left_alone() {
        assert_eq!(
            arrow_action(KeyCode::Up, true, 3, false),
            ArrowAction::Ignore
        );
        assert_eq!(
            arrow_action(KeyCode::Char('j'), true, 3, false),
            ArrowAction::Ignore
        );
    }
}
