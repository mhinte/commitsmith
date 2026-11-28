//! Typing into the commit: cursor, fields, focus and the keys that edit them.
//!
//! The value accessors stay on `App` next to the state they read; everything
//! that moves a cursor or a value lives here.

use super::*;

impl App {
    /// Put the branch's reference back when the footer was emptied.
    ///
    /// `seed_footer` only writes while the footer is empty or still holds an
    /// earlier suggestion, so typed text is never overwritten — this only brings
    /// the suggestion back after Ctrl+K or after deleting the last character.
    fn reseed_empty_footer(&mut self) {
        if self.form.commit.footer.trim().is_empty() {
            self.form.footer_seed = None;
            self.seed_footer();
        }
    }

    /// Ctrl+N: start over with an empty message.
    ///
    /// Ctrl+N used to open the suggestions, which is what Ctrl+Space is for. The
    /// type follows the branch again (as it does on a fresh start) and the footer
    /// is seeded from the branch, so a new commit starts in the state the branch
    /// implies instead of in the state of the last one.
    pub(crate) fn start_new_commit(&mut self) {
        let kind = self
            .config
            .types()
            .first()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        self.form = Form {
            compact: self.form.compact,
            ..Form::with_kind(kind)
        };
        self.form.footer_seed = None;
        self.apply_branch_type();
        self.seed_footer();
        self.focus(FIELDS.iter().position(|f| *f == Field::Type).unwrap_or(0));
        self.revalidate();
        self.note("new commit · everything cleared");
    }

    /// Ctrl+A: load the message of `HEAD`, so Ctrl+S can replace it.
    ///
    /// The raw message goes through the same parser the check uses, so type,
    /// scope, subject, body and footers come back unchanged — including a
    /// `BREAKING CHANGE:` line, which a rendered template would lose.
    pub(crate) fn begin_amend(&mut self) {
        let Some(git) = self.repo.git.as_deref() else {
            self.problem("not a git repository · nothing to amend");
            return;
        };
        match git.head_message() {
            Ok(Some(text)) => {
                let parsed = crate::commit::parse(&text);
                self.form.commit = parsed.commit;
                self.form.amend = true;
                self.form.confirm_amend = false;
                // The type came from the commit, not from the branch: the branch
                // must not override it afterwards.
                self.form.type_touched = true;
                self.focus(
                    FIELDS
                        .iter()
                        .position(|f| *f == Field::Subject)
                        .unwrap_or(0),
                );
                self.revalidate();
                self.note("amend: HEAD message loaded · Ctrl+D replaces that commit");
            }
            Ok(None) => self.problem("no commit to amend yet"),
            Err(error) => self.problem(error.to_string()),
        }
    }

    /// True when `HEAD` is already on the remote: replacing it rewrites published
    /// history, so the user has to say yes.
    pub(crate) fn head_is_published(&self) -> bool {
        self.repo
            .git
            .as_deref()
            .and_then(|git| git.ahead_behind().ok().flatten())
            .is_some_and(|(ahead, _)| ahead == 0)
    }

    pub(crate) fn focus(&mut self, index: usize) {
        self.form.field = index % FIELDS.len();
        let len = self.value(self.active_field()).chars().count();
        self.form.cursors[self.form.field] = self.form.cursors[self.form.field].min(len);
    }

    /// Move to the next field that is actually on screen. Hidden fields keep
    /// their content, so walking past them must not clear or skip anything else.
    pub(crate) fn step_field(&mut self, delta: isize) {
        let len = FIELDS.len() as isize;
        let mut index = self.form.field as isize;
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

    /// A field is on screen unless it is optional and the form is compact.
    pub(crate) fn field_visible(&self, field: Field) -> bool {
        !self.form.compact || !field.optional()
    }

    pub(crate) fn cycle_type(&mut self, forward: bool) {
        let types = self.config.type_names();
        if types.is_empty() {
            return;
        }
        let current = types.iter().position(|kind| *kind == self.form.commit.kind);
        let idx = match current {
            Some(idx) if forward => (idx + 1) % types.len(),
            Some(idx) => (idx + types.len() - 1) % types.len(),
            None => 0,
        };
        self.form.commit.kind.clone_from(&types[idx]);
        self.form.type_touched = true;
        self.revalidate();
    }

    /// `←`/`→`: always the text, on every tab.
    pub(crate) fn arrow(&mut self, key: KeyEvent) {
        let has_text = self.active_field() != Field::Type;
        match navigation::horizontal(key.code, has_text) {
            Some(navigation::Horizontal::Caret(delta)) => self.move_cursor(delta),
            Some(navigation::Horizontal::Value(delta)) => self.cycle_type(delta > 0),
            None => {}
        }
    }

    /// `↑`/`↓`: between fields, or through the lines of a multiline field.
    pub(crate) fn vertical(&mut self, key: KeyEvent) {
        match navigation::vertical(key.code, self.active_field().multiline()) {
            Some(navigation::Vertical::Lines(delta)) => self.line_move(delta),
            Some(navigation::Vertical::Field(delta)) => {
                if delta < 0 {
                    self.previous_field();
                } else {
                    self.next_field();
                }
            }
            None => {}
        }
    }

    pub(crate) fn home(&mut self) {
        let value = self.value(self.active_field()).to_string();
        let cursor = self.form.cursors[self.form.field].min(value.chars().count());
        let chars: Vec<char> = value.chars().collect();
        self.form.cursors[self.form.field] = chars[..cursor]
            .iter()
            .rposition(|c| *c == '\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
    }

    pub(crate) fn end(&mut self) {
        let value = self.value(self.active_field()).to_string();
        let cursor = self.form.cursors[self.form.field].min(value.chars().count());
        let chars: Vec<char> = value.chars().collect();
        self.form.cursors[self.form.field] = chars[cursor..]
            .iter()
            .position(|c| *c == '\n')
            .map(|offset| cursor + offset)
            .unwrap_or(chars.len());
    }

    pub(crate) fn backspace(&mut self) {
        let field = self.active_field();
        if field == Field::Type {
            return;
        }
        let cursor = self.form.cursors[self.form.field];
        if cursor == 0 {
            return;
        }
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        chars.remove(cursor - 1);
        *value = chars.into_iter().collect();
        self.form.cursors[self.form.field] = cursor - 1;
        if field == Field::Footer {
            self.reseed_empty_footer();
        }
        self.revalidate();
    }

    pub(crate) fn delete(&mut self) {
        let field = self.active_field();
        if field == Field::Type {
            return;
        }
        let cursor = self.form.cursors[self.form.field];
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        if cursor < chars.len() {
            chars.remove(cursor);
            *value = chars.into_iter().collect();
            if field == Field::Footer {
                self.reseed_empty_footer();
            }
            self.revalidate();
        }
    }

    /// Ctrl+K: empty the field under the cursor. The type is a choice from the
    /// picker, not text, so it stays.
    pub(crate) fn clear_field(&mut self) {
        let field = self.active_field();
        if field == Field::Type {
            self.status = "the type stays · Space picks another one".to_string();
            return;
        }
        if self.value(field).trim().is_empty() {
            self.note(format!("{} is already empty", field.label()));
            return;
        }
        *self.value_mut(field) = String::new();
        self.form.cursors[self.form.field] = 0;
        if field == Field::Footer {
            self.reseed_empty_footer();
        }
        self.revalidate();
        self.note(format!("cleared {}", field.label()));
    }

    /// Enter inside a multiline field starts a new line, elsewhere it advances.
    pub(crate) fn enter(&mut self) {
        if self.active_field().multiline() {
            self.insert('\n');
        } else if self.active_field() == Field::Type {
            self.open_type_picker();
        } else {
            self.next_field();
        }
    }

    /// True while the active field takes typed characters.
    pub(crate) fn is_editing(&self) -> bool {
        self.picker.is_none() && !self.show_help && self.active_field() != Field::Type
    }

    /// Insert typed or pasted text at the cursor. Control characters are
    /// dropped, except the line breaks that belong in multiline fields.
    pub(crate) fn handle_paste(&mut self, text: &str) {
        if self.active_field() == Field::Type {
            return;
        }
        for c in text.chars() {
            if c == '\r' {
                continue;
            }
            if c == '\n' && !self.active_field().multiline() {
                self.insert(' ');
                continue;
            }
            if c.is_control() && c != '\n' {
                continue;
            }
            self.insert(c);
        }
    }
}
