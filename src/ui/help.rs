use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::App;
use crate::application::tabs::Tab;

use super::{HINT, MUTED, theme};

pub(crate) fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    // Only what this tab can do, then the keys that work everywhere. A shortcut
    // that does nothing here would be worse than no help at all.
    let mut rows = app.tab.help();
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
                Span::styled(format!(" {key:<16}"), theme::accent()),
                Span::styled(*text, HINT),
            ])
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::accent())
        .title(Span::styled(
            format!(" keys · {} ", app.tab.label()),
            theme::accent(),
        ));

    frame.render_widget(Paragraph::new(Text::from(lines)).block(block), popup);
}
