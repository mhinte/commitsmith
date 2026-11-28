//! The terminal surface: drawing, the event loop and the setup dialog.
//!
//! Nothing here touches `gix` or spawns a process; the state comes from
//! [`crate::application`] and is only read.

pub mod compact;
pub mod footer;
pub mod help;
pub mod hooks_wizard;
pub mod init;
pub mod picker;
pub mod setup;
pub mod wizard;

use std::io;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::crossterm::event::{self, Event};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Row, Table, Wrap};
use unicode_width::UnicodeWidthChar;

use crate::app::{App, FIELDS, Field, Picker, PickerKind};
use crate::application::tabs::Tab;
use crate::validate::{self, Severity};

use self::form::{draw_form, wrap_field};
use self::picker::draw_picker;

pub(crate) const LABEL_WIDTH: usize = 10;
/// Columns from the inner edge to the first character of a value: one leading
/// space, the padded label, then the `▌ ` marker. The caret is placed with this
/// offset, so a change to the label layout cannot shift it by a column again.
pub(crate) const VALUE_OFFSET: u16 = LABEL_WIDTH as u16 + 3;
/// Rows a multiline field may occupy in the form.
pub(crate) const MULTILINE_ROWS: u16 = 3;
/// Below this height the full form does not fit, so one field is shown at a time.
pub(crate) const COMPACT_HEIGHT: u16 = 24;
pub(crate) const COMPACT_WIDTH: u16 = 62;

pub(crate) const KEY: Style = Style::new().fg(Color::Black).bg(Color::DarkGray);

/// "1 error", "2 errors" — the counts sit in titles the user reads constantly.
fn plural(count: usize, word: &str) -> String {
    commitsmith::domain::text::plural(count, word)
}
pub(crate) const HINT: Style = theme::hint();
pub(crate) const MUTED: Style = theme::muted();

pub mod branch_wizard;
pub mod branches;
pub mod form;
pub mod history;
pub mod staging;
pub mod theme;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    if area.height < COMPACT_HEIGHT || area.width < COMPACT_WIDTH {
        compact::draw_compact(frame, app, area);
    } else {
        draw_full(frame, app, area);
    }

    if let Some(picker) = &app.picker {
        draw_picker(frame, picker, area);
    }
    if app.show_help {
        help::draw_help(frame, app, area);
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
            if app.branches.wizard.open {
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
                let [files, diff] = if app.branches.files.show_diff {
                    Layout::vertical([Constraint::Percentage(50), Constraint::Min(6)]).areas(right)
                } else {
                    [right, Rect::default()]
                };
                staging::draw(
                    frame,
                    app,
                    files,
                    app.branches.panel == crate::application::Panel::Files,
                );
                if app.branches.files.show_diff {
                    staging::draw_diff(frame, app, diff);
                }
                None
            } else {
                // Same shape as the other tabs: one main pane, two support panes.
                let [tree, right] =
                    Layout::horizontal([Constraint::Percentage(46), Constraint::Min(34)])
                        .areas(body);
                let [files, diff] = if app.branches.files.show_diff {
                    Layout::vertical([Constraint::Percentage(50), Constraint::Min(6)]).areas(right)
                } else {
                    [right, Rect::default()]
                };
                branches::draw(frame, app, tree);
                staging::draw(
                    frame,
                    app,
                    files,
                    app.branches.panel == crate::application::Panel::Files,
                );
                if app.branches.files.show_diff {
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
    footer::draw_footer(frame, app, footer);

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
                    "  Ctrl+D runs git commit with this message"
                } else {
                    "  Ctrl+D prints the message and exits (start with -c to commit)"
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

/// How long a tick waits for a key before the loop looks at its state again.
const TICK: Duration = Duration::from_millis(200);

/// Wait up to one tick for input; `true` when something arrived.
pub fn poll_tick() -> io::Result<bool> {
    event::poll(TICK)
}

/// The input [`poll_tick`] waited for.
pub fn read_event() -> io::Result<Event> {
    event::read()
}

/// Draw when something happened, not five times a second.
///
/// The old loop drew on every poll, so layout, wrapping, filtering and the lint
/// lookups ran 200 ms apart with nobody at the keyboard — a busy core behind a
/// screen that did not change. Now a frame is drawn when an event arrived (or on
/// the first pass), and a tick without one costs a poll and a state check.
///
/// `next` hands back the event, or `None` for a tick without one; `step` gets
/// that `Option` and returns true when the loop is done, so a later background
/// job can react on a tick instead of waiting for a key.
pub fn drive<S>(
    state: &mut S,
    mut next: impl FnMut() -> io::Result<Option<Event>>,
    mut step: impl FnMut(&mut S, Option<Event>) -> bool,
    mut draw: impl FnMut(&S) -> io::Result<()>,
) -> io::Result<()> {
    let mut dirty = true;
    loop {
        if dirty {
            draw(state)?;
        }
        let event = next()?;
        dirty = event.is_some();
        if step(state, event) {
            return Ok(());
        }
    }
}

/// [`drive`] against a real terminal.
pub fn event_loop<B, S>(
    terminal: &mut Terminal<B>,
    state: &mut S,
    next: impl FnMut() -> io::Result<Option<Event>>,
    step: impl FnMut(&mut S, Option<Event>) -> bool,
    draw: impl Fn(&mut Frame, &S),
) -> io::Result<()>
where
    B: ratatui::backend::Backend,
    B::Error: Into<io::Error>,
{
    drive(state, next, step, |state| {
        match terminal.draw(|frame| draw(frame, state)) {
            Ok(_) => Ok(()),
            Err(error) => Err(error.into()),
        }
    })
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

    event_loop(
        terminal,
        app,
        || match event::poll(TICK)? {
            true => Ok(Some(event::read()?)),
            false => Ok(None),
        },
        |app, event| {
            match event {
                Some(Event::Key(key)) => app.handle_key(key),
                // A pasted block of text goes into the active field, line
                // breaks included, instead of being replayed as key presses.
                Some(Event::Paste(text)) => app.handle_paste(&text),
                // A resize is not a state change, but it is a reason to draw;
                // everything else (and a tick without input) changes nothing.
                _ => {}
            }
            app.should_quit
        },
        draw,
    )
}

#[cfg(test)]
mod tests;
