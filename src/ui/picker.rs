//! The pickers as the eye sees them: the overlay list and the commit preview.

use super::*;
use crate::ui::theme;

/// Overlay list with a filter line. Pickers that carry previews (history) get a
/// pane on the right showing the highlighted entry in full.
pub(crate) fn draw_picker(frame: &mut Frame, picker: &Picker, area: Rect) {
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

pub(crate) fn draw_list(
    frame: &mut Frame,
    picker: &Picker,
    visible: &[usize],
    area: Rect,
    title: String,
) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::accent())
        .title(Span::styled(title, theme::accent()));

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
pub(crate) fn draw_commit_preview(frame: &mut Frame, picker: &Picker, area: Rect) {
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
