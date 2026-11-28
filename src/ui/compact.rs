use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::app::{App, FIELDS, Field};
use crate::validate;

use super::{HINT, KEY, MUTED, theme, wrap_field};

pub(crate) fn draw_compact(frame: &mut Frame, app: &App, area: Rect) {
    let [header, body, footer] = Layout::vertical([
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
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(MUTED)
        // No program name here any more: it lives in the corner.
        .title(Line::from(Span::styled(
            format!(" {step} "),
            theme::accent(),
        )))
        .title_top(
            Line::from(Span::styled(format!(" {errors}✗ {warnings}! "), MUTED)).right_aligned(),
        );

    let header_line = Line::from(vec![
        Span::styled(format!(" {} ", app.form.commit.kind), theme::accent()),
        Span::styled(
            format!(
                "{} chars · {}",
                app.form.commit.subject.chars().count(),
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
            app.form.commit.render().lines().next().unwrap_or_default()
        ),
        MUTED,
    ));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(theme::accent()),
            )
            .wrap(Wrap { trim: false }),
        body,
    );

    // The name and version belong in the corner here too, but the narrow form
    // has to fit beside the keys.
    let version = match app.status.is_empty() {
        // A note or a problem is worth more than the version number, and this
        // row is the only place the compact form can show one.
        false => format!("{} ", app.status),
        true => format!("v{} ", env!("CARGO_PKG_VERSION")),
    };
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
