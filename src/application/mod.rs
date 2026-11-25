//! Application state and the event loop.

//! Application layer: state and the use cases the surface triggers.
//!
//! Key events come in as `crossterm` key codes — that is input, not rendering —
//! but no widget, frame or terminal type appears here.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::commit::Commit;
use crate::config::Config;
use crate::git::{PastCommit, ScopeInfo};
use crate::validate::{self, Issue, Rules, Severity};

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
            Field::Type => "←/→ switch · Space pick",
            Field::Scope => "optional · C-n suggestions",
            Field::Subject => "imperative, no period",
            Field::Body => "why and what, optional",
            Field::Breaking => "what breaks, optional",
            Field::Footer => "Refs: #12, optional",
        }
    }

    /// Fields that accept line breaks.
    pub fn multiline(self) -> bool {
        matches!(self, Field::Body | Field::Breaking | Field::Footer)
    }
}

/// A modal list layered over the form.
pub struct Picker {
    pub kind: PickerKind,
    /// Items as shown, e.g. `feat  ✨  a new feature`.
    pub items: Vec<String>,
    /// Raw values behind the items, e.g. `feat`.
    pub values: Vec<String>,
    pub selected: usize,
    pub filter: String,
    /// Text shown next to the list while an entry is highlighted.
    pub previews: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Type,
    Scope,
    Profile,
    /// Recent commits, reuseable as a template.
    History,
}

impl Picker {
    pub fn filtered(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| needle.is_empty() || item.to_lowercase().contains(&needle))
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn current(&self) -> Option<&String> {
        let visible = self.filtered();
        visible
            .get(self.selected.min(visible.len().saturating_sub(1)))
            .and_then(|idx| self.values.get(*idx))
    }

    /// Preview text of the highlighted entry, if the picker has any.
    pub fn preview(&self) -> Option<&str> {
        let visible = self.filtered();
        let idx = *visible.get(self.selected.min(visible.len().saturating_sub(1)))?;
        self.previews.get(idx).map(|text| text.as_str())
    }

    /// `1`..`9` jump straight to that entry, `0` to the last one. Only while the
    /// filter is empty, otherwise digits belong to the search term.
    fn by_digit(&self, digit: char) -> Option<usize> {
        if !self.filter.is_empty() || !digit.is_ascii_digit() {
            return None;
        }
        let visible = self.filtered();
        let index = match digit.to_digit(10)? {
            0 => visible.len().checked_sub(1)?,
            n => (n as usize).checked_sub(1)?,
        };
        visible.get(index).copied()
    }

    fn move_by(&mut self, delta: isize) {
        let len = self.filtered().len();
        if len == 0 {
            return;
        }
        let next = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            (self.selected + delta as usize).min(len - 1)
        };
        self.selected = next;
    }
}

pub struct App {
    pub config: Config,
    /// Branch and staged file summary of the repository, when there is one.
    pub repo_line: String,
    pub rules: Rules,
    pub scopes: ScopeInfo,
    pub commit: Commit,
    pub field: usize,
    /// Cursor position per field, counted in characters, not bytes.
    cursors: [usize; FIELDS.len()],
    pub issues: Vec<Issue>,
    pub picker: Option<Picker>,
    /// Recent commits, offered by the history picker.
    pub history: Vec<PastCommit>,
    pub status: String,
    /// Set when the user asks to hand the message to git.
    pub output: Option<String>,
    /// True when the program was started with `--commit`, so Ctrl+S really
    /// commits. The footer says which of the two happens.
    pub commit_mode: bool,
    pub should_quit: bool,
    pub show_help: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::with_config(Config::conventional(), Vec::new())
    }
}

impl App {
    pub fn with_config(config: Config, warnings: Vec<String>) -> Self {
        let rules = crate::validate::rules_from_config(&config);
        let scopes = crate::git::empty_scope_info(&config.scopes);
        let kind = config
            .types()
            .first()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        let mut app = Self {
            commit: Commit {
                kind,
                ..Default::default()
            },
            config,
            repo_line: String::new(),
            rules,
            scopes,
            field: 0,
            cursors: [0; FIELDS.len()],
            issues: Vec::new(),
            picker: None,
            history: Vec::new(),
            status: warnings.join(" · "),
            output: None,
            commit_mode: false,
            should_quit: false,
            show_help: false,
        };
        app.revalidate();
        app.seed_footer();
        app
    }

    /// Suggested footer for a new, empty message: the prefix identifies the
    /// author in shared repositories.
    pub fn suggested_footer(&self) -> Option<String> {
        self.config
            .prefix
            .as_deref()
            .map(|prefix| format!("Co-authored-by: {prefix}"))
    }

    /// Fill the footer with the suggestion while it is still untouched, so the
    /// user can delete it like any other text.
    pub fn seed_footer(&mut self) {
        if !self.commit.footer.trim().is_empty() {
            return;
        }
        if let Some(footer) = self.suggested_footer() {
            self.commit.footer = footer;
            self.cursors[FIELDS.iter().position(|f| *f == Field::Footer).unwrap_or(0)] =
                self.commit.footer.chars().count();
            self.revalidate();
        }
    }

    /// What the header shows about the repository on the right.
    pub fn set_repo_line(&mut self, line: String) {
        self.repo_line = line;
    }

    /// Replace the available scopes, usually after reading git history.
    pub fn set_scopes(&mut self, scopes: ScopeInfo) {
        self.scopes = scopes;
    }

    /// Replace the list of recent commits offered as templates.
    pub fn set_history(&mut self, history: Vec<PastCommit>) {
        self.history = history;
    }

    /// Tell the UI whether Ctrl+S commits or only prints.
    pub fn set_commit_mode(&mut self, commit: bool) {
        self.commit_mode = commit;
    }

    /// True while the active field takes typed characters.
    pub fn is_editing(&self) -> bool {
        self.picker.is_none() && !self.show_help && self.active_field() != Field::Type
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
        self.issues = validate::validate_with(&self.commit, &self.rules);
    }

    /// Name, emoji and description of the current type, for the header.
    pub fn type_label(&self) -> String {
        let commit = &self.commit;
        let found = self.config.types().iter().find(|t| t.name == commit.kind);
        match found {
            Some(t) if self.config.emoji => match &t.emoji {
                Some(emoji) => format!("{emoji} {:<9} {}", t.name, t.description),
                None => format!("  {:<9} {}", t.name, t.description),
            },
            Some(t) => format!("  {:<9} {}", t.name, t.description),
            None => format!("  {}", commit.kind),
        }
    }

    /// One-line hint shown in narrow terminals.
    pub fn field_hint(&self) -> &'static str {
        self.active_field().hint()
    }

    fn cycle_type(&mut self, forward: bool) {
        let types = self.config.type_names();
        if types.is_empty() {
            return;
        }
        let current = types.iter().position(|kind| *kind == self.commit.kind);
        let idx = match current {
            Some(idx) if forward => (idx + 1) % types.len(),
            Some(idx) => (idx + types.len() - 1) % types.len(),
            None => 0,
        };
        self.commit.kind = types[idx].clone();
        self.revalidate();
    }

    fn open_type_picker(&mut self) {
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

    fn open_scope_picker(&mut self) {
        // Offer what the user already typed in the scope field first.
        let typed = self.commit.scope.trim().to_string();
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
            self.status = "no scopes yet: type one, it is remembered per project".into();
            return;
        }
        self.open_picker(PickerKind::Scope, items, values);
    }

    /// Choose a profile by name, which also swaps the type list.
    fn open_profile_picker(&mut self) {
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
    fn open_history_picker(&mut self) {
        if self.history.is_empty() {
            self.status = "no commits yet: this repository has no history to reuse".into();
            return;
        }
        let items: Vec<String> = self.history.iter().map(|commit| commit.label(60)).collect();
        let values: Vec<String> = self
            .history
            .iter()
            .map(|commit| commit.id.clone())
            .collect();
        let previews: Vec<String> = self.history.iter().map(|commit| commit.preview()).collect();
        // Start on the newest conventional commit: a plain message has no
        // type or scope to reuse.
        let selected = self
            .history
            .iter()
            .position(|commit| commit.conventional())
            .unwrap_or(0);
        self.picker = Some(Picker {
            kind: PickerKind::History,
            items,
            values,
            selected,
            filter: String::new(),
            previews,
        });
    }

    /// Take type, scope and body from a past commit, keeping what is already
    /// typed in the subject and in the footer.
    fn apply_template(&mut self, id: &str) {
        let Some(past) = self.history.iter().find(|commit| commit.id == id) else {
            self.status = format!("commit {id} is not in the cached history");
            return;
        };
        let template = past.template();
        let mut applied = Vec::new();

        if let Some(kind) = template.kind {
            if self.rules.types.is_empty() || self.rules.types.contains(&kind) {
                applied.push("type");
                self.commit.kind = kind;
            }
        }
        if let Some(scope) = template.scope {
            if !scope.is_empty() {
                applied.push("scope");
                self.commit.scope = scope;
            }
        }
        if !template.body.is_empty() {
            applied.push("body");
            self.commit.body = template.body;
        }

        if applied.is_empty() {
            self.status = format!("{} reused: nothing to copy", past.short_id);
        } else {
            self.status = format!("{} reused: {}", past.short_id, applied.join(", "));
        }
        self.focus(
            FIELDS
                .iter()
                .position(|f| *f == Field::Subject)
                .unwrap_or(0),
        );
        self.revalidate();
    }

    fn open_picker(&mut self, kind: PickerKind, items: Vec<String>, values: Vec<String>) {
        let current = match kind {
            PickerKind::Profile => self.config.profile.clone(),
            _ => self.commit.kind.clone(),
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

    fn confirm_picker(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        let Some(choice) = picker.current().cloned() else {
            self.status = "nothing selected".to_string();
            return;
        };
        self.apply_picker_choice(picker.kind, choice);
    }

    fn apply_picker_choice(&mut self, kind: PickerKind, choice: String) {
        match kind {
            PickerKind::Type => {
                self.commit.kind = choice;
                self.next_field();
            }
            PickerKind::Scope => self.commit.scope = choice,
            PickerKind::Profile => {
                self.config.profile = choice;
                self.rules = crate::validate::rules_from_config(&self.config);
                if !self.config.type_names().contains(&self.commit.kind) {
                    self.commit.kind = self
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

    fn picker_key(&mut self, key: KeyEvent) -> bool {
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

    /// Ctrl+S. Errors keep the composer open: committing `feat:` because the
    /// subject was forgotten is a mistake the linter exists to prevent.
    fn finish(&mut self) {
        if let Some(issue) = self
            .issues
            .iter()
            .find(|issue| issue.severity == Severity::Error)
        {
            let more = self
                .issues
                .iter()
                .filter(|issue| issue.severity == Severity::Error)
                .count()
                .saturating_sub(1);
            self.status = if more > 0 {
                format!(
                    "not yet: {} · {} more error(s) on the right",
                    issue.message, more
                )
            } else {
                format!("not yet: {}", issue.message)
            };
            return;
        }
        self.output = Some(self.commit.render());
        self.should_quit = true;
        self.status = "handing the message over".to_string();
    }

    /// Ctrl+K: empty the field under the cursor. The type is a choice from the
    /// picker, not text, so it stays.
    fn clear_field(&mut self) {
        let field = self.active_field();
        if field == Field::Type {
            self.status = "the type stays · Space picks another one".to_string();
            return;
        }
        if self.value(field).trim().is_empty() {
            self.status = format!("{} is already empty", field.label());
            return;
        }
        *self.value_mut(field) = String::new();
        self.cursors[self.field] = 0;
        self.revalidate();
        self.status = format!("cleared {}", field.label());
    }

    /// Enter inside a multiline field starts a new line, elsewhere it advances.
    fn enter(&mut self) {
        if self.active_field().multiline() {
            self.insert('\n');
        } else if self.active_field() == Field::Type {
            self.open_type_picker();
        } else {
            self.next_field();
        }
    }

    /// Insert typed or pasted text at the cursor. Control characters are
    /// dropped, except the line breaks that belong in multiline fields.
    pub fn handle_paste(&mut self, text: &str) {
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

    /// `↑`/`↓` inside a multiline field stay on the line where possible, and
    /// move to the neighbouring field at the first or last line.
    fn line_move(&mut self, delta: isize) {
        let field = self.active_field();
        if !field.multiline() {
            if delta < 0 {
                self.previous_field();
            } else {
                self.next_field();
            }
            return;
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

        if delta < 0 {
            if line_start == 0 {
                self.previous_field();
                return;
            }
            let prev_start = chars[..line_start - 1]
                .iter()
                .rposition(|c| *c == '\n')
                .map(|idx| idx + 1)
                .unwrap_or(0);
            let prev_len = line_start - 1 - prev_start;
            self.cursors[self.field] = prev_start + column.min(prev_len);
        } else {
            let rest: Vec<char> = chars[cursor.min(chars.len())..].to_vec();
            match rest.iter().position(|c| *c == '\n') {
                Some(offset) => {
                    let line_end = cursor + offset;
                    let next_start = line_end + 1;
                    let next_len = chars[next_start..]
                        .iter()
                        .position(|c| *c == '\n')
                        .unwrap_or(chars.len() - next_start);
                    self.cursors[self.field] = next_start + column.min(next_len);
                }
                None => self.next_field(),
            }
        }
    }

    fn home(&mut self) {
        let value = self.value(self.active_field()).to_string();
        let cursor = self.cursors[self.field].min(value.chars().count());
        let chars: Vec<char> = value.chars().collect();
        self.cursors[self.field] = chars[..cursor]
            .iter()
            .rposition(|c| *c == '\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
    }

    fn end(&mut self) {
        let value = self.value(self.active_field()).to_string();
        let cursor = self.cursors[self.field].min(value.chars().count());
        let chars: Vec<char> = value.chars().collect();
        self.cursors[self.field] = chars[cursor..]
            .iter()
            .position(|c| *c == '\n')
            .map(|offset| cursor + offset)
            .unwrap_or(chars.len());
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.picker_key(key) {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if self.show_help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.show_help = false,
                KeyCode::Char('c') if ctrl => self.should_quit = true,
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('c') | KeyCode::Char('q') if ctrl => self.should_quit = true,
            KeyCode::Char('s') if ctrl => self.finish(),
            KeyCode::Char('k') if ctrl => self.clear_field(),
            KeyCode::Char('n') if ctrl => self.open_scope_picker(),
            KeyCode::Char('t') if ctrl => self.open_type_picker(),
            KeyCode::Char('p') if ctrl => self.open_profile_picker(),
            KeyCode::Char('r') if ctrl => self.open_history_picker(),
            KeyCode::Char('q') if self.active_field() == Field::Type => self.should_quit = true,
            KeyCode::Tab => self.next_field(),
            KeyCode::BackTab => self.previous_field(),
            KeyCode::Enter => self.enter(),
            KeyCode::Up => self.line_move(-1),
            KeyCode::Down => self.line_move(1),
            KeyCode::Left | KeyCode::Char('h') if self.active_field() == Field::Type => {
                self.cycle_type(false);
            }
            KeyCode::Right | KeyCode::Char('l') if self.active_field() == Field::Type => {
                self.cycle_type(true);
            }
            KeyCode::Char('j') if self.active_field() == Field::Type => self.next_field(),
            KeyCode::Char('k') if self.active_field() == Field::Type => self.previous_field(),
            KeyCode::Char(' ') if self.active_field() == Field::Type => self.open_type_picker(),
            KeyCode::Left => self.move_cursor(-1),
            KeyCode::Right => self.move_cursor(1),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Char(c) if !ctrl => self.insert(c),
            _ => {}
        }
    }
}

enum PickerAction {
    Cancel,
    History,
    Confirm,
    Move(isize),
    Backspace,
    Filter(char),
    Pick(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
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
        assert_eq!(
            app.commit.kind, "security",
            "wraps around the full type list"
        );
    }

    #[test]
    fn space_opens_the_type_picker() {
        let mut app = App::default();
        press(&mut app, KeyCode::Char(' '));
        assert!(app.picker.is_some());
        type_text(&mut app, "refak");
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.commit.kind, "refactor");
        assert_eq!(app.active_field(), Field::Scope);
    }

    #[test]
    fn digit_selects_the_nth_entry() {
        let mut app = App::default();
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.commit.kind, "fix");
        assert!(app.picker.is_none());
        assert_eq!(app.active_field(), Field::Scope);

        press(&mut app, KeyCode::BackTab);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('0'));
        assert_eq!(app.commit.kind, "security", "0 selects the last entry");
    }

    #[test]
    fn digits_filter_once_a_filter_is_typed() {
        let mut app = App::default();
        press(&mut app, KeyCode::Char(' '));
        type_text(&mut app, "b");
        press(&mut app, KeyCode::Char('2'));
        let picker = app.picker.as_ref().expect("picker open");
        assert_eq!(picker.filter, "b2");
        assert!(picker.current().is_none(), "no entry matches b2");
    }

    #[test]
    fn picker_filter_narrows_the_list() {
        let mut app = App::default();
        press(&mut app, KeyCode::Char(' '));
        type_text(&mut app, "er");
        let visible = app.picker.as_ref().map(|p| p.filtered().len()).unwrap();
        assert!(visible >= 1);
        press(&mut app, KeyCode::Esc);
        assert!(app.picker.is_none());
        assert_eq!(app.commit.kind, "feat");
    }

    #[test]
    fn ctrl_n_offers_scopes_from_history() {
        let mut app = App::default();
        app.set_scopes(ScopeInfo {
            history: vec!["api".into(), "ui".into()],
            configured: Vec::new(),
        });
        ctrl(&mut app, 'n');
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.commit.scope, "ui");
    }

    fn past(id: &str, kind: &str, scope: Option<&str>, summary: &str, body: &str) -> PastCommit {
        PastCommit {
            short_id: id[..7].to_string(),
            id: id.to_string(),
            author: "Marc".into(),
            date: "2026-11-24".into(),
            summary: summary.into(),
            body: body.into(),
            kind: Some(kind.into()),
            scope: scope.map(str::to_string),
            files: vec!["M src/app.rs".into()],
        }
    }

    fn app_with_history() -> App {
        let mut app = App::default();
        app.set_history(vec![
            past(
                "aaaaaaa1111111",
                "feat",
                Some("composer"),
                "add picker preview",
                "The picker previews the highlighted commit.",
            ),
            past("bbbbbbb2222222", "fix", None, "stop the crash", ""),
        ]);
        app
    }

    #[test]
    fn history_picker_carries_previews() {
        let mut app = app_with_history();
        ctrl(&mut app, 'r');
        let picker = app.picker.as_ref().expect("history picker open");
        assert_eq!(picker.kind, PickerKind::History);
        assert_eq!(picker.items.len(), 2);
        assert!(picker.items[0].contains("feat(composer)"));
        assert!(
            picker
                .preview()
                .unwrap()
                .contains("previews the highlighted")
        );
        press(&mut app, KeyCode::Down);
        let picker = app.picker.as_ref().unwrap();
        assert!(picker.preview().unwrap().starts_with("fix: stop the crash"));
    }

    #[test]
    fn history_without_commits_reports_instead_of_opening() {
        let mut app = App::default();
        ctrl(&mut app, 'r');
        assert!(app.picker.is_none());
        assert!(app.status.contains("no commits yet"));
    }

    #[test]
    fn confirming_history_reuses_type_scope_and_body() {
        let mut app = app_with_history();
        app.commit.subject = "already typed".into();
        ctrl(&mut app, 'r');
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.commit.kind, "feat");
        assert_eq!(app.commit.scope, "composer");
        assert!(app.commit.body.contains("previews the highlighted"));
        assert_eq!(app.commit.subject, "already typed", "subject stays");
        assert_eq!(app.active_field(), Field::Subject);
        assert!(app.status.contains("reused: type, scope, body"));
    }

    #[test]
    fn history_filter_finds_a_commit_by_text() {
        let mut app = app_with_history();
        ctrl(&mut app, 'r');
        type_text(&mut app, "crash");
        assert_eq!(app.picker.as_ref().unwrap().filtered().len(), 1);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.commit.kind, "fix");
        assert_eq!(app.commit.scope, "", "plain commit keeps the scope empty");
    }

    #[test]
    fn unknown_types_from_history_are_not_applied() {
        let mut app = App::default();
        app.set_history(vec![past("ccccccc3333333", "wip", None, "scratch", "")]);
        ctrl(&mut app, 'r');
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.commit.kind, "feat", "wip is not in the profile");
        assert!(app.status.contains("nothing to copy"));
    }

    #[test]
    fn ctrl_p_switches_profile_and_types() {
        let mut app = App::default();
        ctrl(&mut app, 'p');
        type_text(&mut app, "ang");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.config.profile, "angular");
        assert_eq!(app.commit.kind, app.config.type_names()[0]);
        ctrl(&mut app, 'p');
        type_text(&mut app, "atomic");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.config.profile, "atomic");
    }

    #[test]
    fn typing_lands_in_the_active_field() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "api");
        assert_eq!(app.commit.scope, "api");
        assert_eq!(app.cursor(), 3);
    }

    #[test]
    fn backspace_and_delete_edit_around_the_cursor() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "abc");
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.commit.scope, "ac");
        press(&mut app, KeyCode::Delete);
        assert_eq!(app.commit.scope, "a");
    }

    #[test]
    fn enter_in_body_starts_a_new_line() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "first");
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "second");
        assert_eq!(app.commit.body, "first\nsecond");
        press(&mut app, KeyCode::Up);
        assert_eq!(app.cursor(), 5);
    }

    #[test]
    fn ctrl_s_renders_the_message() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "add composer");
        ctrl(&mut app, 's');
        assert_eq!(app.output.as_deref(), Some("feat: add composer"));
        assert!(app.should_quit);
    }

    #[test]
    fn ctrl_k_clears_only_the_field_under_the_cursor() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "api");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "a subject");
        // Back to the scope, then clear it.
        press(&mut app, KeyCode::BackTab);
        ctrl(&mut app, 'k');
        assert_eq!(app.value(Field::Scope), "");
        assert_eq!(app.cursor(), 0);
        assert_eq!(app.value(Field::Subject), "a subject", "other fields stay");
        assert!(app.status.contains("cleared"));
    }

    #[test]
    fn ctrl_k_leaves_the_type_alone() {
        let mut app = App::default();
        let before = app.value(Field::Type).to_string();
        ctrl(&mut app, 'k');
        assert_eq!(app.value(Field::Type), before);
        assert!(app.status.contains("type stays"));
    }

    #[test]
    fn ctrl_k_on_an_empty_field_says_so() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        ctrl(&mut app, 'k');
        assert!(app.status.contains("already empty"), "{}", app.status);
    }

    #[test]
    fn ctrl_s_refuses_to_finish_with_errors() {
        let mut app = App::default();
        // 0.3.2 rendered "feat:" for an empty subject and committed it.
        ctrl(&mut app, 's');
        assert!(
            app.output.is_none(),
            "an invalid message must not leave the app"
        );
        assert!(!app.should_quit);
        assert!(app.status.starts_with("not yet:"), "{}", app.status);
        assert!(app.status.contains("subject"), "{}", app.status);
    }

    #[test]
    fn ctrl_s_finishes_after_clearing_the_error() {
        let mut app = App::default();
        ctrl(&mut app, 's');
        assert!(app.output.is_none());
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "add composer");
        ctrl(&mut app, 's');
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
    fn help_overlay_swallows_keys_until_closed() {
        let mut app = App::default();
        press(&mut app, KeyCode::Char('?'));
        assert!(app.show_help);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.active_field(), Field::Type);
        press(&mut app, KeyCode::Esc);
        assert!(!app.show_help);
        assert!(!app.should_quit, "Esc closes the overlay first");
    }

    #[test]
    fn validation_follows_the_input() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "Added stuff.");
        assert!(
            app.issues
                .iter()
                .any(|issue| issue.message.contains("imperative"))
        );
    }

    #[test]
    fn footer_is_seeded_with_the_author_handle() {
        let mut config = Config::conventional();
        config.user = Some("Marc Hinte".into());
        config.prefix = Some("hima".into());
        let app = App::with_config(config, Vec::new());
        assert_eq!(app.commit.footer, "Co-authored-by: hima");
    }

    #[test]
    fn no_handle_means_no_footer_noise() {
        let app = App::default();
        assert_eq!(app.commit.footer, "");
        assert_eq!(app.suggested_footer(), None);
    }

    #[test]
    fn custom_profile_types_are_accepted() {
        let mut config = Config::conventional();
        config.profile = "atomic".into();
        let mut app = App::with_config(config, Vec::new());
        app.commit.subject = "add thing".into();
        assert!(!app.issues.iter().any(|i| i.field == "type"));
    }
}
