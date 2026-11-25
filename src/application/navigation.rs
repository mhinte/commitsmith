//! Navigation rules, the same on every tab.
//!
//! One idea runs through all panes: **moving between fields and moving inside a
//! field are separate keys.** `Tab`, `Shift+Tab`, `↑` and `↓` always move between
//! fields; `←` and `→` always move through the text.
//!
//! That works because a field's caret is stored even when the field is not being
//! edited: coming back to it with `Tab`/`Shift+Tab` puts the caret back where it
//! was, so `←`/`→` are always useful.
//!
//! The one exception is a field that holds no text at all — the type on tab 1, the
//! kind in the branch wizard. There is nothing to walk through, so `←`/`→` change
//! its value instead.

use ratatui::crossterm::event::KeyCode;

/// What `←`/`→` should do in the focused field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Horizontal {
    /// Move the caret inside the field: `-1` back, `1` forward.
    Caret(isize),
    /// The field holds a value, not text: change it.
    Value(isize),
}

/// Decide for `←`/`→`, given whether the field carries text.
pub fn horizontal(key: KeyCode, has_text: bool) -> Option<Horizontal> {
    let delta = match key {
        KeyCode::Left => -1,
        KeyCode::Right => 1,
        _ => return None,
    };
    Some(if has_text {
        Horizontal::Caret(delta)
    } else {
        Horizontal::Value(delta)
    })
}

/// What `↑`/`↓` should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vertical {
    /// Move between fields: `-1` previous, `1` next.
    Field(isize),
    /// Walk the lines of a multiline field, then leave it at the ends.
    Lines(isize),
}

/// Decide for `↑`/`↓`. Inside a multiline field they walk its lines first.
pub fn vertical(key: KeyCode, multiline: bool) -> Option<Vertical> {
    let delta = match key {
        KeyCode::Up => -1,
        KeyCode::Down => 1,
        _ => return None,
    };
    Some(if multiline {
        Vertical::Lines(delta)
    } else {
        Vertical::Field(delta)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_and_right_move_the_caret_in_a_text_field() {
        assert_eq!(horizontal(KeyCode::Left, true), Some(Horizontal::Caret(-1)));
        assert_eq!(horizontal(KeyCode::Right, true), Some(Horizontal::Caret(1)));
    }

    #[test]
    fn left_and_right_change_the_value_of_a_field_without_text() {
        assert_eq!(
            horizontal(KeyCode::Left, false),
            Some(Horizontal::Value(-1))
        );
        assert_eq!(
            horizontal(KeyCode::Right, false),
            Some(Horizontal::Value(1))
        );
    }

    #[test]
    fn up_and_down_walk_fields_unless_the_field_has_lines() {
        assert_eq!(vertical(KeyCode::Down, false), Some(Vertical::Field(1)));
        assert_eq!(vertical(KeyCode::Up, false), Some(Vertical::Field(-1)));
        assert_eq!(vertical(KeyCode::Down, true), Some(Vertical::Lines(1)));
        assert_eq!(vertical(KeyCode::Up, true), Some(Vertical::Lines(-1)));
    }

    #[test]
    fn other_keys_are_not_ours() {
        assert_eq!(horizontal(KeyCode::Tab, true), None);
        assert_eq!(vertical(KeyCode::Enter, true), None);
    }
}
