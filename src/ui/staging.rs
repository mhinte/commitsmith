//! Tab 1, left side: what changed, what is staged, and the diff on demand.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::App;
use crate::application::staging::Side;
use crate::ui::theme;

const MUTED: Style = Style::new().fg(Color::DarkGray);
const HINT: Style = Style::new().fg(Color::Gray);

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let staging = &app.staging;
    let focused = staging.side == Side::Staging;

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme::focused_panel()
        } else {
            theme::idle_panel()
        })
        .title(Line::from(Span::styled(
            if staging.items.is_empty() {
                " staging ".to_string()
            } else {
                format!(" staging · {} ", staging.items.len())
            },
            if focused {
                theme::focused_panel()
            } else {
                theme::idle_title()
            },
        )));

    let mut lines: Vec<Line> = Vec::new();
    if staging.items.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("  nothing changed", MUTED));
        lines.push(Line::styled("  edit a file to see it here", MUTED));
    } else {
        for (index, file) in staging.items.iter().enumerate() {
            let selected = index == staging.selected;
            let code = file
                .index
                .map(|state| state.code().to_string())
                .or_else(|| file.worktree.map(|state| state.code().to_string()))
                .unwrap_or_else(|| "?".to_string());
            let staged = file.is_staged();
            let row = Line::from(vec![
                Span::styled(
                    if selected { " ▌" } else { "  " },
                    if selected {
                        theme::focused_label()
                    } else {
                        MUTED
                    },
                ),
                Span::styled(
                    format!(" {code} "),
                    if staged {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Yellow)
                    },
                ),
                Span::styled(
                    file.path.clone(),
                    if selected {
                        theme::focused_value()
                    } else {
                        theme::idle_row()
                    },
                ),
            ]);
            lines.push(if selected {
                row.style(theme::focused_row())
            } else {
                row
            });
        }
    }

    let [list_area, diff_area] = if staging.show_diff {
        ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Min(4),
            ratatui::layout::Constraint::Percentage(45),
        ])
        .areas(area)
    } else {
        [area, Rect::default()]
    };

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), list_area);
    if staging.show_diff {
        draw_diff(frame, app, diff_area);
    }
}

fn draw_diff(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::idle_panel())
        .title(Line::from(Span::styled(" diff ", theme::idle_title())));

    let mut lines = Vec::new();
    for raw in app.staging.diff.lines() {
        let style = if raw.starts_with('+') && !raw.starts_with("+++") {
            Style::default().fg(Color::Green)
        } else if raw.starts_with('-') && !raw.starts_with("---") {
            Style::default().fg(Color::Red)
        } else if raw.starts_with("@@") {
            Style::default().fg(Color::Cyan)
        } else if raw.starts_with("diff --git") || raw.starts_with("index ") {
            MUTED
        } else {
            HINT
        };
        lines.push(Line::styled(raw.to_string(), style));
    }
    if lines.is_empty() {
        lines.push(Line::styled("  no diff for this file", MUTED));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}
