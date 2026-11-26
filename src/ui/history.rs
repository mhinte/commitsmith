//! Tab 3, first half: the conventional log with its filter and lint marks.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::App;
use crate::application::log::type_counts;
use crate::ui::theme;

const MUTED: Style = theme::muted();
const HINT: Style = theme::hint();

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let visible = app.release.log.visible(&app.history.commits);
    let [list_area, right] =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Min(42)]).areas(area);
    let [detail_area, release_area] =
        Layout::vertical([Constraint::Percentage(45), Constraint::Min(8)]).areas(right);

    draw_list(frame, app, list_area, visible.len());
    draw_detail(frame, app, detail_area);
    draw_release(frame, app, release_area);
}

fn draw_list(frame: &mut Frame, app: &App, area: Rect, matches: usize) {
    let title = match &app.release.log.input {
        Some(text) => format!(" filter › {text}▏ "),
        None => {
            let counts = type_counts(&app.history.commits);
            let filter = app.release.log.filter.describe();
            if filter.is_empty() {
                format!(" log · {} · {counts} ", app.history.commits.len())
            } else {
                format!(
                    " log · {filter} · {matches} of {} ",
                    app.history.commits.len()
                )
            }
        }
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::focused_panel())
        .title(Line::from(Span::styled(title, theme::focused_panel())));

    let height = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    if app.history.commits.is_empty() {
        lines.push(Line::styled("  no commits yet", MUTED));
    } else if matches == 0 {
        lines.push(Line::styled(
            format!(
                "  keine Treffer für „{}“",
                app.release.log.filter.describe()
            ),
            MUTED,
        ));
        lines.push(Line::styled("  c clears the filter", MUTED));
    } else {
        let offset = app
            .release
            .log
            .selected
            .saturating_sub(height.saturating_sub(1));
        let name_width = area.width.saturating_sub(18) as usize;
        for (index, commit) in app
            .release
            .log
            .visible(&app.history.commits)
            .iter()
            .enumerate()
            .skip(offset)
        {
            if lines.len() >= height {
                break;
            }
            let selected = index == app.release.log.selected;
            let (mark, mark_style) = match app.log_mark(commit) {
                crate::validate::Severity::Error => ("✗", theme::bad()),
                crate::validate::Severity::Warning => ("▲", theme::warn()),
                _ => ("✓", theme::ok()),
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
                    crate::ui::history::fit(&commit.summary, name_width),
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
        .release
        .log
        .visible(&app.history.commits)
        .get(app.release.log.selected)
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
                    theme::ok(),
                ));
            } else {
                for issue in findings.iter().take(6) {
                    let (mark, style) = match issue.severity {
                        crate::validate::Severity::Error => ("✗", theme::bad()),
                        crate::validate::Severity::Warning => ("▲", theme::warn()),
                        _ => ("✓", theme::ok()),
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
    let title = match &app.release.plan {
        Some(plan) => format!(" release · {} ", plan.next),
        None => " release ".to_string(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::idle_panel())
        .title(Line::from(Span::styled(title, theme::idle_title())));

    let height = area.height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    match &app.release.plan {
        None => lines.push(Line::styled("  no repository", MUTED)),
        Some(plan) => {
            lines.push(Line::styled(format!("  {}", plan.headline()), HINT));
            if app.release.confirm {
                lines.push(Line::styled(
                    format!(
                        "  Enter writes CHANGELOG.md for {} · Esc cancels",
                        plan.next
                    ),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ));
            }
            lines.push(Line::raw(""));
            for line in commitsmith::domain::release::render_section(plan, None).lines() {
                if lines.len() >= height {
                    lines.push(Line::styled("  …", MUTED));
                    break;
                }
                let style = if line.starts_with('#') {
                    theme::accent()
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
pub fn fit(text: &str, width: usize) -> String {
    commitsmith::domain::text::fit(text, width)
}
