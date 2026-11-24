//! Application state and the event loop.

use std::io;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::commit::{self, Commit};
use crate::ui;
use crate::validate::{self, Issue};

/// Order of the form fields, top to bottom.
pub const FIELDS: [Field; 6] = [
    Field::Type,
    Field::Scope,
    Field::Subject,
    Field::Body,
    Field::Breaking,
    Field::Footer,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Type,
    Scope,
    Subject,
    Body,
    Breaking,
    Footer,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Type => "type",
            Field::Scope => "scope",
            Field::Subject => "subject",
            Field::Body => "body",
            Field::Breaking => "breaking",
            Field::Footer => "footer",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Field::Type => "←/→ switch",
            Field::Scope => "optional, e.g. api",
            Field::Subject => "imperative, no period",
            Field::Body => "why and what, optional",
            Field::Breaking => "what breaks, optional",
            Field::Footer => "Refs: #12, optional",
        }
    }
}

pub struct App {
    pub commit: Commit,
    /// Index into `commit::TYPES`, kept in sync with `commit.kind`.
    pub type_idx: usize,
    pub field: usize,
    /// Cursor position per field, counted in characters, not bytes.
    cursors: [usize; FIELDS.len()],
    pub issues: Vec<Issue>,
    pub status: String,
    /// Set when the user asks to hand the message to git.
    pub output: Option<String>,
    pub should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        let mut app = Self {
            commit: Commit {
                kind: commit::TYPES[0].to_string(),
                ..Default::default()
            },
            type_idx: 0,
            field: 0,
            cursors: [0; FIELDS.len()],
            issues: Vec::new(),
            status: String::new(),
            output: None,
            should_quit: false,
        };
        app.revalidate();
        app
    }
}

impl App {
    /// True while the active field takes typed characters.
    pub fn is_editing(&self) -> bool {
        self.active_field() != Field::Type
    }

    pub fn active_field(&self) -> Field {
        FIELDS[self.field.min(FIELDS.len() - 1)]
    }

    /// Cursor of the active field.
    pub fn cursor(&self) -> usize {
        self.cursors[self.field]
    }

    /// Value of a field, for rendering.
    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::Type => &self.commit.kind,
            Field::Scope => &self.commit.scope,
            Field::Subject => &self.commit.subject,
            Field::Body => &self.commit.body,
            Field::Breaking => &self.commit.breaking,
            Field::Footer => &self.commit.footer,
        }
    }

    fn value_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Type => &mut self.commit.kind,
            Field::Scope => &mut self.commit.scope,
            Field::Subject => &mut self.commit.subject,
            Field::Body => &mut self.commit.body,
            Field::Breaking => &mut self.commit.breaking,
            Field::Footer => &mut self.commit.footer,
        }
    }

    pub fn revalidate(&mut self) {
        self.issues = validate::validate(&self.commit);
    }

    fn cycle_type(&mut self, forward: bool) {
        let len = commit::TYPES.len();
        self.type_idx = if forward {
            (self.type_idx + 1) % len
        } else {
            (self.type_idx + len - 1) % len
        };
        self.commit.kind = commit::TYPES[self.type_idx].to_string();
        self.revalidate();
    }

    fn focus(&mut self, index: usize) {
        self.field = index % FIELDS.len();
        let len = self.value(self.active_field()).chars().count();
        self.cursors[self.field] = self.cursors[self.field].min(len);
    }

    fn next_field(&mut self) {
        self.focus(self.field + 1);
    }

    fn previous_field(&mut self) {
        self.focus((self.field + FIELDS.len() - 1) % FIELDS.len());
    }

    fn insert(&mut self, c: char) {
        let field = self.active_field();
        if field == Field::Type {
            return;
        }
        let cursor = self.cursors[self.field];
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        chars.insert(cursor.min(chars.len()), c);
        *value = chars.into_iter().collect();
        self.cursors[self.field] = (cursor + 1).min(value.chars().count());
        self.revalidate();
    }

    fn backspace(&mut self) {
        let field = self.active_field();
        if field == Field::Type {
            return;
        }
        let cursor = self.cursors[self.field];
        if cursor == 0 {
            return;
        }
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        chars.remove(cursor - 1);
        *value = chars.into_iter().collect();
        self.cursors[self.field] = cursor - 1;
        self.revalidate();
    }

    fn delete(&mut self) {
        let field = self.active_field();
        if field == Field::Type {
            return;
        }
        let cursor = self.cursors[self.field];
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        if cursor < chars.len() {
            chars.remove(cursor);
            *value = chars.into_iter().collect();
            self.revalidate();
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.value(self.active_field()).chars().count();
        let cursor = self.cursors[self.field];
        let next = if delta < 0 {
            cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (cursor + delta as usize).min(len)
        };
        self.cursors[self.field] = next;
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') | KeyCode::Char('q') if ctrl => self.should_quit = true,
            KeyCode::Char('s') if ctrl => {
                self.output = Some(self.commit.render());
                self.should_quit = true;
                self.status = "message written to stdout".to_string();
            }
            KeyCode::Char('q') if self.active_field() == Field::Type => self.should_quit = true,
            KeyCode::Tab => self.next_field(),
            KeyCode::BackTab => self.previous_field(),
            KeyCode::Enter => self.next_field(),
            KeyCode::Up => self.previous_field(),
            KeyCode::Down => self.next_field(),
            KeyCode::Left | KeyCode::Char('h') if self.active_field() == Field::Type => {
                self.cycle_type(false);
            }
            KeyCode::Right | KeyCode::Char('l') if self.active_field() == Field::Type => {
                self.cycle_type(true);
            }
            KeyCode::Left => self.move_cursor(-1),
            KeyCode::Right => self.move_cursor(1),
            KeyCode::Home => self.cursors[self.field] = 0,
            KeyCode::End => {
                self.cursors[self.field] = self.value(self.active_field()).chars().count();
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Char(c) if !ctrl => self.insert(c),
            _ => {}
        }
    }
}

/// Run the TUI until the user quits or hands the message over.
pub fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
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
mod tests {
    use super::*;

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn starts_on_the_type_field() {
        let app = App::default();
        assert_eq!(app.active_field(), Field::Type);
        assert!(!app.is_editing());
        assert_eq!(app.commit.kind, "feat");
    }

    #[test]
    fn cycles_types_with_arrows() {
        let mut app = App::default();
        press(&mut app, KeyCode::Right);
        assert_eq!(app.commit.kind, "fix");
        press(&mut app, KeyCode::Left);
        assert_eq!(app.commit.kind, "feat");
        press(&mut app, KeyCode::Left);
        assert_eq!(app.commit.kind, "revert");
    }

    #[test]
    fn typing_lands_in_the_active_field() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        for c in "api".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert_eq!(app.commit.scope, "api");
        assert_eq!(app.cursor(), 3);
    }

    #[test]
    fn backspace_and_delete_edit_around_the_cursor() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('b'));
        press(&mut app, KeyCode::Char('c'));
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.commit.scope, "ac");
        press(&mut app, KeyCode::Delete);
        assert_eq!(app.commit.scope, "a");
    }

    #[test]
    fn tab_alone_does_not_insert_a_tab() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.commit.scope, "");
        assert_eq!(app.active_field(), Field::Subject);
    }

    #[test]
    fn ctrl_s_renders_the_message() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        for c in "add composer".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(app.output.as_deref(), Some("feat: add composer"));
        assert!(app.should_quit);
    }

    #[test]
    fn q_quits_only_outside_a_text_field() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.commit.scope, "q");
        assert!(!app.should_quit);
        press(&mut app, KeyCode::Esc);
        assert!(app.should_quit);
    }

    #[test]
    fn validation_follows_the_input() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        for c in "Added stuff.".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert!(
            app.issues
                .iter()
                .any(|issue| issue.message.contains("imperative"))
        );
    }
}
