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
                // The wizard opens as a pane under the list, like the diff under
                // the files: the branches and the files stay in view.
                let [tree, right] =
                    Layout::horizontal([Constraint::Percentage(46), Constraint::Min(34)])
                        .areas(body);
                let [list, wizard] =
                    Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .areas(tree);
                branches::draw(frame, app, list);
                branch_wizard::draw(frame, app, wizard);
                let [files, diff] = if app.staging.show_diff {
                    Layout::vertical([Constraint::Percentage(50), Constraint::Min(6)]).areas(right)
                } else {
                    [right, Rect::default()]
                };
                staging::draw(
                    frame,
                    app,
                    files,
                    app.panel == crate::application::Panel::Files,
                );
                if app.staging.show_diff {
                    staging::draw_diff(frame, app, diff);
                }
                None
            } else {
                // Same shape as the other tabs: one main pane, two support panes.
                let [tree, right] =
                    Layout::horizontal([Constraint::Percentage(46), Constraint::Min(34)])
                        .areas(body);
                let [files, diff] = if app.staging.show_diff {
                    Layout::vertical([Constraint::Percentage(50), Constraint::Min(6)]).areas(right)
                } else {
                    [right, Rect::default()]
                };
                branches::draw(frame, app, tree);
                staging::draw(
                    frame,
                    app,
                    files,
                    app.panel == crate::application::Panel::Files,
                );
                if app.staging.show_diff {
                    staging::draw_diff(frame, app, diff);
                }
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
    // The files moved to tab 2; here the composer gets the width it earns.
    let right = area;
    // Body, breaking and footer all grow to MULTILINE_ROWS; counting only two of
    // them cut the footer's last rows off.
    // A multiline field takes one row while it is empty and grows with its text,
    // up to three: fixed three-row blocks wasted half the form.
    let form_width = right.width.saturating_sub(LABEL_WIDTH as u16 + 4) as usize;
    let rows: u16 = FIELDS
        .iter()
        .filter(|field| app.field_visible(**field))
        .map(|field| {
            if field.multiline() {
                let wrapped = wrap_field(app.value(*field), form_width).len().max(1);
                wrapped.min(MULTILINE_ROWS as usize) as u16
            } else {
                1
            }
        })
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

/// One pane for the message and its checks: the preview is what would be
/// committed, and the findings belong right under it, not in a second box.
fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    let message = app.commit.render();
    // Only a completely blank composer gets the hints. With just a scope typed
    // the message is already worth showing — that was the "preview does not
    // update" complaint: the placeholder stayed until a subject existed.
    let empty = app.commit.subject.trim().is_empty() && app.commit.scope.trim().is_empty();
    let mut lines: Vec<Line> = message
        .lines()
        .map(|line| Line::raw(line.to_string()))
        .collect();

    if empty {
        lines = vec![
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
        ];
    }

    lines.push(Line::raw(""));
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

    let (errors, warnings) = validate::count(&app.issues);
    let checks = format!(
        " {} · {} ",
        plural(errors, "error"),
        plural(warnings, "warning")
    );
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        .title(Span::styled(" preview ", HINT))
        .title_top(Line::from(Span::styled(checks, HINT)).right_aligned());

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
    // Tab 1 has no file list; there the footer is free for the status.
    (app.tab == crate::application::tabs::Tab::Branches)
        .then(|| app.staging.selected_path())
        .flatten()
}

/// Cut `text` from the left to `width` cells: the end of a path is the useful part.
fn fit_left(text: &str, width: usize) -> String {
    commitsmith::domain::text::fit_left(text, width)
}

/// Cut `text` to `width` display cells, with an ellipsis when it was longer.
fn fit(text: &str, width: usize) -> String {
    commitsmith::domain::text::fit(text, width)
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
mod tests;
