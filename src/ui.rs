//! Rendering. Two layouts: the full form for normal terminals and a
//! single-field view for small ones.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Row, Table, Wrap};

use crate::app::{App, FIELDS, Field, Picker, PickerKind};
use crate::validate::{self, Severity};

const LABEL_WIDTH: usize = 10;
/// Rows a multiline field may occupy in the form.
const MULTILINE_ROWS: u16 = 3;
/// Below this height the full form does not fit, so one field is shown at a time.
const COMPACT_HEIGHT: u16 = 24;
const COMPACT_WIDTH: u16 = 62;

const KEY: Style = Style::new().fg(Color::Black).bg(Color::DarkGray);
const HINT: Style = Style::new().fg(Color::Gray);
const MUTED: Style = Style::new().fg(Color::DarkGray);

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if area.height < COMPACT_HEIGHT || area.width < COMPACT_WIDTH {
        draw_compact(frame, app, area);
    } else {
        draw_full(frame, app, area);
    }

    if let Some(picker) = &app.picker {
        draw_picker(frame, picker, area);
    }
    if app.show_help {
        draw_help(frame, area);
    }
}

fn draw_full(frame: &mut Frame, app: &App, area: Rect) {
    let multiline = 2 * (MULTILINE_ROWS - 1);
    let [header, form, content, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(FIELDS.len() as u16 + 2 + multiline),
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
        .border_style(MUTED)
        .title(Line::from(vec![
            Span::styled(
                " commit-tui ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(app.type_label(), HINT),
        ]))
        .title_top(
            Line::from(Span::styled(
                if app.repo_line.is_empty() {
                    format!(" {} ", app.config.profile)
                } else {
                    format!(" {} · {} ", app.config.profile, app.repo_line)
                },
                MUTED,
            ))
            .right_aligned(),
        );

    let subject_len = app.commit.subject.chars().count();
    let max = app.rules.max_subject;
    let (errors, warnings) = validate::count(&app.issues);
    let counter_color = if subject_len > max {
        Color::Red
    } else if subject_len > app.rules.recommended_subject {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let line = Line::from(vec![
        Span::styled(" subject ", MUTED),
        Span::styled(
            format!("{subject_len}/{max}"),
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
    let mut lines = Vec::new();
    let mut cursor = None;
    let mut row_y = inner.y;
    let width = area.width.saturating_sub(LABEL_WIDTH as u16 + 4) as usize;

    for (idx, field) in FIELDS.iter().enumerate() {
        let active = idx == app.field && app.picker.is_none() && !app.show_help;
        let label_style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            HINT
        };
        let marker = if active { "▌ " } else { "  " };
        let value_style = if active {
            Style::default().fg(Color::White)
        } else {
            HINT
        };
        let value = app.value(*field);

        if field.multiline() {
            let wrapped = wrap_field(value, width);
            let rows = wrapped.len().max(1).min(MULTILINE_ROWS as usize);
            for (line_idx, text) in wrapped.iter().take(rows).enumerate() {
                let label = if line_idx == 0 {
                    format!(" {:<LABEL_WIDTH$}", field.label())
                } else {
                    " ".repeat(LABEL_WIDTH + 1)
                };
                let leading = if line_idx == 0 {
                    Span::styled(marker, Style::default().fg(Color::Cyan))
                } else {
                    Span::styled("  ", Style::default().fg(Color::Cyan))
                };
                let mut spans = vec![Span::styled(label, label_style), leading];
                if line_idx == 0 && value.is_empty() {
                    spans.push(Span::styled(format!("  {}", field.hint()), MUTED));
                } else {
                    spans.push(Span::styled(text.clone(), value_style));
                }
                lines.push(Line::from(spans));

                if active && app.is_editing() {
                    if let Some((row, col)) = cursor_in_wrapped(&wrapped, app.cursor()) {
                        if row == line_idx {
                            cursor = Some((inner.x + LABEL_WIDTH as u16 + 2 + col as u16, row_y));
                        }
                    }
                }
                row_y += 1;
            }
            for _ in rows..MULTILINE_ROWS as usize {
                lines.push(Line::raw(""));
                row_y += 1;
            }
            continue;
        }

        let (display, cursor_col, hint) = if *field == Field::Type {
            (format!("[ {value} ]"), None, String::new())
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
            Span::styled(hint, MUTED),
        ]));

        if let Some(col) = cursor_col {
            cursor = Some((inner.x + LABEL_WIDTH as u16 + 2 + col as u16, row_y));
        }
        row_y += 1;
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(MUTED)
                .title(Span::styled(" compose ", HINT)),
        ),
        area,
    );

    cursor
}

/// Soft-wrap a field value to `width`.
fn wrap_field(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    let mut count = 0usize;

    for c in value.chars() {
        if c == '\n' {
            rows.push(std::mem::take(&mut row));
            count = 0;
            continue;
        }
        if count == width {
            rows.push(std::mem::take(&mut row));
            count = 0;
        }
        row.push(c);
        count += 1;
    }
    rows.push(row);
    rows
}

/// Map a char index to (row, column) inside wrapped rows.
fn cursor_in_wrapped(rows: &[String], cursor: usize) -> Option<(usize, usize)> {
    let mut remaining = cursor;
    for (row_idx, row) in rows.iter().enumerate() {
        let len = row.chars().count();
        if remaining <= len {
            return Some((row_idx, remaining.min(len.saturating_sub(1))));
        }
        remaining -= len;
    }
    rows.len().checked_sub(1).map(|row| (row, 0))
}

/// Cut a single-line value down to `width` characters around the cursor.
fn window(value: &str, cursor: usize, active: bool, width: usize) -> (String, Option<usize>) {
    let normalized: String = value.replace('\n', " ");
    let chars: Vec<char> = normalized.chars().collect();
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
            Line::styled("  start with a subject on the left", MUTED),
            Line::styled("  Ctrl+S hands the message to git", MUTED),
        ]);
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        .title(Span::styled(" preview ", HINT));

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
                Span::styled(format!("{:<8}", issue.field), MUTED),
                Span::styled(issue.message.clone(), HINT),
            ]));
        }
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        .title(Span::styled(title, HINT));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// Narrow or short terminal: only the active field, everything else in one line.
fn draw_compact(frame: &mut Frame, app: &App, area: Rect) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);

    let (errors, warnings) = validate::count(&app.issues);
    let step = format!("{}/{}", app.field + 1, FIELDS.len());
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        .title(Line::from(vec![
            Span::styled(" commit-tui ", Style::default().fg(Color::Cyan)),
            Span::styled(format!("{step} "), MUTED),
        ]))
        .title_top(
            Line::from(Span::styled(format!(" {errors}✗ {warnings}! "), MUTED)).right_aligned(),
        );

    let header_line = Line::from(vec![
        Span::styled(
            format!(" {} ", app.commit.kind),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!(
                "{} chars · {}",
                app.commit.subject.chars().count(),
                app.active_field().label()
            ),
            MUTED,
        ),
    ]);
    frame.render_widget(Paragraph::new(header_line).block(block), header);

    let inner_width = body.width.saturating_sub(4) as usize;
    let value = app.value(app.active_field());
    let mut lines = vec![Line::styled(
        format!("{}  {}", app.active_field().label(), app.field_hint()),
        MUTED,
    )];
    if app.active_field() == Field::Type {
        lines.push(Line::styled(
            format!("[ {} ]", value),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    } else if value.is_empty() {
        lines.push(Line::styled("(empty)", MUTED));
    } else {
        for row in wrap_field(value, inner_width.max(8)) {
            lines.push(Line::raw(row));
        }
    }
    lines.push(Line::styled(
        format!(
            "→ {}",
            app.commit.render().lines().next().unwrap_or_default()
        ),
        MUTED,
    ));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false }),
        body,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Tab ", KEY),
            Span::styled("next ", HINT),
            Span::styled(" Enter ", KEY),
            Span::styled("ok ", HINT),
            Span::styled(" C-s ", KEY),
            Span::styled("commit ", HINT),
            Span::styled(" ? ", KEY),
            Span::styled("help", HINT),
        ])),
        footer,
    );
}

/// Overlay list with a filter line. Pickers that carry previews (history) get a
/// pane on the right showing the highlighted entry in full.
fn draw_picker(frame: &mut Frame, picker: &Picker, area: Rect) {
    let visible = picker.filtered();
    let rows_wanted = visible.len() as u16 + 2;
    let has_preview = picker.kind == PickerKind::History;

    // With a preview the popup spans most of the terminal, otherwise it hugs
    // the list so the form behind it stays visible.
    let (width, height) = if has_preview {
        (
            area.width.saturating_sub(4).clamp(40, 116),
            area.height.saturating_sub(2).clamp(10, 26),
        )
    } else {
        (
            area.width.saturating_sub(6).clamp(24, 74),
            rows_wanted.min(area.height.saturating_sub(2)).max(4),
        )
    };
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    frame.render_widget(Clear, popup);

    let heading = match picker.kind {
        PickerKind::Type => "commit type",
        PickerKind::Scope => "scope",
        PickerKind::Profile => "profile",
        PickerKind::History => "recent commits",
    };
    let filter = if picker.filter.is_empty() {
        "_".to_string()
    } else {
        picker.filter.clone()
    };
    let title = if has_preview {
        format!(" {heading} · filter: {filter} · Enter reuses type, scope and body ")
    } else {
        format!(" {heading} · filter: {filter} ")
    };

    if has_preview && width >= 90 {
        let [list_area, preview_area] =
            Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
                .areas(popup);
        draw_list(frame, picker, &visible, list_area, title);
        draw_commit_preview(frame, picker, preview_area);
    } else {
        draw_list(frame, picker, &visible, popup, title);
    }
}

fn draw_list(frame: &mut Frame, picker: &Picker, visible: &[usize], area: Rect, title: String) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(title, Style::default().fg(Color::Cyan)));

    let inner_height = area.height.saturating_sub(2) as usize;
    let offset = if picker.selected >= inner_height && inner_height > 0 {
        picker.selected + 1 - inner_height
    } else {
        0
    };

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .skip(offset)
        .take(inner_height)
        .map(|(pos, idx)| {
            let selected = pos == picker.selected;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                HINT
            };
            let marker = if selected { "\u{25b8} " } else { "  " };
            let number = if picker.filter.is_empty() {
                format!("{}", if pos == 9 { 0 } else { pos + 1 })
            } else {
                " ".to_string()
            };
            let text = picker.items.get(*idx).cloned().unwrap_or_default();
            Row::new(vec![format!("{marker}{number} {text}")]).style(style)
        })
        .collect();

    let table = Table::new(rows, [Constraint::Fill(1)]).block(block);
    frame.render_widget(table, area);
}

/// The message of the highlighted commit, as it will look when reused.
fn draw_commit_preview(frame: &mut Frame, picker: &Picker, area: Rect) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        .title(Span::styled(" commit ", HINT));

    let text = match picker.preview() {
        Some(preview) if !preview.is_empty() => Text::from(
            preview
                .lines()
                .map(|line| {
                    if line.starts_with("M ") || line.starts_with("A ") || line.starts_with("D ") {
                        Line::styled(line.to_string(), MUTED)
                    } else {
                        Line::raw(line.to_string())
                    }
                })
                .collect::<Vec<_>>(),
        ),
        _ => Text::from(vec![Line::styled("  nothing selected", MUTED)]),
    };

    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let rows = [
        ("Tab / ↓ ↑", "next or previous field"),
        ("Enter", "next field or new line"),
        ("Space, Ctrl+T", "type picker: 1-9 jump, 0 last"),
        ("Ctrl+N", "scopes from config and history"),
        ("Ctrl+P", "switch type profile"),
        ("Ctrl+R", "reuse a recent commit's type, scope and body"),
        ("← →", "previous/next type"),
        ("Ctrl+S", "render the message and exit"),
        ("Esc, Ctrl+Q", "quit without output"),
        ("?", "this help"),
    ];

    let height = rows.len() as u16 + 2;
    let width = area.width.saturating_sub(4).clamp(30, 68);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);

    let lines: Vec<Line> = rows
        .iter()
        .map(|(key, text)| {
            Line::from(vec![
                Span::styled(format!(" {key:<14}"), Style::default().fg(Color::Cyan)),
                Span::styled(*text, HINT),
            ])
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(" keys ", Style::default().fg(Color::Cyan)));

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), popup);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let keys = Line::from(vec![
        Span::styled(" Tab ", KEY),
        Span::styled(" next  ", HINT),
        Span::styled(" Space ", KEY),
        Span::styled(" type  ", HINT),
        Span::styled(" C-n ", KEY),
        Span::styled(" scope  ", HINT),
        Span::styled(" C-p ", KEY),
        Span::styled(" profile  ", HINT),
        Span::styled(" C-r ", KEY),
        Span::styled(" reuse  ", HINT),
        Span::styled(" C-s ", KEY),
        Span::styled(" commit  ", HINT),
        Span::styled(" ? ", KEY),
        Span::styled(" help", HINT),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_keeps_the_cursor_visible() {
        let (display, col) = window("abcdefghij", 9, true, 5);
        assert_eq!(display, "fghij");
        assert_eq!(col, Some(4));
    }

    #[test]
    fn wrap_field_splits_on_newlines_and_width() {
        assert_eq!(wrap_field("abc\ndef", 10), vec!["abc", "def"]);
        assert_eq!(wrap_field("abcdef", 3), vec!["abc", "def"]);
    }

    #[test]
    fn cursor_maps_into_wrapped_rows() {
        assert_eq!(
            cursor_in_wrapped(&["abc".into(), "def".into()], 4),
            Some((1, 1))
        );
    }
}
