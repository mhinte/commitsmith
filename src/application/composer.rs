//! The keys that edit the commit, and what the form change means for the screen.
//!
//! The editing itself lives on [`Form`] (`application/form.rs`) and answers
//! whether it changed something; this file only adds what needs the rest of the
//! program — revalidating the message, seeding the footer from the branch, and
//! writing the status line.

use super::form::Cleared;
use super::*;

impl App {
    /// Put the branch's reference back when the footer was emptied.
    ///
    /// `seed_footer` only writes while the footer is empty or still holds an
    /// earlier suggestion, so typed text is never overwritten — this only brings
    /// the suggestion back after Ctrl+U or after deleting the last character.
    fn reseed_empty_footer(&mut self) {
        if self.form.commit.footer.trim().is_empty() {
            self.form.footer_seed = None;
            self.seed_footer();
        }
    }

    /// Ctrl+N: start over with an empty message.
    ///
    /// The type follows the branch again (as it does on a fresh start) and the
    /// footer is seeded from the branch, so a new commit starts in the state the
    /// branch implies instead of in the state of the last one.
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

    /// Ctrl+A: load the message of `HEAD`, so Ctrl+D can replace it.
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
        self.form.focus(index);
    }

    pub fn active_field(&self) -> Field {
        self.form.active_field()
    }

    /// Cursor of the active field.
    pub fn cursor(&self) -> usize {
        self.form.cursor()
    }

    /// Value of a field, for rendering.
    pub fn value(&self, field: Field) -> &str {
        self.form.value(field)
    }

    pub(crate) fn move_cursor(&mut self, delta: isize) {
        self.form.move_cursor(delta);
    }

    /// `↑`/`↓`: inside a multiline field the text decides; at its edge the
    /// walk to the neighbouring field does.
    fn line_move(&mut self, delta: isize) {
        if self.form.line_move(delta) {
            return;
        }
        if delta < 0 {
            self.previous_field();
        } else {
            self.next_field();
        }
    }

    pub(crate) fn next_field(&mut self) {
        self.form.next_field();
    }

    pub(crate) fn previous_field(&mut self) {
        self.form.previous_field();
    }

    pub(crate) fn field_visible(&self, field: Field) -> bool {
        self.form.field_visible(field)
    }

    pub(crate) fn cycle_type(&mut self, forward: bool) {
        let types = self.config.type_names();
        if self.form.cycle_type(&types, forward) {
            self.revalidate();
        }
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
        self.form.home();
    }

    pub(crate) fn end(&mut self) {
        self.form.end();
    }

    pub(crate) fn backspace(&mut self) {
        if self.form.backspace() {
            if self.active_field() == Field::Footer {
                self.reseed_empty_footer();
            }
            self.revalidate();
        }
    }

    pub(crate) fn delete(&mut self) {
        if self.form.delete() {
            if self.active_field() == Field::Footer {
                self.reseed_empty_footer();
            }
            self.revalidate();
        }
    }

    /// Ctrl+U: empty the field under the cursor, and say what happened.
    pub(crate) fn clear_field(&mut self) {
        match self.form.clear() {
            Cleared::TypeStays => {
                self.status = "the type stays · Space picks another one".to_string();
            }
            Cleared::AlreadyEmpty(field) => {
                self.note(format!("{} is already empty", field.label()));
            }
            Cleared::Done(field) => {
                if field == Field::Footer {
                    self.reseed_empty_footer();
                }
                self.revalidate();
                self.note(format!("cleared {}", field.label()));
            }
        }
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

    /// Insert typed or pasted text at the cursor.
    pub(crate) fn handle_paste(&mut self, text: &str) {
        if self.form.paste(text) {
            if self.active_field() == Field::Footer {
                self.reseed_empty_footer();
            }
            self.revalidate();
        }
    }

    pub(crate) fn insert(&mut self, c: char) {
        if self.form.insert(c) {
            self.revalidate();
        }
    }
}
