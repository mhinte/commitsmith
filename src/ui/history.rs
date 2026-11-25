//! Tab 3, first half: the conventional log with its filter and lint marks.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::App;
use crate::application::log::type_counts;
use crate::ui::theme;

const MUTED: Style = Style::new().fg(Color::DarkGray);
const HINT: Style = Style::new().fg(Color::Gray);

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let visible = app.log.visible(&app.history);
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Min(36)]).areas(area);

    draw_list(frame, app, list_area, visible.len());
    draw_detail(frame, app, detail_area);
}

fn draw_list(frame: &mut Frame, app: &App, area: Rect, matches: usize) {
    let title = match &app.log.input {
        Some(text) => format!(" filter › {text}▏ "),
        None => {
            let counts = type_counts(&app.history);
            let filter = app.log.filter.describe();
            if filter.is_empty() {
                format!(" log · {} · {counts} ", app.history.len())
            } else {
                format!(" log · {filter} · {matches} of {} ", app.history.len())
            }
        }
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::focused_panel())
        .title(Line::from(Span::styled(title, theme::focused_panel())));

    let height = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    if app.history.is_empty() {
        lines.push(Line::styled("  no commits yet", MUTED));
    } else if matches == 0 {
        lines.push(Line::styled(
            format!("  keine Treffer für „{}“", app.log.filter.describe()),
            MUTED,
        ));
        lines.push(Line::styled("  c clears the filter", MUTED));
    } else {
        let offset = app.log.selected.saturating_sub(height.saturating_sub(1));
        let name_width = area.width.saturating_sub(24) as usize;
        for (index, commit) in app
            .log
            .visible(&app.history)
            .iter()
            .enumerate()
            .skip(offset)
        {
            if lines.len() >= height {
                break;
            }
            let selected = index == app.log.selected;
            let (mark, mark_style) = match app.log_mark(commit) {
                crate::validate::Severity::Error => ("✗", Style::default().fg(Color::Red)),
                crate::validate::Severity::Warning => ("▲", Style::default().fg(Color::Yellow)),
                _ => ("·", MUTED),
            };
            let kind = match (&commit.kind, &commit.scope) {
                (Some(kind), Some(scope)) => format!("{kind}({scope})"),
                (Some(kind), None) => kind.clone(),
                _ => "plain".to_string(),
            };
            let spans = vec![
                Span::styled(
                    if selected { " ▌" } else { "  " },
                    if selected {
                        theme::focused_label()
                    } else {
                        MUTED
                    },
                ),
                Span::styled(format!(" {mark} "), mark_style),
                Span::styled(format!("{} ", commit.short_id), HINT),
                Span::styled(
                    format!("{kind:<14} "),
                    if selected {
                        theme::focused_value()
                    } else {
                        theme::idle_row()
                    },
                ),
                Span::styled(
                    crate::ui::history::crop(&commit.summary, name_width),
                    if selected {
                        theme::focused_value()
                    } else {
                        HINT
                    },
                ),
            ];
            let row = Line::from(spans);
            lines.push(if selected {
                row.style(theme::focused_row())
            } else {
                row
            });
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

fn draw_detail(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::idle_panel())
        .title(Line::from(Span::styled(" details ", theme::idle_title())));

    let mut lines: Vec<Line> = Vec::new();
    let selected = app
        .log
        .visible(&app.history)
        .get(app.log.selected)
        .copied()
        .cloned();
    match selected {
        None => lines.push(Line::styled("  nothing selected", MUTED)),
        Some(commit) => {
            lines.push(Line::styled(format!("  {}", commit.header()), HINT));
            if !commit.body.trim().is_empty() {
                lines.push(Line::raw(""));
                for line in commit.body.lines().take(12) {
                    lines.push(Line::styled(format!("  {line}"), HINT));
                }
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                format!("  {} · {}", commit.author, commit.date),
                MUTED,
            ));
            let issues = app.log_mark(&commit);
            let (text, style) = match issues {
                crate::validate::Severity::Error => {
                    ("lint: would fail today", Style::default().fg(Color::Red))
                }
                crate::validate::Severity::Warning => (
                    "lint: passes with a warning",
                    Style::default().fg(Color::Yellow),
                ),
                _ => ("lint: clean", Style::default().fg(Color::Green)),
            };
            lines.push(Line::styled(format!("  {text}"), style));
            if !commit.files.is_empty() {
                lines.push(Line::raw(""));
                for file in commit.files.iter().take(8) {
                    lines.push(Line::styled(format!("  {file}"), MUTED));
                }
            }
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
}

/// Cut a line to `width` characters, with an ellipsis.
pub fn crop(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    let mut out: String = chars.into_iter().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
