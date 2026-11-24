//! First-start dialog: ask for the name, show the handle, write the config.

use std::io;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};

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
        if key.kind != KeyEventKind::Press || self.done.is_some() {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => self.submit(),
            KeyCode::Esc => self.done = Some(Outcome::Skipped),
            KeyCode::Char('c') if ctrl => self.done = Some(Outcome::Skipped),
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
    let mut setup = Setup::new();
    loop {
        terminal.draw(|frame| draw(frame, &setup))?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                setup.handle_key(key);
            }
        }
        if let Some(outcome) = setup.done.take() {
            return Ok(outcome);
        }
    }
}

fn draw(frame: &mut ratatui::Frame, setup: &Setup) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).clamp(28, 64);
    let height = area.height.saturating_sub(2).clamp(9, 14);
    let popup = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " coggit · first start ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [form_area, hint_area] =
        Layout::vertical([Constraint::Min(4), Constraint::Length(3)]).areas(inner);

    let prefix = setup.prefix();
    let prefix_line = match &prefix {
        Some(prefix) => Line::from(vec![
            Span::styled("  handle   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                prefix.clone(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        None => Line::from(Span::styled(
            "  handle   two letters of the surname, three of the given name",
            Style::default().fg(Color::DarkGray),
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
                Span::styled("Firstname Lastname", Style::default().fg(Color::DarkGray))
            } else {
                Span::styled(setup.name.clone(), Style::default().fg(Color::White))
            },
        ]),
        Line::raw(""),
        prefix_line,
    ];
    if let Some(error) = &setup.error {
        lines.push(Line::styled(
            format!("  {error}"),
            Style::default().fg(Color::Red),
        ));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        form_area,
    );

    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled("  Enter save · Esc skip", Style::default().fg(Color::Gray)),
            Line::styled(
                "  written to the user config, not to this repository",
                Style::default().fg(Color::DarkGray),
            ),
            Line::styled(
                "  used as the default footer: Co-authored-by",
                Style::default().fg(Color::DarkGray),
            ),
        ])),
        hint_area,
    );

    // Cursor inside the name field: third line of the form area.
    let field_x = form_area.x + 2 + setup.cursor as u16;
    let field_y = form_area.y + 2;
    if field_y < form_area.y + form_area.height {
        frame.set_cursor_position(Position::new(field_x, field_y));
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
}
