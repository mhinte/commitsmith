//! The conventional branch wizard, drawn beside the branch list.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::App;
use crate::application::branch_wizard::WizardField;

const MUTED: Style = Style::new().fg(Color::DarkGray);
const HINT: Style = Style::new().fg(Color::Gray);

/// Draws the wizard and returns the caret, when a text field has it.
pub fn draw(frame: &mut Frame, app: &App, area: Rect) -> Option<(u16, u16)> {
    let wizard = &app.wizard;
    let mut lines = Vec::new();

    for field in WizardField::ALL {
        let active = field == wizard.field;
        let marker = if active { "▌ " } else { "  " };
        let label_style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            HINT
        };

        let (value, hint) = match field {
            WizardField::Kind => (wizard.kind.as_str().to_string(), String::new()),
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

        lines.push(Line::from(vec![
            Span::styled(format!(" {:<12}", field.label()), label_style),
            Span::styled(marker, Style::default().fg(Color::Cyan)),
            Span::styled(
                value,
                if active {
                    Style::default().fg(Color::White)
                } else {
                    HINT
                },
            ),
            Span::styled(hint, MUTED),
        ]));
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
                    Style::default().fg(Color::Yellow),
                ));
            }
        }
        Err(error) => {
            lines.push(Line::styled(" branch", MUTED));
            lines.push(Line::styled(
                format!("  {error}"),
                Style::default().fg(Color::Yellow),
            ));
        }
    }

    if let Some(error) = &wizard.error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!(" {error}"),
            Style::default().fg(Color::Red),
        ));
    }

    lines.push(Line::raw(""));
    lines.push(Line::styled(
        " Enter create + checkout · Tab next field · Esc cancel",
        HINT,
    ));

    // The caret sits in the field being typed into, as in the composer: the
    // label (12) + marker (2) come first, then the text.
    let caret = match wizard.field {
        WizardField::Kind => None,
        field => {
            let width = unicode_width::UnicodeWidthStr::width(match field {
                WizardField::Ticket => wizard.ticket.as_str(),
                _ => wizard.description.as_str(),
            }) as u16;
            Some((area.x + 15 + width, area.y + 1 + field.index() as u16))
        }
    };

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block()), area);
    caret
}

fn block() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        .title(Line::from(Span::styled(" new branch ", HINT)))
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
