use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::app::{App, FIELDS, Field};
use crate::validate::{self, Severity};

use super::{HINT, KEY, MUTED, theme};

pub(crate) fn draw_compact(frame: &mut Frame, app: &App, area: Rect) {
    let [header_area, field_area, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);

    let (errors, warnings) = validate::count(&app.issues);
    // Count only the fields this mode can reach: the optional ones are hidden,
    // and "2/6" while three fields are on screen was a lie. An amend has to be
    // visible here too — Ctrl+S replaces a commit instead of creating one.
    let visible: Vec<Field> = FIELDS
        .iter()
        .copied()
        .filter(|field| app.field_visible(*field))
        .collect();
    let position = visible
        .iter()
        .position(|field| *field == app.active_field())
        .map_or(0, |index| index + 1);
    let step = match app.form.amend {
        true => format!("{position}/{} · amend", visible.len()),
        false => format!("{position}/{}", visible.len()),
    };

    // The header is always complete, even while the subject is being typed: the
    // user asked to see what the message already says. It is rendered from the
    // same template the commit uses, so a scope typed a moment ago appears here
    // before it appears anywhere else.
    let header_text = app
        .form
        .commit
        .render()
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let header_shown = match header_text.is_empty() {
        true => "(no type yet)".to_string(),
        false => header_text,
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {header_shown}"),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )))
        .block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(MUTED)
                .title(Line::from(Span::styled(
                    format!(" {step} "),
                    theme::accent(),
                )))
                .title_top(
                    Line::from(Span::styled(format!(" {errors}✗ {warnings}! "), MUTED))
                        .right_aligned(),
                ),
        )
        .wrap(Wrap { trim: false }),
        header_area,
    );

    // The active field: its value with a caret where the cursor is, and on the
    // right the hint or — if this field has one — the error that concerns it. An
    // error about the field you are typing belongs next to it, not in a count.
    let field = app.active_field();
    let value = app.value(field);
    let cursor = app.cursor().min(value.chars().count());
    let (before, after): (String, String) = {
        let mut chars = value.chars();
        let before: String = chars.by_ref().take(cursor).collect();
        (before, chars.collect())
    };
    let first_error = app
        .issues
        .iter()
        .find(|issue| issue.field == field.label() && issue.severity == Severity::Error)
        .map(|issue| issue.message.clone());

    let caret_line = match value.is_empty() {
        true => Line::from(Span::styled(" ▌(empty)", MUTED)),
        false => Line::from(vec![
            Span::raw(" "),
            Span::styled(before.clone(), Style::default().fg(Color::White)),
            Span::styled(
                "▌",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(after.clone(), Style::default().fg(Color::White)),
        ]),
    };
    let mut lines = vec![caret_line];
    lines.push(Line::from(match &first_error {
        // The error replaces the hint: what is wrong is more useful than what the
        // field would like to be.
        Some(error) => Span::styled(format!(" {error}"), theme::bad()),
        None => Span::styled(
            format!(" {}{}", app.field_hint(), counter(app, field)),
            MUTED,
        ),
    }));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(theme::accent())
                    .title(Line::from(Span::styled(
                        format!(" {} ", field.label()),
                        theme::accent(),
                    ))),
            )
            .wrap(Wrap { trim: false }),
        field_area,
    );

    draw_keys(frame, app, footer);
}

/// ` · 12/72` for the subject, nothing for the fields that have no limit.
fn counter(app: &App, field: Field) -> String {
    if field != Field::Subject {
        return String::new();
    }
    format!(
        " · {}/{}",
        app.value(field).chars().count(),
        commitsmith::domain::commit::MAX_SUBJECT
    )
}

/// The keys, and the status or the version in the corner.
fn draw_keys(frame: &mut Frame, app: &App, footer: Rect) {
    // The name and version belong in the corner here too, but the narrow form
    // has to fit beside the keys.
    let corner = match app.status.is_empty() {
        // A note or a problem is worth more than the version number, and this
        // row is the only place the compact form can show one.
        false => format!("{} ", app.status),
        true => format!("v{} ", env!("CARGO_PKG_VERSION")),
    };
    let corner_width = if footer.width >= 54 {
        corner.chars().count() as u16
    } else {
        0
    };
    let [keys_area, corner_area] =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(corner_width)]).areas(footer);

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
    if corner_width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(corner, MUTED))).right_aligned(),
            corner_area,
        );
    }
}
