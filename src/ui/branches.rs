//! Tab 2: the branch explorer.
//!
//! One list, locals first and `HEAD` on top, then the remotes under a heading.
//! The actions live in the application layer; this module only draws state.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::App;

const MUTED: Style = Style::new().fg(Color::DarkGray);
const HINT: Style = Style::new().fg(Color::Gray);

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if app.git.is_none() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("  no repository ", Style::default().fg(Color::Yellow)),
            Span::styled("· open commitsmith inside a checkout", MUTED),
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::styled("  1 back to the composer", HINT));
        frame.render_widget(Paragraph::new(Text::from(lines)).block(block(app)), area);
        return;
    }

    if app.branches.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("  no local branches yet", MUTED));
        frame.render_widget(Paragraph::new(Text::from(lines)).block(block(app)), area);
        return;
    }

    // Keep the selected row in view without a scroll widget: the list is short
    // and an offset is all the explorer needs.
    let height = area.height.saturating_sub(2) as usize;
    let offset = app
        .branches
        .selected
        .saturating_sub(height.saturating_sub(1));
    let mut remote_heading_drawn = false;

    for (index, branch) in app.branches.items.iter().enumerate().skip(offset) {
        if lines.len() >= height {
            break;
        }
        if branch.is_remote && !remote_heading_drawn {
            remote_heading_drawn = true;
            lines.push(Line::styled("  remotes", MUTED));
        }
        let selected = index == app.branches.selected;
        let marker = if selected { "▸ " } else { "  " };
        let head = if branch.is_head { "● " } else { "○ " };
        let width = area.width.saturating_sub(12) as usize;
        let name = shorten(&branch.name, width);

        let row = Line::from(vec![
            Span::styled(
                format!(" {marker}"),
                if selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    MUTED
                },
            ),
            Span::styled(
                head,
                if branch.is_head {
                    Style::default().fg(Color::Green)
                } else {
                    MUTED
                },
            ),
            Span::styled(
                format!("{name:<width$}"),
                if selected {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else if branch.is_remote {
                    MUTED
                } else {
                    HINT
                },
            ),
            Span::styled(
                branch
                    .upstream
                    .as_ref()
                    .map(|upstream| format!(" ↑{upstream}"))
                    .unwrap_or_default(),
                MUTED,
            ),
        ]);
        lines.push(row);
    }

    if let Some(name) = &app.branches.confirming {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(" delete ", Style::default().fg(Color::Yellow)),
            Span::styled(format!("{name} · press d again, Esc keeps it "), HINT),
        ]));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block(app)), area);
}

fn block(app: &App) -> Block<'static> {
    let count = app.branches.len();
    let title = if count == 1 {
        " branches · 1 ".to_string()
    } else {
        format!(" branches · {count} ")
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        .title(Line::from(Span::styled(title, HINT)))
}

/// Cut a branch name to `width` characters, keeping the end where the type and
/// ticket live in a conventional name.
fn shorten(name: &str, width: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= width || width < 4 {
        return name.to_string();
    }
    let keep = width.saturating_sub(1);
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_keeps_the_end_of_the_name() {
        assert_eq!(
            shorten("feat/mmu/TICKET-1/add-thing", 80),
            "feat/mmu/TICKET-1/add-thing"
        );
        assert_eq!(shorten("feat/mmu/TICKET-1/add-thing", 10), "…add-thing");
    }
}
