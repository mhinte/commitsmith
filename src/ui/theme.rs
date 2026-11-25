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
    Style::default().fg(Color::Gray)
}

/// The title of a panel that does not have the keyboard.
pub fn idle_title() -> Style {
    Style::default().fg(Color::Gray)
}

/// Border and title of the panel that has the keyboard.
pub fn focused_panel() -> Style {
    Style::default().fg(Color::Cyan)
}

/// Border and title of every other panel.
pub fn idle_panel() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// The value inside a focused field, once the row background is set.
pub fn focused_value() -> Style {
    Style::default().fg(FOCUS_FG)
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
