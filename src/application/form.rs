//! What the user is typing: the commit, where the cursor sits, what is folded.
//!
//! Split out of `App`, which had grown into one struct for the form, the
//! repository, the views and the commands at once. The rules that only need the
//! form live next to it: they read the text, the cursor and the field index and
//! answer whether something changed. `App` owns the consequences — revalidating,
//! seeding the footer, writing a status line — because those need the config and
//! the repository.
//!
//! That split is the point: a method that answers `bool` can be tested with a
//! `Form` and nothing else.

use super::*;

#[derive(Debug, Default, Clone)]
pub struct Form {
    /// The commit being written.
    pub commit: Commit,
    /// Index into `FIELDS`.
    pub field: usize,
    /// Cursor position per field, counted in characters, not bytes.
    pub(crate) cursors: [usize; FIELDS.len()],
    /// True once the user chose a type; the branch must not override it.
    pub(crate) type_touched: bool,
    /// What `seed_footer` last wrote, so a later suggestion may replace its own
    /// text but never what the user typed.
    pub(crate) footer_seed: Option<String>,
    /// Body, breaking and footer folded away.
    pub compact: bool,
    /// Ctrl+A loaded the message of `HEAD`: Ctrl+D replaces that commit instead
    /// of creating a new one.
    pub amend: bool,
    /// Set when the commit being replaced is already on the remote, so the next
    /// keypress has to confirm it.
    pub confirm_amend: bool,
}

/// What an attempt to empty the active field did, so `App` can say it.
///
/// The wording belongs to `App` (it writes the status line), the decision
/// belongs here — that is why this is an answer and not a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleared {
    /// The type is a choice, not text: nothing was cleared.
    TypeStays,
    /// There was nothing to clear.
    AlreadyEmpty(Field),
    /// The field was emptied.
    Done(Field),
}

impl Form {
    /// A form that starts with the given type.
    pub fn with_kind(kind: String) -> Self {
        Self {
            commit: Commit {
                kind,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn active_field(&self) -> Field {
        FIELDS[self.field.min(FIELDS.len() - 1)]
    }

    /// Cursor of the active field.
    pub fn cursor(&self) -> usize {
        self.cursors[self.field]
    }

    /// Value of a field, for rendering and for the editing rules here.
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

    pub(crate) fn value_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Type => &mut self.commit.kind,
            Field::Scope => &mut self.commit.scope,
            Field::Subject => &mut self.commit.subject,
            Field::Body => &mut self.commit.body,
            Field::Breaking => &mut self.commit.breaking,
            Field::Footer => &mut self.commit.footer,
        }
    }

    /// A field is on screen unless it is optional and the form is compact.
    pub fn field_visible(&self, field: Field) -> bool {
        !self.compact || !field.optional()
    }

    /// Put the cursor on a field, never past the end of its text.
    pub(crate) fn focus(&mut self, index: usize) {
        self.field = index % FIELDS.len();
        let len = self.value(self.active_field()).chars().count();
        self.cursors[self.field] = self.cursors[self.field].min(len);
    }

    /// Move to the next field that is actually on screen. Hidden fields keep
    /// their content, so walking past them must not clear or skip anything else.
    pub(crate) fn step_field(&mut self, delta: isize) {
        let len = FIELDS.len() as isize;
        let mut index = self.field as isize;
        for _ in 0..len {
            index = (index + delta).rem_euclid(len);
            if self.field_visible(FIELDS[index as usize]) {
                self.focus(index as usize);
                return;
            }
        }
    }

    pub(crate) fn next_field(&mut self) {
        self.step_field(1);
    }

    pub(crate) fn previous_field(&mut self) {
        self.step_field(-1);
    }

    /// The type is a choice from a list that belongs to the config: the caller
    /// hands it in, the form only moves through it.
    pub(crate) fn cycle_type(&mut self, types: &[String], forward: bool) -> bool {
        if types.is_empty() {
            return false;
        }
        let idx = match types.iter().position(|kind| *kind == self.commit.kind) {
            Some(idx) if forward => (idx + 1) % types.len(),
            Some(idx) => (idx + types.len() - 1) % types.len(),
            None => 0,
        };
        self.commit.kind.clone_from(&types[idx]);
        self.type_touched = true;
        true
    }

    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let len = self.value(self.active_field()).chars().count();
        let cursor = self.cursors[self.field];
        self.cursors[self.field] = if delta < 0 {
            cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (cursor + delta as usize).min(len)
        };
    }

    /// Start of the line the cursor is on.
    pub(crate) fn home(&mut self) {
        let value = self.value(self.active_field()).to_string();
        let cursor = self.cursors[self.field].min(value.chars().count());
        let chars: Vec<char> = value.chars().collect();
        self.cursors[self.field] = chars[..cursor]
            .iter()
            .rposition(|c| *c == '\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
    }

    /// End of the line the cursor is on.
    pub(crate) fn end(&mut self) {
        let value = self.value(self.active_field()).to_string();
        let cursor = self.cursors[self.field].min(value.chars().count());
        let chars: Vec<char> = value.chars().collect();
        self.cursors[self.field] = chars[cursor..]
            .iter()
            .position(|c| *c == '\n')
            .map(|offset| cursor + offset)
            .unwrap_or(chars.len());
    }

    /// `↑`/`↓` inside a multiline field stay on the line where possible.
    ///
    /// Answers `false` when the move would leave the text (a one-line field, the
    /// first line upwards, the last line downwards): moving to another field is
    /// the caller's decision, because only it knows which fields are on screen.
    pub(crate) fn line_move(&mut self, delta: isize) -> bool {
        let field = self.active_field();
        if !field.multiline() {
            return false;
        }
        let value = self.value(field).to_string();
        let cursor = self.cursors[self.field];
        let chars: Vec<char> = value.chars().collect();
        let line_start = chars[..cursor.min(chars.len())]
            .iter()
            .rposition(|c| *c == '\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        let column = cursor - line_start;
        let target = if delta < 0 {
            if line_start == 0 {
                return false;
            }
            let previous_end = line_start - 1;
            let previous_start = chars[..previous_end]
                .iter()
                .rposition(|c| *c == '\n')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            (previous_start + column).min(previous_end)
        } else {
            let next = chars[cursor..]
                .iter()
                .position(|c| *c == '\n')
                .map(|offset| cursor + offset + 1);
            match next {
                Some(start) => {
                    let end = chars[start..]
                        .iter()
                        .position(|c| *c == '\n')
                        .map(|offset| start + offset)
                        .unwrap_or(chars.len());
                    (start + column).min(end)
                }
                None => return false,
            }
        };
        self.cursors[self.field] = target;
        true
    }

    /// Insert one character at the cursor. `false` when the field takes no text.
    pub(crate) fn insert(&mut self, c: char) -> bool {
        let field = self.active_field();
        if field == Field::Type {
            return false;
        }
        let cursor = self.cursors[self.field];
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        chars.insert(cursor.min(chars.len()), c);
        *value = chars.into_iter().collect();
        self.cursors[self.field] = (cursor + 1).min(value.chars().count());
        true
    }

    /// Remove the character before the cursor. `false` when there is none.
    pub(crate) fn backspace(&mut self) -> bool {
        let field = self.active_field();
        if field == Field::Type {
            return false;
        }
        let cursor = self.cursors[self.field];
        if cursor == 0 {
            return false;
        }
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        chars.remove(cursor - 1);
        *value = chars.into_iter().collect();
        self.cursors[self.field] = cursor - 1;
        true
    }

    /// Remove the character at the cursor. `false` when there is none.
    pub(crate) fn delete(&mut self) -> bool {
        let field = self.active_field();
        if field == Field::Type {
            return false;
        }
        let cursor = self.cursors[self.field];
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        if cursor >= chars.len() {
            return false;
        }
        chars.remove(cursor);
        *value = chars.into_iter().collect();
        true
    }

    /// Ctrl+U: empty the field under the cursor. The type is a choice from the
    /// picker, not text, so it stays.
    pub(crate) fn clear(&mut self) -> Cleared {
        let field = self.active_field();
        if field == Field::Type {
            return Cleared::TypeStays;
        }
        if self.value(field).trim().is_empty() {
            return Cleared::AlreadyEmpty(field);
        }
        *self.value_mut(field) = String::new();
        self.cursors[self.field] = 0;
        Cleared::Done(field)
    }

    /// Insert typed or pasted text at the cursor. Control characters are
    /// dropped, except the line breaks that belong in multiline fields.
    ///
    /// A line break in a one-line field becomes a space: a paste must not
    /// produce a message that the linter then refuses for its shape.
    pub(crate) fn paste(&mut self, text: &str) -> bool {
        if self.active_field() == Field::Type {
            return false;
        }
        let multiline = self.active_field().multiline();
        let mut changed = false;
        for c in text.chars() {
            if c == '\r' {
                continue;
            }
            if c == '\n' && !multiline {
                changed |= self.insert(' ');
                continue;
            }
            if c.is_control() && c != '\n' {
                continue;
            }
            changed |= self.insert(c);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::{Cleared, Form};
    use crate::application::{FIELDS, Field};

    fn form(field: Field) -> Form {
        let mut form = Form::with_kind("feat".to_string());
        form.focus(FIELDS.iter().position(|f| *f == field).unwrap_or(0));
        form
    }

    #[test]
    fn the_type_is_a_choice_and_takes_no_text() {
        let mut form = form(Field::Type);
        assert!(!form.insert('x'), "the type field takes no text");
        assert!(!form.backspace());
        assert_eq!(form.clear(), Cleared::TypeStays);
        assert_eq!(form.commit.kind, "feat");
    }

    #[test]
    fn typing_lands_at_the_cursor_and_backspace_takes_it_back() {
        let mut form = form(Field::Subject);
        form.insert('a');
        form.insert('b');
        form.move_cursor(-2);
        assert!(form.insert('x'));
        assert_eq!(form.value(Field::Subject), "xab");
        assert_eq!(form.cursor(), 1);
        assert!(form.backspace());
        assert_eq!(form.value(Field::Subject), "ab");
        assert_eq!(form.cursor(), 0);
        assert!(!form.backspace(), "nothing left before the cursor");
    }

    #[test]
    fn delete_takes_the_character_at_the_cursor_and_stops_at_the_end() {
        let mut form = form(Field::Subject);
        form.paste("ab");
        form.move_cursor(-2);
        assert!(form.delete());
        assert_eq!(form.value(Field::Subject), "b");
        form.move_cursor(5);
        assert!(!form.delete(), "cursor is at the end");
    }

    #[test]
    fn home_and_end_stay_on_the_current_line() {
        let mut form = form(Field::Body);
        form.paste("one\nsecond\nthree");
        assert_eq!(form.value(Field::Body), "one\nsecond\nthree");
        assert_eq!(form.cursor(), 16);
        form.line_move(-1);
        form.home();
        assert_eq!(form.cursor(), 4, "start of the middle line");
        form.end();
        assert_eq!(form.cursor(), 10, "end of the middle line");
        form.line_move(1);
        form.home();
        assert_eq!(form.cursor(), 11, "start of the last line");
        assert!(!form.line_move(1), "there is no line below");
        assert!(form.line_move(-1), "back into the middle line");
        assert!(form.line_move(-1), "up to the first line");
        assert!(!form.line_move(-1), "there is nothing above the first line");
    }

    #[test]
    fn a_line_break_in_a_one_line_field_becomes_a_space() {
        let mut form = form(Field::Subject);
        form.paste("add the\nthing");
        assert_eq!(form.value(Field::Subject), "add the thing");
    }

    #[test]
    fn a_folded_form_walks_past_the_optional_fields() {
        let mut form = Form::with_kind("feat".to_string());
        form.compact = true;
        form.focus(0);
        form.next_field();
        assert_eq!(form.active_field(), Field::Scope);
        form.next_field();
        assert_eq!(
            form.active_field(),
            Field::Subject,
            "body, breaking and footer are folded away"
        );
    }

    #[test]
    fn cycling_the_type_stops_and_answers_when_there_is_no_list() {
        let mut form = form(Field::Type);
        assert!(!form.cycle_type(&[], true));
        assert_eq!(form.commit.kind, "feat");
        let types = vec!["feat".to_string(), "fix".to_string()];
        assert!(form.cycle_type(&types, true));
        assert_eq!(form.commit.kind, "fix");
        assert!(form.type_touched);
        assert!(form.cycle_type(&types, false));
        assert_eq!(form.commit.kind, "feat");
    }

    #[test]
    fn clearing_tells_the_three_outcomes_apart() {
        let mut form = form(Field::Scope);
        assert_eq!(form.clear(), Cleared::AlreadyEmpty(Field::Scope));
        form.insert('u');
        assert_eq!(form.clear(), Cleared::Done(Field::Scope));
        assert_eq!(form.value(Field::Scope), "");
        assert_eq!(form.cursor(), 0);
    }
}
