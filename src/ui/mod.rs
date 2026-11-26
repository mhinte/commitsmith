//! The terminal surface: drawing, the event loop and the setup dialog.
//!
//! Nothing here touches `gix` or spawns a process; the state comes from
//! [`crate::application`] and is only read.

pub mod init;
pub mod picker;
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

use commitsmith::domain::text::{fit, fit_left};

use self::form::{draw_form, wrap_field};
use self::picker::draw_picker;

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
    commitsmith::domain::text::plural(count, word)
}
const HINT: Style = theme::hint();
const MUTED: Style = theme::muted();

pub mod branch_wizard;
pub mod branches;
pub mod form;
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

/// One pane for the message and its checks: the preview is what would be
/// committed, and the findings belong right under it, not in a second box.
fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    let message = app.form.commit.render();
    // Only a completely blank composer gets the hints. With just a scope typed
    // the message is already worth showing — that was the "preview does not
    // update" complaint: the placeholder stayed until a subject existed.
    let empty =
        app.form.commit.subject.trim().is_empty() && app.form.commit.scope.trim().is_empty();
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
            Span::styled(" ✓ ", theme::ok()),
            Span::styled("ready to commit", theme::ok()),
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
    let step = format!("{}/{}", app.form.field + 1, FIELDS.len());
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
