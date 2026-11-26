//! First-start dialog: ask for the name, show the handle, write the config.
//!
//! The dialog is drawn in the whole terminal instead of in a centred box: on
//! some terminals (notably Windows) a box placed in the middle left the hardware
//! cursor a character or two off, and it also avoids the layout work.

use crate::ui::theme;
use std::io;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::config::{Config, prefix_from_name};

/// What the dialog ended with.
pub enum Outcome {
    /// Config written to this path.
    Saved(std::path::PathBuf),
    /// The user declined; nothing was written.
    Skipped,
}

struct Setup {
    name: String,
    /// Characters typed, so the cursor and umlauts behave.
    cursor: usize,
    error: Option<String>,
    done: Option<Outcome>,
}

impl Setup {
    fn new() -> Self {
        Self {
            name: String::new(),
            cursor: 0,
            error: None,
            done: None,
        }
    }

    fn prefix(&self) -> Option<String> {
        prefix_from_name(&self.name)
    }

    fn insert(&mut self, c: char) {
        // Pasting on Windows sends a CR as well; it must not submit.
        if c == '\r' || c == '\n' || c == '\t' {
            return;
        }
        let mut chars: Vec<char> = self.name.chars().collect();
        chars.insert(self.cursor.min(chars.len()), c);
        self.name = chars.into_iter().collect();
        self.cursor = (self.cursor + 1).min(self.name.chars().count());
        self.error = None;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.name.chars().collect();
        chars.remove(self.cursor - 1);
        self.name = chars.into_iter().collect();
        self.cursor -= 1;
        self.error = None;
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.name.chars().count();
        self.cursor = if delta < 0 {
            self.cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (self.cursor + delta as usize).min(len)
        };
    }

    fn submit(&mut self) {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            self.error = Some("please enter a name".into());
            return;
        }
        if prefix_from_name(&name).is_none() {
            self.error = Some("that name has no usable letters".into());
            return;
        }
        match Config::save_identity(&name) {
            Ok(path) => self.done = Some(Outcome::Saved(path)),
            Err(error) => self.error = Some(error),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Windows terminals send both presses and releases for modifier keys on
        // some hosts; only real presses must be handled.
        if key.kind == KeyEventKind::Release || self.done.is_some() {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => self.submit(),
            KeyCode::Esc => self.done = Some(Outcome::Skipped),
            KeyCode::Char('c') if ctrl => self.done = Some(Outcome::Skipped),
            KeyCode::Char('s') if ctrl => self.submit(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => {
                let len = self.name.chars().count();
                if self.cursor < len {
                    let mut chars: Vec<char> = self.name.chars().collect();
                    chars.remove(self.cursor);
                    self.name = chars.into_iter().collect();
                    self.error = None;
                }
            }
            KeyCode::Left => self.move_cursor(-1),
            KeyCode::Right => self.move_cursor(1),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.name.chars().count(),
            KeyCode::Char(c) if !ctrl => self.insert(c),
            _ => {}
        }
    }
}

/// Show the dialog. Returns what happened; the caller decides whether a failure
/// is fatal.
pub fn run(terminal: &mut DefaultTerminal) -> io::Result<Outcome> {
    // Bracketed paste keeps a pasted name from being read as separate keys,
    // and it is what makes Ctrl+V behave in Windows terminals.
    let _ = ratatui::crossterm::execute!(
        io::stdout(),
        ratatui::crossterm::event::EnableBracketedPaste
    );

    let mut setup = Setup::new();
    let mut outcome: Option<Outcome> = None;
    crate::ui::event_loop(
        terminal,
        &mut setup,
        || match crate::ui::poll_tick()? {
            true => Ok(Some(crate::ui::read_event()?)),
            false => Ok(None),
        },
        |setup, event| {
            match event {
                Some(Event::Key(key)) => setup.handle_key(key),
                Some(Event::Paste(text)) => {
                    // A pasted line break would otherwise submit the dialog.
                    for c in text.chars().filter(|c| !c.is_control()) {
                        setup.insert(c);
                    }
                }
                _ => {}
            }
            outcome = setup.done.take();
            outcome.is_some()
        },
        draw,
    )?;
    let outcome = outcome.expect("the loop only returns once the dialog is done");

    let _ = ratatui::crossterm::execute!(
        io::stdout(),
        ratatui::crossterm::event::DisableBracketedPaste
    );
    Ok(outcome)
}

fn draw(frame: &mut ratatui::Frame, setup: &Setup) {
    let area = frame.area();
    let [_, body, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(1),
    ])
    .areas(area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::accent())
        .title(Span::styled(
            " commitsmith · first start ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(ratatui::widgets::Clear, body);
    let inner = block.inner(body);
    frame.render_widget(block, body);

    let [form_area, hint_area] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(3)]).areas(inner);

    // Field text starts two columns in; the same constant positions the cursor.
    const PAD: u16 = 2;

    let prefix_line = match setup.prefix() {
        Some(prefix) => Line::from(vec![
            Span::styled("  handle   ", theme::muted()),
            Span::styled(
                prefix,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        None => Line::from(Span::styled(
            "  handle   two letters of the surname, three of the given name",
            theme::muted(),
        )),
    };

    let mut lines = vec![
        Line::styled(
            "  What is your full name?",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            if setup.name.is_empty() {
                Span::styled("Firstname Lastname", theme::muted())
            } else {
                Span::styled(setup.name.clone(), theme::bright())
            },
        ]),
        Line::raw(""),
        prefix_line,
    ];
    if let Some(error) = &setup.error {
        lines.push(Line::styled(format!("  {error}"), theme::bad()));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        form_area,
    );

    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled(
                "  Enter save · Esc skip · name and handle stay editable in the file",
                theme::hint(),
            ),
            Line::styled(
                "  written to the user config, not to this repository",
                theme::muted(),
            ),
        ])),
        hint_area,
    );

    // Cursor sits after the typed name: same column maths for padding and text.
    let typed = setup.name.chars().take(setup.cursor).count() as u16;
    let x = form_area.x + PAD + typed;
    let y = form_area.y + 2;
    if y < form_area.y + form_area.height.saturating_sub(1) {
        frame.set_cursor_position(Position::new(x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(setup: &mut Setup, code: KeyCode) {
        setup.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn type_text(setup: &mut Setup, text: &str) {
        for c in text.chars() {
            press(setup, KeyCode::Char(c));
        }
    }

    #[test]
    fn prefix_follows_the_name() {
        let mut setup = Setup::new();
        type_text(&mut setup, "Marc Hinte");
        assert_eq!(setup.prefix().as_deref(), Some("himar"));
    }

    #[test]
    fn empty_name_cannot_be_submitted() {
        let mut setup = Setup::new();
        press(&mut setup, KeyCode::Enter);
        assert!(setup.done.is_none(), "no config written without a name");
        assert!(setup.error.as_deref().unwrap().contains("name"));
    }

    #[test]
    fn esc_skips_without_writing() {
        let mut setup = Setup::new();
        type_text(&mut setup, "Marc Hinte");
        press(&mut setup, KeyCode::Esc);
        assert!(matches!(setup.done, Some(Outcome::Skipped)));
    }

    #[test]
    fn editing_works_like_a_single_line_field() {
        let mut setup = Setup::new();
        type_text(&mut setup, "abc");
        press(&mut setup, KeyCode::Left);
        press(&mut setup, KeyCode::Backspace);
        assert_eq!(setup.name, "ac");
        assert_eq!(setup.cursor, 1);
        press(&mut setup, KeyCode::End);
        assert_eq!(setup.cursor, 2);
    }

    #[test]
    fn delete_removes_the_character_under_the_cursor() {
        let mut setup = Setup::new();
        type_text(&mut setup, "abc");
        press(&mut setup, KeyCode::Home);
        press(&mut setup, KeyCode::Delete);
        assert_eq!(setup.name, "bc");
        assert_eq!(setup.cursor, 0);
    }

    #[test]
    fn a_carriage_return_never_lands_in_the_name() {
        // Pasting a line on Windows sends CRLF with the text.
        let mut setup = Setup::new();
        type_text(&mut setup, "Marc\r\nHinte");
        assert_eq!(setup.name, "MarcHinte");
        assert!(setup.done.is_none(), "CR must not submit");
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut setup = Setup::new();
        setup.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        let release = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: ratatui::crossterm::event::KeyEventState::NONE,
        };
        setup.handle_key(release);
        assert_eq!(setup.name, "a", "a released key must not type twice");
    }

    #[test]
    fn ctrl_s_also_submits() {
        let mut setup = Setup::new();
        type_text(&mut setup, "Marc Hinte");
        setup.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        // Without a writable config dir in the test environment this reports an
        // error instead of finishing, but it must have tried to save.
        assert!(setup.done.is_some() || setup.error.is_some());
    }
}
