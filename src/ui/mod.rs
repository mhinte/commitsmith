//! The terminal surface: drawing, the event loop and the setup dialog.
//!
//! Nothing here touches `gix` or spawns a process; the state comes from
//! [`crate::application`] and is only read.

pub mod init;
pub mod setup;
pub mod wizard;

use std::io;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Row, Table, Wrap};
use unicode_width::UnicodeWidthChar;

use crate::app::{App, FIELDS, Field, Picker, PickerKind};
use crate::application::tabs::Tab;
use crate::validate::{self, Severity};

const LABEL_WIDTH: usize = 10;
/// Columns from the inner edge to the first character of a value: one leading
/// space, the padded label, then the `▌ ` marker. The caret is placed with this
/// offset, so a change to the label layout cannot shift it by a column again.
const VALUE_OFFSET: u16 = LABEL_WIDTH as u16 + 3;
/// Rows a multiline field may occupy in the form.
const MULTILINE_ROWS: u16 = 3;
/// Below this height the full form does not fit, so one field is shown at a time.
const COMPACT_HEIGHT: u16 = 24;
const COMPACT_WIDTH: u16 = 62;

const KEY: Style = Style::new().fg(Color::Black).bg(Color::DarkGray);

/// "1 error", "2 errors" — the counts sit in titles the user reads constantly.
fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}
const HINT: Style = Style::new().fg(Color::Gray);
const MUTED: Style = Style::new().fg(Color::DarkGray);

pub mod branch_wizard;
pub mod branches;
pub mod history;
pub mod staging;
pub mod theme;

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
        draw_help(frame, app, area);
    }
}

fn draw_full(frame: &mut Frame, app: &App, area: Rect) {
    // No separate header block: the tab bar is one line at the top and the state
    // of the message lives in the title of the compose panel.
    let [tab_bar, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .areas(area);

    draw_tab_bar(frame, app, tab_bar);

    let cursor = match app.tab {
        Tab::Commit => draw_commit_body(frame, app, body),
        Tab::Branches => {
            // The wizard sits beside the list, not on top of it: while it is
            // open the branches stay visible for reference.
            if app.wizard.open {
                let [list, wizard] = Layout::horizontal([
                    Constraint::Min(24),
                    Constraint::Length(body.width.clamp(0, 52).max(body.width / 2)),
                ])
                .areas(body);
                branches::draw(frame, app, list);
                branch_wizard::draw(frame, app, wizard)
            } else {
                branches::draw(frame, app, body);
                None
            }
        }
        Tab::History => {
            history::draw(frame, app, body);
            None
        }
    };
    draw_footer(frame, app, footer);

    if let Some((x, y)) = cursor {
        frame.set_cursor_position(Position::new(x, y));
    }
}

/// Staging, composer, preview and checks: the first tab.
fn draw_commit_body(frame: &mut Frame, app: &App, area: Rect) -> Option<(u16, u16)> {
    // The staging list sits left of the composer, as wide as its names need.
    let list_width = (area.width / 3).clamp(24, 40);
    let [list, right] =
        Layout::horizontal([Constraint::Length(list_width), Constraint::Min(30)]).areas(area);
    staging::draw(frame, app, list);
    // Body, breaking and footer all grow to MULTILINE_ROWS; counting only two of
    // them cut the footer's last rows off.
    let rows: u16 = FIELDS
        .iter()
        .filter(|field| app.field_visible(**field))
        .map(|field| if field.multiline() { MULTILINE_ROWS } else { 1 })
        .sum();
    let [form, content] =
        Layout::vertical([Constraint::Length(rows + 2), Constraint::Min(6)]).areas(right);

    let cursor = draw_form(frame, app, form);
    draw_content(frame, app, content);
    cursor
}

/// One line of tabs; the active one is highlighted, the others are muted.
fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = Vec::new();
    for (index, tab) in Tab::ALL.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" │ ", MUTED));
        }
        let active = tab == app.tab;
        let style = if active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            HINT
        };
        spans.push(Span::styled(
            format!(" [{}] {} ", tab.number(), tab.label()),
            style,
        ));
    }
    // The name and version belong in the top right corner, out of everyone's way.
    let version = if area.width >= 96 {
        format!("commitsmith {} ", env!("CARGO_PKG_VERSION"))
    } else {
        format!("v{} ", env!("CARGO_PKG_VERSION"))
    };
    let width = version.chars().count() as u16;
    let [tabs_area, version_area] =
        Layout::horizontal([Constraint::Min(20), Constraint::Length(width)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(spans)), tabs_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(version, MUTED))).right_aligned(),
        version_area,
    );
}

/// Something to look at on a tab that is not built yet.
#[allow(dead_code)]
fn draw_placeholder(frame: &mut Frame, area: Rect, title: &str, text: &str) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(Color::Gray),
        ));
    let mut lines = vec![Line::raw("")];
    lines.push(Line::styled(
        format!("  {text}"),
        Style::default().fg(Color::DarkGray),
    ));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "  1 back to the composer",
        Style::default().fg(Color::Gray),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Draws the form and returns the terminal cursor position, if any.
fn draw_form(frame: &mut Frame, app: &App, area: Rect) -> Option<(u16, u16)> {
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    let mut lines = Vec::new();
    let mut cursor = None;
    let mut subject_row = None;
    let mut row_y = inner.y;
    let width = area.width.saturating_sub(LABEL_WIDTH as u16 + 4) as usize;

    for (idx, field) in FIELDS.iter().enumerate() {
        // Folded fields keep their text and are still validated; they are just
        // not drawn.
        if !app.field_visible(*field) {
            continue;
        }
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
                // A multiline field is marked row by row, so the area reads as
                // one block instead of three loose lines.
                lines.push(if active {
                    Line::from(spans).style(theme::focused_row())
                } else {
                    Line::from(spans)
                });

                if active && app.is_editing() {
                    if let Some((row, col)) = cursor_in_wrapped(&wrapped, app.cursor()) {
                        if row == line_idx {
                            cursor = Some((inner.x + VALUE_OFFSET + col as u16, row_y));
                        }
                    }
                }
                row_y += 1;
            }
            for _ in rows..MULTILINE_ROWS as usize {
                lines.push(if active {
                    Line::raw("").style(theme::focused_row())
                } else {
                    Line::raw("")
                });
                row_y += 1;
            }
            continue;
        }

        let (display, cursor_col, hint) = if *field == Field::Type {
            (format!("[ {value} ]"), None, String::new())
        } else {
            // The subject row carries the counter on its right, so the text
            // has to stop short of it instead of running underneath.
            let room = if *field == Field::Subject {
                width.saturating_sub(8)
            } else {
                width
            };
            let (display, col) = window(value, app.cursor(), active && app.is_editing(), room);
            let hint = if value.is_empty() {
                format!("  {}", field.hint())
            } else {
                String::new()
            };
            (display, col, hint)
        };

        let row = Line::from(vec![
            Span::styled(format!(" {:<LABEL_WIDTH$}", field.label()), label_style),
            Span::styled(marker, Style::default().fg(Color::Cyan)),
            Span::styled(display, value_style),
            Span::styled(hint, MUTED),
        ]);
        // Same mark as the wizard and the lists: the focused row is filled, so
        // the eye finds it without reading the labels.
        lines.push(if active && !field.multiline() {
            row.style(theme::focused_row())
        } else {
            row
        });

        if let Some(col) = cursor_col {
            cursor = Some((inner.x + VALUE_OFFSET + col as u16, row_y));
        }
        if *field == Field::Subject && subject_row.is_none() {
            subject_row = Some(row_y);
        }
        row_y += 1;
    }

    // The compose panel has the keyboard, so it carries the same bright border
    // as the wizard on tab 2; preview and checks stay quiet.
    // Only the side that has the keyboard is lit; `Ctrl+L` moves that mark with
    // the focus.
    let compose_focused = app.staging.side == crate::application::staging::Side::Composer;
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if compose_focused {
            theme::focused_panel()
        } else {
            theme::idle_panel()
        })
        .title(Line::from(Span::styled(
            " compose ",
            if compose_focused {
                theme::focused_panel()
            } else {
                theme::idle_title()
            },
        )));
    if area.width >= 46 {
        block = block.title_top(
            Line::from(Span::styled(format!("{}  ", app.type_label()), HINT)).right_aligned(),
        );
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), area);

    // The subject counter belongs to its field, not in a title far above it.
    if let Some(row) = subject_row {
        let (length, max) = subject_len(app);
        let color = if length > max {
            Color::Red
        } else if length > app.rules.recommended_subject {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("{length}/{max} "),
                Style::default().fg(color),
            )))
            .right_aligned(),
            Rect {
                x: area.x + 1,
                y: row,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    }

    cursor
}

/// Characters in the subject, and the limit they are measured against.
fn subject_len(app: &App) -> (usize, usize) {
    (app.commit.subject.chars().count(), app.rules.max_subject)
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
        let w = cells(c);
        if count + w > width {
            rows.push(std::mem::take(&mut row));
            count = 0;
        }
        row.push(c);
        count += w;
    }
    rows.push(row);
    rows
}

/// Map a char index to (row, column) inside wrapped rows.
/// Row and column (in screen cells) of a character offset inside wrapped rows.
fn cursor_in_wrapped(rows: &[String], cursor: usize) -> Option<(usize, usize)> {
    let mut remaining = cursor;
    for (row_idx, row) in rows.iter().enumerate() {
        let len = row.chars().count();
        if remaining <= len {
            let col = text_cells(&row.chars().take(remaining).collect::<String>());
            return Some((row_idx, col));
        }
        remaining -= len;
    }
    rows.len()
        .checked_sub(1)
        .map(|row| (row, text_cells(&rows[row])))
}

/// Cut a single-line value down to `width` characters around the cursor.
/// Columns a character occupies on screen. Terminals draw wide characters
/// (CJK, emoji) in two cells, so counting characters is not enough for the
/// cursor position.
fn cells(c: char) -> usize {
    c.width().unwrap_or(0)
}

fn text_cells(text: &str) -> usize {
    text.chars().map(cells).sum()
}

/// Cut a string to at most `width` columns, keeping whole characters.
fn take_cells(text: &str, width: usize) -> (String, usize) {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = cells(c);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    (out, used)
}

/// Visible part of a single-line field plus the cursor column inside it.
fn window(value: &str, cursor: usize, active: bool, width: usize) -> (String, Option<usize>) {
    let normalized: String = value.replace('\n', " ");
    let chars: Vec<char> = normalized.chars().collect();
    let width = width.max(1);

    // Columns left of the cursor decide the scroll offset.
    let before: usize = chars
        .iter()
        .take(cursor.min(chars.len()))
        .map(|c| cells(*c))
        .sum();
    // Scroll just far enough that the cursor stays inside the field. The bounds
    // check has to come first: an empty field has no character to look at, and
    // indexing it panicked the whole program on startup.
    let keep_visible = width.saturating_sub(1);
    let mut start_char = 0;
    let mut skipped = 0;
    while before.saturating_sub(skipped) > keep_visible && start_char < cursor {
        let Some(c) = chars.get(start_char) else {
            break;
        };
        skipped += cells(*c);
        start_char += 1;
    }

    let rest: String = chars[start_char..].iter().collect();
    let (display, _) = take_cells(&rest, width);

    if active {
        let col = (before.saturating_sub(skipped)).min(width.saturating_sub(1));
        (display, Some(col))
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
            Line::styled(
                if app.commit_mode {
                    "  Ctrl+S runs git commit with this message"
                } else {
                    "  Ctrl+S prints the message and exits (start with -c to commit)"
                },
                MUTED,
            ),
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
    let title = format!(
        " checks · {} · {} ",
        plural(errors, "error"),
        plural(warnings, "warning")
    );

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
        // No program name here any more: it lives in the corner.
        .title(Line::from(Span::styled(
            format!(" {step} "),
            Style::default().fg(Color::Cyan),
        )))
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

    // The name and version belong in the corner here too, but the narrow form
    // has to fit beside the keys.
    let version = format!("v{} ", env!("CARGO_PKG_VERSION"));
    let version_width = if footer.width >= 54 {
        version.chars().count() as u16
    } else {
        0
    };
    let [keys_area, version_area] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(version_width)]).areas(footer);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Tab ", KEY),
            Span::styled("next ", HINT),
            Span::styled(" Enter ", KEY),
            Span::styled("ok ", HINT),
            Span::styled(" C-k ", KEY),
            Span::styled("clear ", HINT),
            Span::styled(" C-s ", KEY),
            Span::styled(
                if app.commit_mode {
                    "commit "
                } else {
                    "message "
                },
                HINT,
            ),
            Span::styled(" ? ", KEY),
            Span::styled("help", HINT),
        ])),
        keys_area,
    );
    if version_width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(version, MUTED))).right_aligned(),
            version_area,
        );
    }
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

fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    // Only what this tab can do, then the keys that work everywhere. A shortcut
    // that does nothing here would be worse than no help at all.
    let mut rows = app.tab.help(app.commit_mode);
    rows.push(("", ""));
    rows.extend(Tab::global_help());

    let height = rows.len() as u16 + 2;
    let width = area.width.saturating_sub(4).clamp(30, 72);
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
            if key.is_empty() && text.is_empty() {
                return Line::raw("");
            }
            if key.is_empty() {
                // A note about the tab, not a shortcut.
                return Line::styled(format!(" {text}"), MUTED);
            }
            Line::from(vec![
                Span::styled(format!(" {key:<16}"), Style::default().fg(Color::Cyan)),
                Span::styled(*text, HINT),
            ])
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            format!(" keys · {} ", app.tab.label()),
            Style::default().fg(Color::Cyan),
        ));

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), popup);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    // The active tab decides which keys are advertised: a shortcut that does
    // nothing here would be a lie in the footer. The last two entries are the
    // global ones (`1-3`, `?`) and stay, the middle ones are dropped when the
    // line gets too narrow.
    let keys = app.tab.keys(app.commit_mode);
    let globals = keys.len().saturating_sub(2);
    let primary = &keys[..4.min(globals)];
    let secondary = &keys[4.min(globals)..globals];
    let essential_count = primary.len() + (keys.len() - globals);
    let essentials: Vec<(&str, &str)> = primary
        .iter()
        .copied()
        .chain(keys[globals..].iter().copied())
        .collect();
    let optional: Vec<(&str, &str)> = secondary.to_vec();
    let _ = essential_count;

    let measure = |entries: &[(&str, &str)]| -> usize {
        entries
            .iter()
            .enumerate()
            .map(|(index, (key, label))| {
                let own = key.chars().count() + label.chars().count() + 3;
                if index == 0 { own } else { own + 1 }
            })
            .sum()
    };

    // Insert the optional hints after "Tab" while they fit, most useful first.
    let select = |budget: usize| -> Vec<(&str, &str)> {
        let mut entries: Vec<(&str, &str)> = essentials.to_vec();
        for take in 1..=optional.len() {
            let mut candidate = essentials.to_vec();
            for (index, extra) in optional.iter().take(take).enumerate() {
                candidate.insert(1 + index, *extra);
            }
            if measure(&candidate) <= budget {
                entries = candidate;
            } else {
                break;
            }
        }
        entries
    };

    // The corner shows the name and the version. If the essentials cannot fit
    // beside the name, the number alone has to do — but the hints win.
    // The context takes the right end of this row; the keys only get what is left.
    let context = branch_context(app, area.width);
    let context_width = unicode_width::UnicodeWidthStr::width(context.as_str()) as u16;
    let entries = select(area.width.saturating_sub(context_width + 1) as usize);

    let mut spans = Vec::new();
    for (index, (key, label)) in entries.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!(" {key} "), KEY));
        spans.push(Span::styled(format!(" {label}"), HINT));
    }
    // The branch context moved up here, where the version used to sit: the same
    // row as the keys, the same place the eye looks for it.
    let [keys_area, context_area] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(context_width)])
            .areas(Rect { height: 1, ..area });

    frame.render_widget(Paragraph::new(Line::from(spans)), keys_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            context,
            if app.in_repository {
                MUTED
            } else {
                Style::default().fg(Color::Yellow)
            },
        )))
        .right_aligned(),
        context_area,
    );

    let second_row = Rect {
        y: area.y + 1,
        height: 1,
        ..area
    };

    // Branch and profile are context, not decisions: they sit at the far right
    // of the second row, out of the way of the message.
    // Branch, sync state and profile: context, not decisions. The short `↑n ↓n`
    // form keeps its room even when the footer is busy.
    // The staging list is narrow, so the footer names the file under the cursor in
    // full. The status keeps its place: the path is shortened first.
    let room = second_row.width.saturating_sub(2) as usize;
    let mut left = Vec::new();
    let mut used = 0;
    if let Some(path) = highlighted_file(app) {
        let status_width = if app.status.is_empty() {
            0
        } else {
            unicode_width::UnicodeWidthStr::width(app.status.as_str()) + 2
        };
        let shown = fit_left(&path, room.saturating_sub(status_width).max(12));
        used = unicode_width::UnicodeWidthStr::width(shown.as_str()) + 2;
        left.push(Span::styled(format!(" {shown}  "), HINT));
    }
    if !app.status.is_empty() {
        // The status must not run under the context: it is cut, not overwritten.
        let status = fit(&app.status, room.saturating_sub(used));
        left.push(Span::styled(status, Style::default().fg(Color::Cyan)));
    }
    if !left.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(left)),
            Rect {
                width: (room as u16).saturating_add(1),
                ..second_row
            },
        );
    }
}

/// Branch, sync state and profile: context, not decisions.
fn branch_context(app: &App, width: u16) -> String {
    // A narrow footer cannot carry branch, sync state and profile without eating
    // the keys; the profile is the part that stays.
    if width < 100 {
        return format!("{} ", app.config.profile);
    }
    if !app.in_repository {
        return format!("no git repository · {} ", app.config.profile);
    }
    let mut parts: Vec<String> = Vec::new();
    if !app.repo_line.is_empty() {
        parts.push(app.repo_line.clone());
    }
    if let Some(sync) = app.sync_label() {
        parts.push(sync);
    }
    parts.push(app.config.profile.clone());
    format!("{} ", parts.join(" · "))
}

/// The file the staging cursor sits on, if this tab shows one.
fn highlighted_file(app: &App) -> Option<String> {
    (app.tab == crate::application::tabs::Tab::Commit)
        .then(|| app.staging.selected_path())
        .flatten()
}

/// Cut `text` from the left to `width` cells: the end of a path is the useful part.
fn fit_left(text: &str, width: usize) -> String {
    if unicode_width::UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut used = 0;
    for c in chars.iter().rev() {
        let cell = unicode_width::UnicodeWidthChar::width(*c).unwrap_or(1);
        if used + cell + 1 > width {
            break;
        }
        used += cell;
        out.insert(0, *c);
    }
    format!("…{out}")
}

/// Cut `text` to `width` display cells, with an ellipsis when it was longer.
fn fit(text: &str, width: usize) -> String {
    if unicode_width::UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cell = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if used + cell + 1 > width {
            break;
        }
        used += cell;
        out.push(c);
    }
    out.push('…');
    out
}

/// Run the TUI until the user quits or hands the message over.
pub fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    // A terminal that reports no size draws nothing at all, which looks exactly
    // like a program that did not start. Say so instead of showing a blank screen.
    let size = terminal.size()?;
    if size.width == 0 || size.height == 0 {
        return Err(io::Error::other(
            "the terminal reports a size of 0x0 · resize the window or run it in a real console",
        ));
    }

    loop {
        terminal.draw(|frame| draw(frame, app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                // A pasted block of text goes into the active field, line
                // breaks included, instead of being replayed as key presses.
                Event::Paste(text) => app.handle_paste(&text),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn window_keeps_the_cursor_visible() {
        let (display, col) = window("abcdefghij", 9, true, 5);
        assert_eq!(display, "fghij");
        assert_eq!(col, Some(4));
    }

    #[test]
    fn an_empty_field_has_a_cursor_but_nothing_to_scroll() {
        // 0.3.1 panicked here on startup: every empty field indexed chars[0].
        assert_eq!(window("", 0, true, 40), (String::new(), Some(0)));
        assert_eq!(window("", 0, false, 40), (String::new(), None));
        // A field narrower than one cell must not divide by zero either.
        assert_eq!(window("", 0, true, 0), (String::new(), Some(0)));
    }

    #[test]
    fn the_longest_field_still_shows_the_cursor() {
        let text = "x".repeat(200);
        let (display, col) = window(&text, 200, true, 20);
        // The cursor needs a cell of its own, so 19 characters plus the cursor
        // fill the 20 columns; nothing may overflow the field.
        let col = col.expect("cursor column");
        assert_eq!(col, text_cells(&display));
        assert!(text_cells(&display) < 20);
        assert!(display.ends_with("xxx"));
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn press_mod(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.handle_key(KeyEvent::new(code, modifiers));
    }

    /// Position of the first cell of `needle` in the rendered buffer.
    fn find(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
        needle: &str,
    ) -> Option<(u16, u16)> {
        let buffer = terminal.backend().buffer();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if let Some(byte) = row.find(needle) {
                let column = row[..byte].chars().count() as u16;
                return Some((column, y));
            }
        }
        None
    }

    #[test]
    fn the_caret_sits_after_the_last_character() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        app.handle_paste("new button");

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let (text_x, _) = find(&terminal, "new button").expect("subject row");
        let caret = terminal.backend().cursor_position();
        // The caret belongs one cell behind the last character, not on it.
        assert_eq!(
            caret.x,
            text_x + "new button".len() as u16,
            "caret {caret:?}"
        );
    }

    #[test]
    fn the_caret_starts_at_the_first_value_column_when_the_field_is_empty() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let (marker_x, _) = find(&terminal, "▌").expect("active marker");
        let caret = terminal.backend().cursor_position();
        // Marker plus its trailing space, then the (empty) value.
        assert_eq!(
            caret.x,
            marker_x + 2,
            "caret {caret:?}, marker at {marker_x}"
        );
    }

    /// The whole screen as text, one string per row.
    fn rows(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_warning_outside_a_repository_sits_in_the_footer() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.set_in_repository(false);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(
            screen.contains("no git repository · conventional"),
            "{screen}"
        );
        assert!(
            screen.contains("not a git repository · `check` works here"),
            "{screen}"
        );

        app.set_in_repository(true);
        app.set_repo_line("main · 0 staged".to_string());
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(!screen.contains("no git repository"), "{screen}");
        assert!(
            screen.contains("main · 0 staged · conventional"),
            "{screen}"
        );
    }

    #[test]
    fn the_title_is_plain_like_the_other_panels() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.set_in_repository(true);
        app.set_repo_line("main · 1 staged".to_string());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal);
        let title = &screen
            .iter()
            .find(|row| row.contains("compose"))
            .cloned()
            .expect("compose panel");
        // Same shape as " preview " and " checks ": the name, nothing else.
        assert!(
            title.contains("╭ compose ─") || title.contains("┬ compose ─"),
            "{title}"
        );
        // The type appears once, as the description at the other end.
        assert!(title.contains("a new feature"), "{title}");
        for gone in [
            "error",
            "warning",
            "· conventional",
            "main · 1 staged",
            "0/72",
        ] {
            assert!(
                !title.contains(gone),
                "{gone:?} still in the title: {title}"
            );
        }
    }

    #[test]
    fn the_subject_counter_sits_next_to_the_subject() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.commit.subject = "add the thing".to_string();
        app.revalidate();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal);
        let subject_row = screen
            .iter()
            .position(|line| line.contains("subject"))
            .expect("subject row");
        let counter_row = screen
            .iter()
            .position(|line| line.contains("/72"))
            .expect("counter");
        assert_eq!(counter_row, subject_row, "{screen:#?}");
        let column = screen[counter_row].find("/72").unwrap();
        assert!(column > 60, "right end of the field: {column}");
        assert!(screen[subject_row].contains("add the thing"), "{screen:#?}");
    }

    #[test]
    fn errors_are_counted_only_in_the_checks_panel() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = App::with_config(crate::config::Config::conventional(), Vec::new());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal);
        let mentioning: Vec<&String> = screen
            .iter()
            .filter(|line| line.contains("error") || line.contains("warning"))
            .collect();
        assert_eq!(mentioning.len(), 1, "{screen:#?}");
        assert!(mentioning[0].contains("checks"), "{}", mentioning[0]);
    }

    fn branch_app() -> App {
        use crate::application::test_support::RecordingGit;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.set_in_repository(true);
        let git = RecordingGit::new(&["main", "feature/x"], "main");
        git.branches.borrow_mut().push({
            let mut remote = commitsmith::domain::ports::BranchInfo::remote("origin/main");
            remote.upstream = Some("origin/main".to_string());
            remote
        });
        app.set_git(Box::new(git));
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        app
    }

    #[test]
    fn the_branch_tab_lists_branches_with_the_head_marker() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = branch_app();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");

        assert!(screen.contains(" branches · 3 "), "{screen}");
        assert!(screen.contains("● main"), "HEAD marker: {screen}");
        assert!(screen.contains("○ feature/x"), "{screen}");
        assert!(screen.contains("remotes"), "remote heading: {screen}");
        assert!(screen.contains("origin/main"), "{screen}");
        assert!(screen.contains("▸ "), "selection marker: {screen}");
    }

    #[test]
    fn the_branch_tab_shows_a_confirmation_before_deleting() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = branch_app();
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("press d again"), "{screen}");
        assert!(screen.contains("feature/x"), "{screen}");
    }

    #[test]
    fn the_branch_tab_says_so_outside_a_repository() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("no repository"), "{screen}");
    }

    #[test]
    fn the_help_shows_the_keys_of_the_active_tab() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // Tab 1: the composer's keys, not the explorer's actions.
        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("keys · Commit & Stage"), "{screen}");
        assert!(screen.contains("type picker"), "{screen}");
        assert!(!screen.contains("check out"), "{screen}");

        // Tab 2: the explorer's keys, and no commit shortcut.
        let mut app = branch_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("keys · Branches"), "{screen}");
        assert!(screen.contains("check out the selected branch"), "{screen}");
        assert!(screen.contains("merge the selected branch"), "{screen}");
        assert!(!screen.contains("type picker"), "{screen}");
        assert!(!screen.contains("Ctrl+S"), "{screen}");
    }

    #[test]
    fn every_field_and_the_whole_footer_fit() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.commit.footer = "Refs TICKET-42\nCo-authored-by: hima".to_string();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");

        for label in ["type", "scope", "subject", "body", "breaking", "footer"] {
            assert!(screen.contains(label), "{label} missing: {screen}");
        }
        assert!(
            screen.contains("Co-authored-by: hima"),
            "the second footer line must be visible: {screen}"
        );
    }

    #[test]
    fn the_wizard_sits_beside_the_branch_list() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = branch_app();
        app.config.author = Some("hima".to_string());
        app.config.ticket_prefix = "TICKET".to_string();
        app.config.branch_template = "{type}/{author}/{ticket}/{slug}".to_string();
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");

        assert!(screen.contains("new branch"), "{screen}");
        assert!(
            screen.contains("branches · "),
            "the list stays visible: {screen}"
        );
        assert!(screen.contains("feat/hima/<description>"), "{screen}");
        assert!(screen.contains("description"), "{screen}");
    }

    /// Style of the first cell of the row containing `needle`.
    fn row_style(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
        needle: &str,
    ) -> ratatui::style::Style {
        let buffer = terminal.backend().buffer();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if row.contains(needle) {
                // Any cell of the row carries the fill; the panels start at
                // different columns now.
                return (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].style())
                    .find(|style| {
                        style.bg.is_some() && style.bg != Some(ratatui::style::Color::Reset)
                    })
                    .unwrap_or_else(|| buffer[(0, y)].style());
            }
        }
        panic!("no row with {needle:?}");
    }

    #[test]
    fn the_compose_panel_has_the_bright_border() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = App::with_config(crate::config::Config::conventional(), Vec::new());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();

        let screen = rows(&terminal);
        let cyan_in = |y: u16| {
            (0..buffer.area.width).any(|x| buffer[(x, y)].fg == ratatui::style::Color::Cyan)
        };
        let compose_row = screen
            .iter()
            .position(|row| row.contains("compose"))
            .expect("compose panel");
        assert!(cyan_in(compose_row as u16), "the compose panel is lit");
        let preview_row = screen
            .iter()
            .position(|row| row.contains("preview"))
            .expect("preview panel");
        assert!(!cyan_in(preview_row as u16), "preview stays quiet");
    }

    #[test]
    fn tab_one_lists_the_changed_files_left_of_the_composer() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (app, _) = staging_app_in_ui();
        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("staging · 1"), "{screen}");
        assert!(screen.contains("a.txt"), "{screen}");
        assert!(screen.contains("compose"), "the composer stays: {screen}");
    }

    fn staging_app_in_ui() -> (App, ()) {
        use crate::application::test_support::RecordingGit;
        use commitsmith::domain::ports::{ChangedFile, FileState};

        let git = RecordingGit::new(&["main"], "main");
        git.files.borrow_mut().push(ChangedFile {
            path: "a.txt".to_string(),
            index: None,
            worktree: Some(FileState::Modified),
        });
        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        (app, ())
    }

    #[test]
    fn the_focused_field_is_filled_in_the_composer() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        press(&mut app, KeyCode::Tab); // scope
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();

        let focused = row_style(&terminal, "scope");
        let idle = row_style(&terminal, "subject");
        assert_eq!(focused.bg, Some(theme::FOCUS_BG), "scope has the focus");
        assert_ne!(idle.bg, Some(theme::FOCUS_BG), "subject does not");

        press(&mut app, KeyCode::Down);
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert_eq!(row_style(&terminal, "subject").bg, Some(theme::FOCUS_BG));
        assert_ne!(row_style(&terminal, "scope").bg, Some(theme::FOCUS_BG));
    }

    #[test]
    fn the_focused_wizard_field_is_filled_and_its_panel_bright() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = branch_app();
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();

        assert_eq!(
            row_style(&terminal, "│ type").bg,
            Some(theme::FOCUS_BG),
            "the first wizard field is focused"
        );
        assert_ne!(row_style(&terminal, "ticket").bg, Some(theme::FOCUS_BG));

        // The panel border carries the same cyan as a focused row's label.
        let buffer = terminal.backend().buffer();
        let border = (0..buffer.area.height)
            .map(|y| buffer[(56, y)].style())
            .find(|style| style.fg == Some(ratatui::style::Color::Cyan));
        assert!(border.is_some(), "the wizard panel has the bright border");
    }

    #[test]
    fn the_selected_branch_is_filled_too() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = branch_app();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert_eq!(row_style(&terminal, "● main").bg, Some(theme::FOCUS_BG));
        assert_ne!(
            row_style(&terminal, "○ feature/x").bg,
            Some(theme::FOCUS_BG)
        );

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert_eq!(
            row_style(&terminal, "○ feature/x").bg,
            Some(theme::FOCUS_BG)
        );
    }

    #[test]
    fn the_wizard_calls_the_field_type_and_brackets_the_value() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = branch_app();
        app.config.author = Some("hima".to_string());
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");

        assert!(screen.contains("type"), "same word as on tab 1: {screen}");
        assert!(
            screen.contains("[ feat ]"),
            "bracketed like tab 1: {screen}"
        );
        assert!(!screen.contains("kind"), "{screen}");
    }

    #[test]
    fn the_wizard_previews_the_name_as_it_is_typed() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = branch_app();
        app.config.author = Some("hima".to_string());
        app.config.branch_template = "{type}/{author}/{slug}".to_string();
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for c in "Add a thing".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }

        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("feat/hima/add-a-thing"), "{screen}");
    }

    #[test]
    fn the_branch_tab_advertises_its_own_keys() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = branch_app();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        for hint in ["checkout", "delete", "merge"] {
            assert!(screen.contains(hint), "{hint} missing: {screen}");
        }
        assert!(
            !screen.contains("C-s"),
            "the composer keys do not belong here"
        );
    }

    #[test]
    fn a_long_status_does_not_run_under_the_context() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.set_in_repository(true);
        app.set_repo_line("feat/hima/TICKET-42/add-a-thing · 0 staged".to_string());
        app.status = "created and checked out feat/hima/TICKET-42/add-a-thing · now on the branch, working tree clean, nothing staged".to_string();

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal);
        // The context moved to the keys row; it must survive there in full.
        assert!(
            screen[28].contains("feat/hima/TICKET-42/add-a-thing · 0 staged · conventional"),
            "the context stays whole: {:?}",
            screen[28]
        );
        // The status has the second row to itself and is cut, not dropped.
        let status = &screen[29];
        assert!(status.contains("created"), "{status:?}");
        assert!(status.contains('…'), "cut, not overwritten: {status:?}");
    }

    #[test]
    fn the_tab_bar_uses_bracketed_numbers() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = App::with_config(crate::config::Config::conventional(), Vec::new());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let top = rows(&terminal).remove(0);
        assert!(top.contains("[1] Commit"), "{top}");
        assert!(top.contains("[2] Branches"), "{top}");
        assert!(top.contains("[3] Historie"), "{top}");
    }

    #[test]
    fn the_version_sits_in_the_header() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = App::with_config(crate::config::Config::conventional(), Vec::new());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal);
        let version = format!("commitsmith {}", env!("CARGO_PKG_VERSION"));
        let row = screen
            .iter()
            .position(|line| line.contains(&version))
            .expect("version in the footer");
        assert_eq!(row, 0, "the header row: {screen:#?}");
        let column = screen[row].find(&version).unwrap() as u16;
        assert!(column > 60, "top right, not left: {column}");
    }

    #[test]
    fn the_footer_fits_eighty_columns() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        for width in [80u16, 100, 120, 160] {
            let app = App::with_config(crate::config::Config::conventional(), Vec::new());
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal.draw(|frame| draw(frame, &app)).unwrap();

            let buffer = terminal.backend().buffer();
            let rows: Vec<String> = (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect();
            let footer = rows
                .iter()
                .find(|row| row.contains("C-k"))
                .unwrap_or_else(|| panic!("no footer at width {width}: {rows:?}"));
            // The essentials have to be readable at every width, including the
            // last entry, which used to be cut off at 80 columns.
            for needed in ["Tab", "next", "C-k", "clear", "C-s", "?", "help"] {
                assert!(
                    footer.contains(needed),
                    "{needed:?} missing at {width}: {footer:?}"
                );
            }
            // The version moved to the header; the footer must not repeat it.
            assert!(
                !footer.contains("commitsmith"),
                "no version in the footer at {width}: {footer:?}"
            );
            // The context takes the right end of this row, so the optional hints
            // start later than they used to.
            if width >= 120 {
                assert!(
                    footer.contains("type"),
                    "a 120 column footer should reach the type hint: {footer:?}"
                );
            }
        }
    }

    #[test]
    fn the_composer_draws_at_any_terminal_size() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = App::with_config(crate::config::Config::conventional(), Vec::new());
        // Empty fields, then a filled one with the cursor at the end.
        for size in [(100, 30), (80, 24), (62, 24), (61, 23), (40, 12), (20, 6)] {
            let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
            terminal
                .draw(|frame| draw(frame, &app))
                .expect("empty composer");
        }

        let mut filled = App::with_config(crate::config::Config::conventional(), Vec::new());
        filled.handle_paste("scope with a rather long value that needs scrolling");
        filled.handle_key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Tab,
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        for size in [(100, 30), (80, 24), (40, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
            terminal
                .draw(|frame| draw(frame, &filled))
                .expect("filled composer");
        }
    }

    #[test]
    fn wide_characters_occupy_two_columns() {
        assert_eq!(cells('a'), 1);
        assert_eq!(cells('我'), 2);
        assert_eq!(text_cells("我我a"), 5);
        let (kept, used) = take_cells("我我我", 4);
        assert_eq!(kept, "我我");
        assert_eq!(used, 4);
    }

    #[test]
    fn cursor_column_counts_cells_not_characters() {
        // Cursor after two CJK characters sits at column 4, not 2.
        let (display, col) = window("我我ab", 2, true, 20);
        assert_eq!(display, "我我ab");
        assert_eq!(col, Some(4));
    }

    #[test]
    fn scrolling_uses_cells() {
        // Ten wide characters in an eight column field: the visible window
        // starts at a character boundary and the cursor stays inside.
        let (display, col) = window("我我我我我我我我我我", 10, true, 8);
        assert!(text_cells(&display) <= 8, "{display:?}");
        assert!(col.unwrap() < 8);
    }

    #[test]
    fn wrap_field_splits_on_newlines_and_width() {
        assert_eq!(wrap_field("abc\ndef", 10), vec!["abc", "def"]);
        assert_eq!(wrap_field("abcdef", 3), vec!["abc", "def"]);
    }

    /// Style of the cell at the first character of `needle`.
    fn style_of(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
        needle: &str,
    ) -> Style {
        let (x, y) = find(terminal, needle).expect("text on screen");
        // Only the colour matters; the buffer fills in explicit defaults that a
        // hand-built style leaves unset.
        Style::new().fg(terminal.backend().buffer()[(x, y)]
            .style()
            .fg
            .unwrap_or(Color::Reset))
    }

    fn staging_item(path: &str) -> commitsmith::domain::ports::ChangedFile {
        commitsmith::domain::ports::ChangedFile {
            path: path.to_string(),
            index: None,
            worktree: Some(commitsmith::domain::ports::FileState::Modified),
        }
    }

    #[test]
    fn the_bright_border_follows_the_keyboard_to_the_file_list() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.staging.items = vec![staging_item("src/main.rs")];

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let compose_before = style_of(&terminal, "compose");
        let staging_before = style_of(&terminal, "staging");

        press_mod(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let compose_after = style_of(&terminal, "compose");
        let staging_after = style_of(&terminal, "staging");

        // Before: the composer is lit and the list is not. After `Ctrl+L` the two
        // styles have swapped — the lit border moved with the keyboard.
        assert_eq!(compose_before, theme::focused_panel());
        assert_eq!(staging_before, theme::idle_title());
        assert_eq!(staging_after, theme::focused_panel());
        assert_eq!(compose_after, theme::idle_title());
    }

    #[test]
    fn the_file_list_scrolls_to_keep_the_cursor_visible() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.staging.items = (0..30)
            .map(|index| staging_item(&format!("src/file{index:02}.rs")))
            .collect();
        press_mod(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
        // There is no repository here, so the list cannot load a diff; the cursor
        // position is what the renderer has to follow.
        app.staging.selected = 25;

        // The diff halves the list; the cursor must stay inside that half.
        app.staging.show_diff = true;
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("file25.rs"), "the selected file is visible");
        assert!(
            !screen.contains("file00.rs"),
            "the list scrolled instead of clipping the cursor"
        );
        assert!(screen.contains("▌"), "the cursor marker is on screen");
    }

    #[test]
    fn a_long_path_is_shortened_but_named_in_full_in_the_diff() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let path = "src/infrastructure/very/long/directory/chain/file_name.rs";
        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.staging.items = vec![staging_item(path)];
        app.staging.show_diff = true;
        app.staging.diff = String::new();

        // Narrow enough that the full path cannot fit next to the status code.
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");

        assert!(screen.contains("file_name.rs"), "the file name survives");
        // The list shortens; only the footer names the file in full.
        assert_eq!(
            screen.matches(path).count(),
            1,
            "the whole path appears once, in the footer"
        );
        // The diff title names the file in full, so nothing is lost.
        assert!(screen.contains("file_name.rs"), "{screen}");
    }

    fn past(kind: &str, scope: &str, summary: &str) -> commitsmith::domain::history::PastCommit {
        commitsmith::domain::history::PastCommit {
            short_id: "abc1234".to_string(),
            id: "abc1234".to_string(),
            author: "Marc".to_string(),
            date: "2026-11-25".to_string(),
            summary: summary.to_string(),
            body: String::new(),
            kind: Some(kind.to_string()),
            scope: Some(scope.to_string()),
            files: Vec::new(),
        }
    }

    #[test]
    fn the_log_shows_what_the_filter_keeps() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.tab = crate::application::tabs::Tab::History;
        app.set_history(vec![
            past("feat", "ui", "add the button"),
            past("fix", "api", "repair the route"),
        ]);
        app.log.filter.kind = Some("feat".to_string());

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("add the button"), "{screen}");
        assert!(!screen.contains("repair the route"), "{screen}");
        assert!(screen.contains("1 of 2"), "{screen}");
    }

    #[test]
    fn help_opens_on_the_third_tab() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.tab = crate::application::tabs::Tab::History;
        app.set_history(vec![past("feat", "ui", "add the button")]);

        press(&mut app, KeyCode::Char('?'));
        assert!(app.show_help, "the help must open here too");
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("filter"), "{screen}");
        assert!(screen.contains("clear filter"), "{screen}");

        // Esc closes it again.
        press(&mut app, KeyCode::Esc);
        assert!(!app.show_help);
    }

    #[test]
    fn the_version_sits_in_the_header_and_the_branch_in_the_footer() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.set_in_repository(true);
        app.set_repo_line("main".to_string());

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let lines = rows(&terminal);
        assert!(lines[0].contains("commitsmith"), "header: {}", lines[0]);
        let footer = lines
            .iter()
            .rev()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(footer.contains("main"), "context moved down: {footer}");
        assert!(
            !footer.contains("commitsmith"),
            "no double version: {footer}"
        );
    }

    #[test]
    fn the_staging_title_counts_what_is_staged() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.staging.items = vec![
            commitsmith::domain::ports::ChangedFile {
                path: "src/a.rs".to_string(),
                index: Some(commitsmith::domain::ports::FileState::Modified),
                worktree: None,
            },
            commitsmith::domain::ports::ChangedFile {
                path: "src/b.rs".to_string(),
                index: None,
                worktree: Some(commitsmith::domain::ports::FileState::Modified),
            },
        ];

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("staging · 2 · 1 staged"), "{screen}");
    }

    #[test]
    fn a_clean_commit_is_green_and_optional_hints_are_not_repeated() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.tab = crate::application::tabs::Tab::History;
        app.set_history(vec![past("feat", "ui", "add the button")]);

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("lint: clean"), "{screen}");
        assert!(!screen.contains("optional:"), "no hints here: {screen}");
    }

    #[test]
    fn the_detail_pane_names_what_the_linter_objects_to() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.tab = crate::application::tabs::Tab::History;
        // A subject that starts with a capital and ends with a period: two
        // findings the composer would also show.
        app.set_history(vec![past("feat", "ui", "Add the button.")]);

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("subject:"), "{screen}");
        assert!(
            screen.contains("period") || screen.contains("imperative"),
            "{screen}"
        );
    }

    #[test]
    fn an_empty_filter_result_says_so() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.tab = crate::application::tabs::Tab::History;
        app.set_history(vec![past("fix", "api", "repair the route")]);
        app.log.filter.kind = Some("feat".to_string());

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("keine Treffer"), "{screen}");
        assert!(screen.contains("type: feat"), "{screen}");
    }

    #[test]
    fn folding_takes_body_breaking_and_footer_off_the_screen() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let full = rows(&terminal).join("\n");
        assert!(full.contains("breaking"), "{full}");

        press_mod(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let folded = rows(&terminal).join("\n");
        // The labels leave the form; the help text and the field list may still
        // name them, so look for the row, not the word.
        assert!(!folded.contains("breaking   "), "{folded}");
        assert!(folded.contains("subject"), "{folded}");
        // The preview still shows the message that would be committed.
        assert!(folded.contains("preview"), "{folded}");
    }

    #[test]
    fn the_footer_names_the_highlighted_file_in_full() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let path = "src/infrastructure/very/long/directory/chain/file_name.rs";
        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.staging.items = vec![staging_item(path)];

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let footer = rows(&terminal)
            .into_iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(footer.contains(path), "{footer}");
    }

    #[test]
    fn the_footer_keeps_the_status_beside_the_path() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let short = "src/main.rs";
        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.staging.items = vec![staging_item(short)];
        app.status = "staged src/main.rs".to_string();

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let footer = rows(&terminal)
            .into_iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(footer.contains(short), "path: {footer}");
        assert!(footer.contains("staged src/main.rs"), "status: {footer}");
    }

    #[test]
    fn the_branch_wizard_explains_the_selected_type() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.tab = crate::application::tabs::Tab::Branches;
        press(&mut app, KeyCode::Char('n'));

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert!(
            rows(&terminal).join("\n").contains("new functionality"),
            "the feat description sits in the panel title"
        );

        // The description follows the kind.
        press(&mut app, KeyCode::Right);
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("a bug fix on develop"), "{screen}");
        assert!(!screen.contains("new functionality"), "{screen}");
    }

    #[test]
    fn cursor_maps_into_wrapped_rows() {
        assert_eq!(
            cursor_in_wrapped(&["abc".into(), "def".into()], 4),
            Some((1, 1))
        );
    }
}
