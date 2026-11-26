//! Tab 2: the branch explorer.
//!
//! One list, locals first and `HEAD` on top, then the remotes under a heading.
//! The actions live in the application layer; this module only draws state.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::App;
use crate::ui::theme;

const MUTED: Style = theme::muted();
const HINT: Style = theme::hint();

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if app.repo.git.is_none() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("  no repository ", theme::warn()),
            Span::styled("· open commitsmith inside a checkout", MUTED),
        ]));
        lines.push(Line::raw(""));
        lines.push(Line::styled("  1 back to the composer", HINT));
        frame.render_widget(Paragraph::new(Text::from(lines)).block(block(app)), area);
        return;
    }

    if app.branches.list.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("  no local branches yet", MUTED));
        frame.render_widget(Paragraph::new(Text::from(lines)).block(block(app)), area);
        return;
    }

    // Keep the selected row in view without a scroll widget: the list is short
    // and an offset is all the explorer needs.
    let height = area.height.saturating_sub(2) as usize;
    let offset = app
        .branches
        .list
        .selected
        .saturating_sub(height.saturating_sub(1));
    let mut remote_heading_drawn = false;

    for (index, branch) in app.branches.list.items.iter().enumerate().skip(offset) {
        if lines.len() >= height {
            break;
        }
        if branch.is_remote && !remote_heading_drawn {
            remote_heading_drawn = true;
            lines.push(Line::styled("  remotes", MUTED));
        }
        let selected = index == app.branches.list.selected;
        let marker = if selected { "▸ " } else { "  " };
        let head = if branch.is_head { "● " } else { "○ " };
        let width = area.width.saturating_sub(12) as usize;
        let name = shorten(&branch.name, width);

        let row = Line::from(vec![
            Span::styled(
                format!(" {marker}"),
                if selected {
                    theme::focused_label()
                } else {
                    MUTED
                },
            ),
            Span::styled(head, if branch.is_head { theme::ok() } else { MUTED }),
            Span::styled(
                format!("{name:<width$}"),
                if selected {
                    theme::focused_value()
                } else if branch.is_remote {
                    MUTED
                } else {
                    theme::idle_row()
                },
            ),
            // The sync mark sits where the upstream name used to: the numbers
            // say more than `origin/main` and fit in a narrow panel.
            Span::styled(
                app.branches
                    .list
                    .sync_label(branch)
                    .map(|label| format!(" {label}"))
                    .unwrap_or_default(),
                if selected {
                    theme::focused_label()
                } else {
                    MUTED
                },
            ),
        ]);
        // The selected branch is filled, like the focused field in the composer.
        lines.push(if selected {
            row.style(theme::focused_row())
        } else {
            row
        });
    }

    if let Some(name) = &app.branches.list.confirming {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(" delete ", theme::warn()),
            Span::styled(format!("{name} · press d again, Esc keeps it "), HINT),
        ]));
    }

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block(app)), area);
}

fn block(app: &App) -> Block<'static> {
    let count = app.branches.list.len();
    let title = if count == 1 {
        " branches · 1 ".to_string()
    } else {
        format!(" branches · {count} ")
    };
    // The wizard and the other panes take the bright border when they have the
    // keyboard; the list is only lit while it is the one being used.
    let lit =
        !app.branches.wizard.open && app.branches.panel == crate::application::Panel::Branches;
    let (border, title_style) = if lit {
        (theme::focused_panel(), theme::focused_panel())
    } else {
        (theme::idle_panel(), theme::idle_title())
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Line::from(Span::styled(title, title_style)))
}

/// Cut a branch name to `width` characters, keeping the end where the type and
/// ticket live in a conventional name.
fn shorten(name: &str, width: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= width || width < 4 {
        return name.to_string();
    }
    let keep = width.saturating_sub(1);
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_keeps_the_end_of_the_name() {
        assert_eq!(
            shorten("feat/mmu/TICKET-1/add-thing", 80),
            "feat/mmu/TICKET-1/add-thing"
        );
        assert_eq!(shorten("feat/mmu/TICKET-1/add-thing", 10), "…add-thing");
    }
}
