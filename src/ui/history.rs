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
    let [list_area, right] =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Min(42)]).areas(area);
    let [detail_area, release_area] =
        Layout::vertical([Constraint::Percentage(45), Constraint::Min(8)]).areas(right);

    draw_list(frame, app, list_area, visible.len());
    draw_detail(frame, app, detail_area);
    draw_release(frame, app, release_area);
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
        let name_width = area.width.saturating_sub(18) as usize;
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
                    format!("{kind:<9} "),
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
            lines.push(Line::raw(""));
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
            // What exactly is wrong, not only that something is: the message is
            // the one the composer would show.
            // Only what would block a commit: the "optional: …" hints belong in
            // the composer, where they help while writing, not in a review.
            let findings: Vec<_> = app
                .log_issues(&commit)
                .into_iter()
                .filter(|issue| issue.severity != crate::validate::Severity::Info)
                .collect();
            if findings.is_empty() {
                lines.push(Line::styled(
                    "  ✓ lint: clean · would pass the hook today",
                    Style::default().fg(Color::Green),
                ));
            } else {
                for issue in findings.iter().take(6) {
                    let (mark, style) = match issue.severity {
                        crate::validate::Severity::Error => ("✗", Style::default().fg(Color::Red)),
                        crate::validate::Severity::Warning => {
                            ("▲", Style::default().fg(Color::Yellow))
                        }
                        _ => ("·", MUTED),
                    };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {mark} "), style),
                        Span::styled(format!("{}: ", issue.field), MUTED),
                        Span::styled(issue.message.clone(), HINT),
                    ]));
                }
            }
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

/// The next release: what it would be called, why, and the section that would be
/// written. Nothing here writes anything.
fn draw_release(frame: &mut Frame, app: &App, area: Rect) {
    let title = match &app.plan {
        Some(plan) => format!(" release · {} ", plan.next),
        None => " release ".to_string(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::idle_panel())
        .title(Line::from(Span::styled(title, theme::idle_title())));

    let height = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    match &app.plan {
        None => lines.push(Line::styled("  no repository", MUTED)),
        Some(plan) => {
            lines.push(Line::styled(format!("  {}", plan.headline()), HINT));
            if app.release_confirm {
                lines.push(Line::styled(
                    format!(
                        "  Enter writes CHANGELOG.md and tags v{} · Esc cancels",
                        plan.next
                    ),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ));
            }
            lines.push(Line::raw(""));
            for line in commitsmith::domain::release::render_section(plan).lines() {
                if lines.len() >= height {
                    lines.push(Line::styled("  …", MUTED));
                    break;
                }
                let style = if line.starts_with('#') {
                    Style::default().fg(Color::Cyan)
                } else {
                    HINT
                };
                lines.push(Line::styled(format!("  {line}"), style));
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
