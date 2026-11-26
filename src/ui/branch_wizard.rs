//! The conventional branch wizard, drawn beside the branch list.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::App;
use crate::application::branch_wizard::WizardField;
use crate::ui::theme;

const MUTED: Style = theme::muted();
const HINT: Style = theme::hint();

/// Columns from the panel's left edge to the first character of a value: the
/// border, one leading space, the padded label, then the `▌ ` marker. The caret
/// is placed with this offset so it cannot drift away from the text.
const VALUE_OFFSET: u16 = 1 + 1 + 12 + 2;

/// Draws the wizard and returns the caret, when a text field has it.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) -> Option<(u16, u16)> {
    let wizard = &app.branches.wizard;
    let mut lines = Vec::new();

    for field in WizardField::ALL {
        let active = field == wizard.field;

        let (value, hint) = match field {
            // Bracketed like the type field on tab 1, so the same thing reads the
            // same way in both places.
            WizardField::Kind => (format!("[ {} ]", wizard.kind.as_str()), String::new()),
            WizardField::Ticket => (
                wizard.ticket.clone(),
                if wizard.ticket.is_empty() {
                    format!("  {} number or JIRA key", app.config.ticket_prefix)
                } else {
                    String::new()
                },
            ),
            WizardField::Description => (
                wizard.description.clone(),
                if wizard.description.is_empty() {
                    "  what changes, as words".to_string()
                } else {
                    String::new()
                },
            ),
        };

        if active {
            // The focused row gets a filled background across the panel, so the
            // field you are typing into is unmistakable.
            let row = vec![
                Span::styled(format!(" {:<12}", field.label()), theme::focused_label()),
                Span::styled(theme::FOCUS_MARKER, theme::focused_label()),
                Span::styled(value, theme::focused_value()),
                Span::styled(hint, theme::focused_value()),
            ];
            lines.push(Line::from(row).style(theme::focused_row()));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!(" {:<12}", field.label()), HINT),
                Span::styled("  ", HINT),
                Span::styled(value, HINT),
                Span::styled(hint, MUTED),
            ]));
        }
    }

    lines.push(Line::raw(""));

    // The name is built from the same domain rules as a real branch; when an
    // input is missing, say what is missing instead of showing a fragment.
    let preview = wizard.preview(
        app.config
            .author
            .as_deref()
            .or(app.config.prefix.as_deref()),
        &app.config.ticket_prefix,
        &app.config.branch_template,
    );
    match preview {
        Ok(preview) => {
            lines.push(Line::styled(" branch", MUTED));
            let width = area.width.saturating_sub(4) as usize;
            lines.push(Line::styled(
                format!("  {}", shorten(&preview.name, width)),
                Style::default()
                    .fg(if preview.is_complete {
                        Color::Green
                    } else {
                        Color::Yellow
                    })
                    .add_modifier(Modifier::BOLD),
            ));
            if !preview.is_complete {
                lines.push(Line::styled(
                    "  add a description to make it real",
                    theme::warn(),
                ));
            }
        }
        Err(error) => {
            lines.push(Line::styled(" branch", MUTED));
            lines.push(Line::styled(format!("  {error}"), theme::warn()));
        }
    }

    if let Some(error) = &wizard.error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(format!(" {error}"), theme::bad()));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        " Enter create + checkout · Tab next field · Esc cancel",
        HINT,
    ));

    // The caret sits at its position inside the text, as in the composer — and
    // on the first character of an empty field.
    let caret = wizard.current_text().map(|text| {
        let before: String = text.chars().take(wizard.cursor()).collect();
        let width = unicode_width::UnicodeWidthStr::width(before.as_str()) as u16;
        (
            area.x + VALUE_OFFSET + width,
            area.y + 1 + wizard.field.index() as u16,
        )
    });

    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(block(wizard, area.width)),
        area,
    );
    caret
}

fn block(wizard: &crate::application::branch_wizard::BranchWizard, width: u16) -> Block<'static> {
    // The panel that has the keyboard has the bright border.
    let (border, title) = if wizard.open {
        (theme::focused_panel(), theme::focused_panel())
    } else {
        (theme::idle_panel(), theme::idle_title())
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::from(Span::styled(" new branch ", title)))
        // Same place as the type description on tab 1: top right, where the eye
        // already looks for it.
        .title_top(
            Line::from(Span::styled(
                if width >= 32 {
                    format!(" {} · {} ", wizard.kind.as_str(), wizard.kind.description())
                } else {
                    String::new()
                },
                title,
            ))
            .right_aligned(),
        )
}

fn shorten(name: &str, width: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= width || width < 4 {
        return name.to_string();
    }
    let keep = width - 1;
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_offset_matches_the_drawn_row() {
        // "│" + " " + label(12) + "▌ " is what the row looks like; the caret has
        // to start exactly where the text starts.
        let row = format!("│ {:<12}▌ ", "description");
        assert_eq!(
            row.chars().count() as u16,
            VALUE_OFFSET,
            "the constant and the layout drifted apart: {row}"
        );
    }
}
