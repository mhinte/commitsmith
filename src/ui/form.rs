//! The form: the fields that make up a commit, their wrapping and the cursor.
//!
//! Wrapped values mean the cursor is not a column but a (row, column) pair; the
//! maths for that lives here, next to the drawing that needs it.

use super::*;

/// Draws the form and returns the terminal cursor position, if any.
pub(crate) fn draw_form(frame: &mut Frame, app: &App, area: Rect) -> Option<(u16, u16)> {
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
pub(crate) fn subject_len(app: &App) -> (usize, usize) {
    (app.commit.subject.chars().count(), app.rules.max_subject)
}

/// Soft-wrap a field value to `width`.
pub(crate) fn wrap_field(value: &str, width: usize) -> Vec<String> {
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
pub(crate) fn cursor_in_wrapped(rows: &[String], cursor: usize) -> Option<(usize, usize)> {
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
pub(crate) fn cells(c: char) -> usize {
    c.width().unwrap_or(0)
}

pub(crate) fn text_cells(text: &str) -> usize {
    text.chars().map(cells).sum()
}

/// Cut a string to at most `width` columns, keeping whole characters.
pub(crate) fn take_cells(text: &str, width: usize) -> (String, usize) {
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
pub(crate) fn window(
    value: &str,
    cursor: usize,
    active: bool,
    width: usize,
) -> (String, Option<usize>) {
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
