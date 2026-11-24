//! Rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::app::{App, FIELDS, Field};
use crate::commit::{MAX_SUBJECT, RECOMMENDED_SUBJECT};
use crate::validate::{self, Severity};

const LABEL_WIDTH: usize = 10;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let [header, form, content, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(FIELDS.len() as u16 + 2),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .areas(area);

    draw_header(frame, app, header);
    let cursor = draw_form(frame, app, form);
    draw_content(frame, app, content);
    draw_footer(frame, app, footer);

    if let Some((x, y)) = cursor {
        frame.set_cursor_position(Position::new(x, y));
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Line::from(vec![
            Span::styled(
                " commit-tui ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("conventional commits", Style::default().fg(Color::DarkGray)),
        ]));

    let subject_len = app.commit.subject.chars().count();
    let (errors, warnings) = validate::count(&app.issues);
    let counter_color = if subject_len > MAX_SUBJECT {
        Color::Red
    } else if subject_len > RECOMMENDED_SUBJECT {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let line = Line::from(vec![
        Span::styled(" subject ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{subject_len}/{MAX_SUBJECT}"),
            Style::default().fg(counter_color),
        ),
        Span::raw("   "),
        Span::styled(
            format!("{errors} errors"),
            Style::default().fg(if errors > 0 { Color::Red } else { Color::Green }),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{warnings} warnings"),
            Style::default().fg(if warnings > 0 {
                Color::Yellow
            } else {
                Color::Green
            }),
        ),
    ]);

    frame.render_widget(Paragraph::new(line).block(block), area);
}

/// Draws the form and returns the terminal cursor position, if any.
fn draw_form(frame: &mut Frame, app: &App, area: Rect) -> Option<(u16, u16)> {
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    let mut lines = Vec::with_capacity(FIELDS.len());
    let mut cursor = None;

    for (idx, field) in FIELDS.iter().enumerate() {
        let active = idx == app.field;
        let rows = area.inner(ratatui::layout::Margin::new(1, 1));
        let row_y = rows.y + idx as u16;

        let label_style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let marker = if active { "▌ " } else { "  " };
        let value_style = if active {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        let width = area.width.saturating_sub(LABEL_WIDTH as u16 + 4) as usize;
        let value = app.value(*field);

        let (display, cursor_col, hint) = if *field == Field::Type {
            (format!("[ {value} ]"), None, format!("  {}", field.hint()))
        } else {
            let (display, col) = window(value, app.cursor(), active && app.is_editing(), width);
            let hint = if value.is_empty() {
                format!("  {}", field.hint())
            } else {
                String::new()
            };
            (display, col, hint)
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {:<LABEL_WIDTH$}", field.label()), label_style),
            Span::styled(marker, Style::default().fg(Color::Cyan)),
            Span::styled(display, value_style),
            Span::styled(hint, Style::default().fg(Color::DarkGray)),
        ]));

        if let Some(col) = cursor_col {
            cursor = Some((inner.x + LABEL_WIDTH as u16 + 2 + col as u16, row_y));
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(" compose ", Style::default().fg(Color::Gray))),
        ),
        area,
    );

    cursor
}

/// Cut a single-line value down to `width` characters around the cursor.
fn window(value: &str, cursor: usize, active: bool, width: usize) -> (String, Option<usize>) {
    let chars: Vec<char> = value.chars().collect();
    let width = width.max(1);
    let start = if cursor >= width {
        cursor + 1 - width
    } else {
        0
    }
    .min(chars.len());
    let end = (start + width).min(chars.len());
    let display: String = chars[start..end].iter().collect();
    if active {
        (
            display,
            Some(cursor.saturating_sub(start).min(width.saturating_sub(1))),
        )
    } else {
        (display, None)
    }
}

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    let [preview_area, checks_area] =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)]).areas(area);

    draw_preview(frame, app, preview_area);
    draw_checks(frame, app, checks_area);
}

fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    let message = app.commit.render();
    let empty = app.commit.subject.trim().is_empty();
    let mut text = Text::from(
        message
            .lines()
            .map(|line| Line::raw(line.to_string()))
            .collect::<Vec<_>>(),
    );

    if empty {
        text = Text::from(vec![
            Line::raw(""),
            Line::styled(
                "  start with a subject on the left",
                Style::default().fg(Color::DarkGray),
            ),
            Line::styled(
                "  Ctrl+S hands the message to git",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(" preview ", Style::default().fg(Color::Gray)));

    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_checks(frame: &mut Frame, app: &App, area: Rect) {
    let (errors, warnings) = validate::count(&app.issues);
    let title = format!(" checks · {errors} errors · {warnings} warnings ");

    let mut lines = Vec::new();
    if app.issues.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(" ✓ ", Style::default().fg(Color::Green)),
            Span::styled("ready to commit", Style::default().fg(Color::Green)),
        ]));
    } else {
        for issue in &app.issues {
            let (symbol, color) = match issue.severity {
                Severity::Error => ("✗", Color::Red),
                Severity::Warning => ("!", Color::Yellow),
                Severity::Info => ("·", Color::Gray),
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" {symbol} "), Style::default().fg(color)),
                Span::styled(
                    format!("{:<8}", issue.field),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(issue.message.clone(), Style::default().fg(Color::Gray)),
            ]));
        }
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(title, Style::default().fg(Color::Gray)));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let keys = Line::from(vec![
        Span::styled(
            " Tab ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::styled(" next  ", Style::default().fg(Color::Gray)),
        Span::styled(
            " ←/→ ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::styled(" type  ", Style::default().fg(Color::Gray)),
        Span::styled(
            " Ctrl+S ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::styled(" commit  ", Style::default().fg(Color::Gray)),
        Span::styled(
            " Esc ",
            Style::default().fg(Color::Black).bg(Color::DarkGray),
        ),
        Span::styled(" quit", Style::default().fg(Color::Gray)),
    ]);

    let mut lines = vec![keys];
    if !app.status.is_empty() {
        lines.push(Line::styled(
            format!(" {}", app.status),
            Style::default().fg(Color::Cyan),
        ));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}
