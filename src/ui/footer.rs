use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;

use commitsmith::domain::text::{fit, fit_left};

use super::{HINT, KEY, MUTED, theme};

pub(crate) fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    // The active tab decides which keys are advertised: a shortcut that does
    // nothing here would be a lie in the footer. The last two entries are the
    // global ones (`1-3`, `?`) and stay, the middle ones are dropped when the
    // line gets too narrow.
    let keys = app
        .tab
        .pane_keys(app.branches.panel, app.commit_mode, app.config.keymap);
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
            if app.repo.in_repository {
                MUTED
            } else {
                theme::warn()
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
        left.push(Span::styled(status, theme::accent()));
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
    if !app.repo.in_repository {
        return format!("no git repository · {} ", app.config.profile);
    }
    let mut parts: Vec<String> = Vec::new();
    if !app.repo.line.is_empty() {
        parts.push(app.repo.line.clone());
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
        .then(|| app.branches.files.selected_path())
        .flatten()
}
