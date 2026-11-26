//! Tab 1, left side: what changed, what is staged, and the diff on demand.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::app::App;
use crate::ui::theme;

const MUTED: Style = Style::new().fg(Color::DarkGray);
const HINT: Style = Style::new().fg(Color::Gray);

pub fn draw(frame: &mut Frame, app: &App, area: Rect, focused: bool) {
    let staging = &app.staging;

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme::focused_panel()
        } else {
            theme::idle_panel()
        })
        .title(Line::from(Span::styled(
            match (staging.items.is_empty(), staging.staged_count()) {
                (true, _) => " files ".to_string(),
                (false, 0) => format!(" files · {} ", staging.items.len()),
                (false, staged) => format!(" files · {} · {} staged ", staging.items.len(), staged),
            },
            if focused {
                theme::focused_panel()
            } else {
                theme::idle_title()
            },
        )));

    // The diff lives in its own pane on tab 2; this one is only the list.
    let list_area = area;
    let visible = list_area.height.saturating_sub(2) as usize;
    let name_width = list_area.width.saturating_sub(7) as usize;
    let mut lines: Vec<Line> = Vec::new();

    if staging.items.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("  nothing changed", MUTED));
        lines.push(Line::styled("  edit a file to see it here", MUTED));
    } else {
        let offset = scroll_offset(staging.selected, visible);
        for (index, file) in staging.items.iter().enumerate().skip(offset) {
            if lines.len() >= visible {
                break;
            }
            let selected = index == staging.selected;
            let code = file
                .index
                .map(|state| state.code().to_string())
                .or_else(|| file.worktree.map(|state| state.code().to_string()))
                .unwrap_or_else(|| "?".to_string());
            let mut spans = vec![
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
                    if file.is_staged() {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Yellow)
                    },
                ),
            ];
            spans.extend(path_spans(&file.path, name_width, selected));
            let row = Line::from(spans);
            lines.push(if selected {
                row.style(theme::focused_row())
            } else {
                row
            });
        }
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), list_area);
}

/// First row to draw so the selection stays inside the pane.
fn scroll_offset(selected: usize, visible: usize) -> usize {
    if visible == 0 {
        return 0;
    }
    selected.saturating_sub(visible.saturating_sub(1))
}

/// Directory dimmed, file name bright — shortened from the left, because the end
/// of a path is the part that identifies the file.
fn path_spans(path: &str, width: usize, selected: bool) -> Vec<Span<'static>> {
    let (dir, name) = match path.rfind('/') {
        Some(index) => (&path[..index], &path[index + 1..]),
        None => ("", path),
    };
    let name_style = if selected {
        theme::focused_value()
    } else {
        theme::idle_row()
    };
    if dir.is_empty() || width <= name.chars().count() + 2 {
        return vec![Span::styled(truncate_left(path, width), name_style)];
    }
    let room = width - name.chars().count() - 1;
    let mut dir_text = truncate_left(dir, room);
    if dir_text.chars().count() == dir.chars().count() {
        dir_text.push('/');
    }
    vec![
        Span::styled(dir_text, MUTED),
        Span::styled(name.to_string(), name_style),
    ]
}

/// Cut a path from the left, keeping the end where the file name lives.
fn truncate_left(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    if width < 4 {
        return chars[chars.len() - width..].iter().collect();
    }
    let keep = width - 1;
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("…{tail}")
}

pub fn draw_diff(frame: &mut Frame, app: &App, area: Rect) {
    // The list shows a shortened name; the diff names the file in full, so the
    // complete path is always on screen somewhere.
    let title = match app.staging.selected_path() {
        Some(path) => format!(" diff · {path} "),
        None => " diff ".to_string(),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::idle_panel())
        .title(Line::from(Span::styled(title, theme::idle_title())));

    let height = area.height.saturating_sub(2) as usize;
    let offset = app.staging.diff_offset;
    let mut lines: Vec<Line> = app
        .staging
        .diff
        .lines()
        .skip(offset)
        .map(|raw| {
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
            Line::styled(raw.to_string(), style)
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::styled("  no diff for this file", MUTED));
    }
    lines.truncate(height.max(1));

    // Long lines wrap instead of running off the right edge; the indent of a
    // wrapped line keeps the code readable.
    frame.render_widget(
        Paragraph::new(Text::from(lines.clone()))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );

    // A slim scrollbar: where in the file this window sits.
    let total = app.staging.diff.lines().count();
    if total > height.max(1) && area.height > 2 {
        let track = area.height.saturating_sub(2);
        let visible = height.min(total).max(1) as u16;
        let span = (track as usize * visible as usize / total).max(1) as u16;
        let max_offset = total - visible as usize;
        let start = (app.staging.diff_offset * track as usize)
            .checked_div(max_offset)
            .unwrap_or(0) as u16;
        for step in 0..span {
            let y = area.y + 1 + start.min(track.saturating_sub(span)) + step;
            if y < area.y + 1 + track {
                frame.render_widget(
                    Paragraph::new("▐").style(Style::default().fg(Color::Cyan)),
                    Rect {
                        x: area.x + area.width - 1,
                        y,
                        width: 1,
                        height: 1,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scroll_offset_keeps_the_selection_visible() {
        assert_eq!(scroll_offset(0, 10), 0);
        assert_eq!(scroll_offset(9, 10), 0, "still on the first page");
        assert_eq!(scroll_offset(10, 10), 1, "scrolls by one");
        assert_eq!(scroll_offset(30, 10), 21);
        assert_eq!(scroll_offset(5, 0), 0, "no division by zero");
    }

    #[test]
    fn a_short_path_keeps_its_directory() {
        let text: String = path_spans("src/ui/mod.rs", 30, false)
            .iter()
            .map(|span| span.content.to_string())
            .collect();
        assert_eq!(text, "src/ui/mod.rs");
    }

    #[test]
    fn a_long_path_keeps_the_file_name() {
        let text: String = path_spans(
            "a/very/deeply/nested/directory/structure/file.rs",
            18,
            false,
        )
        .iter()
        .map(|span| span.content.to_string())
        .collect();
        assert!(text.ends_with("file.rs"), "{text}");
        assert!(text.starts_with('…'), "{text}");
        assert!(text.chars().count() <= 18, "{text}");
    }

    #[test]
    fn a_narrow_pane_still_shows_the_end_of_the_name() {
        let text: String = path_spans("README.md", 4, false)
            .iter()
            .map(|span| span.content.to_string())
            .collect();
        assert!(text.ends_with("md"), "{text}");
    }
}
