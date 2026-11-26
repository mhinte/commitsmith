//! One look for "this is where you are".
//!
//! Every pane marks its focus the same way, so the eye learns it once: the
//! focused row gets a filled background, the focused panel gets a bright border
//! and a bold title, everything else stays muted. Panes that hold a list mark the
//! selected row the same way the composer marks its active field.

use ratatui::style::{Color, Modifier, Style};

/// Background of a focused field or selected row.
pub const FOCUS_BG: Color = Color::Indexed(237);
/// Text on the focus background.
pub const FOCUS_FG: Color = Color::White;
/// The marker that also survives a terminal without background colours.
pub const FOCUS_MARKER: &str = "▌ ";

/// Something is fine: a green tick, a clean lint, an added line.
pub const fn ok() -> Style {
    Style::new().fg(Color::Green)
}

/// Something needs a look: a warning, a long subject, an unstaged file.
pub const fn warn() -> Style {
    Style::new().fg(Color::Yellow)
}

/// Something is wrong: a lint error, a removed line, a failed command.
pub const fn bad() -> Style {
    Style::new().fg(Color::Red)
}

/// Keys, counts and the markers the eye follows.
pub const fn accent() -> Style {
    Style::new().fg(Color::Cyan)
}

/// Ordinary text that should not compete for attention.
pub const fn hint() -> Style {
    Style::new().fg(Color::Gray)
}

/// Rules, borders and details that are there when looked for.
pub const fn muted() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// Text on a filled background.
pub const fn bright() -> Style {
    Style::new().fg(Color::White)
}

/// The row the user is working in.
pub fn focused_row() -> Style {
    Style::default()
        .fg(FOCUS_FG)
        .bg(FOCUS_BG)
        .add_modifier(Modifier::BOLD)
}

/// The label of the focused field: bright, so it reads even in a plain terminal.
pub fn focused_label() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

/// A field or row that is not focused.
pub fn idle_row() -> Style {
    Style::new().fg(Color::Gray)
}

/// The title of a panel that does not have the keyboard.
pub fn idle_title() -> Style {
    Style::new().fg(Color::Gray)
}

/// Border and title of the panel that has the keyboard.
pub fn focused_panel() -> Style {
    Style::new().fg(Color::Cyan)
}

/// Border and title of every other panel.
pub fn idle_panel() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// The value inside a focused field, once the row background is set.
pub fn focused_value() -> Style {
    Style::new().fg(FOCUS_FG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focus_style_is_visible_without_colour_backgrounds() {
        // Bold and a distinct foreground: a terminal that drops the background
        // still shows the difference.
        let style = focused_row();
        assert!(style.add_modifier == Modifier::BOLD);
        assert_eq!(style.fg, Some(FOCUS_FG));
        assert_ne!(idle_row().bg, Some(FOCUS_BG));
    }

    #[test]
    fn the_panel_styles_differ() {
        assert_ne!(focused_panel().fg, idle_panel().fg);
    }
}
