//! The pickers: type, scope, profile and history.
//!
//! They share the overlay, the digit shortcuts and the filter, so they live in
//! one file instead of being spread over the application module.

use super::*;

impl App {
    pub(crate) fn open_type_picker(&mut self) {
        let mut items = Vec::new();
        let mut values = Vec::new();
        for kind in self.config.types() {
            let emoji = if self.config.emoji {
                kind.emoji.clone().unwrap_or_else(|| "  ".to_string())
            } else {
                "  ".to_string()
            };
            items.push(format!("{:<12} {} {}", kind.name, emoji, kind.description));
            values.push(kind.name.clone());
        }
        self.open_picker(PickerKind::Type, items, values);
    }

    pub(crate) fn open_scope_picker(&mut self) {
        // Offer what the user already typed in the scope field first.
        let typed = self.form.commit.scope.trim().to_string();
        let mut items = Vec::new();
        let mut values = Vec::new();
        for scope in self.scopes.matching(&typed) {
            let origin = if self.config.scopes.contains(&scope) {
                "config"
            } else {
                "history"
            };
            items.push(format!("{:<16} {}", scope, origin));
            values.push(scope);
        }
        if values.is_empty() {
            self.status = match typed.is_empty() {
                true => "no scopes yet: type one, it is remembered per project".to_string(),
                false => format!("no scope matches {typed}"),
            };
            return;
        }
        // Say what the list is filtered by, so a short list is not mistaken for
        // the whole set.
        let count = values.len();
        self.open_picker(PickerKind::Scope, items, values);
        if !typed.is_empty() {
            self.status = format!("scopes matching {typed} · {count}");
        }
    }

    /// Choose a profile by name, which also swaps the type list.
    pub(crate) fn open_profile_picker(&mut self) {
        let mut items = Vec::new();
        let mut values = Vec::new();
        for profile in self.config.profiles.values() {
            let description = profile.description.clone().unwrap_or_default();
            items.push(format!("{:<14} {}", profile.name, description));
            values.push(profile.name.clone());
        }
        self.open_picker(PickerKind::Profile, items, values);
    }

    /// Recent commits, with their message shown in the pane next to the list.
    pub(crate) fn open_history_picker(&mut self) {
        if self.history.commits.is_empty() {
            self.status = "no commits yet: this repository has no history to reuse".into();
            return;
        }
        let items: Vec<String> = self
            .history
            .commits
            .iter()
            .map(|commit| commit.label(60))
            .collect();
        let values: Vec<String> = self
            .history
            .commits
            .iter()
            .map(|commit| commit.id.clone())
            .collect();
        let previews: Vec<String> = self
            .history
            .commits
            .iter()
            .map(commitsmith::domain::history::PastCommit::preview)
            .collect();
        // Start on the newest conventional commit: a plain message has no
        // type or scope to reuse.
        let selected = self
            .history
            .commits
            .iter()
            .position(commitsmith::domain::history::PastCommit::conventional)
            .unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::History,
            items,
            values,
            selected,
            filter: String::new(),
            previews,
        });
        // The preview for the highlighted commit gets its file list now; the
        // rest is fetched when the selection moves there.
        self.refresh_history_preview();
    }

    /// Replace the whole form with a past commit.
    ///
    /// Every field comes from the reused commit, not just the ones that are still
    /// empty: an old scope or body used to survive the reuse, and a subject that
    /// was already typed blocked the one being reused. `breaking` is cleared
    /// because the text of a `BREAKING CHANGE:` footer does not survive the walk
    /// (`clean_body` drops footers); the footer itself is seeded again from the
    /// current branch, which is also how an old ticket reference disappears.
    pub(crate) fn apply_template(&mut self, id: &str) {
        let Some(past) = self.history.commits.iter().find(|commit| commit.id == id) else {
            self.status = format!("commit {id} is not in the cached history");
            return;
        };
        let template = past.template();
        let mut applied = Vec::new();

        if let Some(kind) = template.kind {
            if self.rules.types.is_empty() || self.rules.types.contains(&kind) {
                applied.push("type");
                self.form.commit.kind = kind;
                self.form.type_touched = true;
            }
        }
        applied.push("scope");
        self.form.commit.scope = template.scope.unwrap_or_default();
        // Subject and body come from the commit, not from a rendered template:
        // the template carries the whole message, which used to land in the body
        // while the subject stayed empty.
        applied.push("subject");
        self.form.commit.subject = past.summary.clone();
        applied.push("body");
        self.form.commit.body = past.body.clone();
        self.form.commit.breaking.clear();

        self.status = format!("{} reused: {}", past.short_id, applied.join(", "));
        self.focus(
            FIELDS
                .iter()
                .position(|f| *f == Field::Subject)
                .unwrap_or(0),
        );
        // The footer belongs to the work in progress, not to the reused commit:
        // drop what was there (including its ticket reference) and seed from the
        // branch again.
        self.form.commit.footer.clear();
        self.form.footer_seed = None;
        self.seed_footer();
        self.revalidate();
    }

    pub(crate) fn open_picker(
        &mut self,
        kind: PickerKind,
        items: Vec<String>,
        values: Vec<String>,
    ) {
        let current = match kind {
            PickerKind::Profile => self.config.profile.clone(),
            _ => self.form.commit.kind.clone(),
        };
        let selected = values
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0);
        let previews = vec![String::new(); values.len()];
        self.picker = Some(Picker {
            kind,
            items,
            values,
            selected,
            filter: String::new(),
            previews,
        });
    }

    pub(crate) fn apply_picker_choice(&mut self, kind: PickerKind, choice: String) {
        match kind {
            PickerKind::Type => {
                self.form.commit.kind = choice;
                self.form.type_touched = true;
                self.next_field();
            }
            PickerKind::Scope => self.form.commit.scope = choice,
            PickerKind::Profile => {
                self.config.profile = choice;
                self.rules = crate::validate::rules_from_config(&self.config);
                // The log's marks were judged against the old rules.
                self.history.rebuild_marks(&self.rules);
                if !self.config.type_names().contains(&self.form.commit.kind) {
                    self.form.commit.kind = self
                        .config
                        .type_names()
                        .first()
                        .cloned()
                        .unwrap_or_default();
                }
                self.revalidate();
            }
            PickerKind::History => self.apply_template(&choice),
        }
    }

    pub(crate) fn picker_key(&mut self, key: KeyEvent) -> bool {
        if self.picker.is_none() {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let action = {
            let picker = self.picker.as_ref().expect("checked above");
            match key.code {
                KeyCode::Esc => Some(PickerAction::Cancel),
                KeyCode::Enter => Some(PickerAction::Confirm),
                KeyCode::Up => Some(PickerAction::Move(-1)),
                KeyCode::Down | KeyCode::Tab => Some(PickerAction::Move(1)),
                KeyCode::Backspace => Some(PickerAction::Backspace),
                KeyCode::Char('r') if ctrl => Some(PickerAction::History),
                KeyCode::Char(c) if !ctrl => match picker.by_digit(c) {
                    Some(value_idx) => Some(PickerAction::Pick(value_idx)),
                    None => Some(PickerAction::Filter(c)),
                },
                _ => None,
            }
        };

        match action {
            Some(PickerAction::Cancel) => self.picker = None,
            Some(PickerAction::History) => self.open_history_picker(),
            Some(PickerAction::Confirm) => self.confirm_picker(),
            Some(PickerAction::Move(delta)) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.move_by(delta);
                }
                self.refresh_history_preview();
            }
            Some(PickerAction::Backspace) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.filter.pop();
                    picker.selected = 0;
                }
            }
            Some(PickerAction::Filter(c)) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.filter.push(c);
                    picker.selected = 0;
                }
            }
            Some(PickerAction::Pick(value_idx)) => {
                let picker = self.picker.take().expect("checked above");
                if let Some(value) = picker.values.get(value_idx).cloned() {
                    self.apply_picker_choice(picker.kind, value);
                }
            }
            None => {}
        }
        true
    }
}
