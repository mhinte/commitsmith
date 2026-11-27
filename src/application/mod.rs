//! Application layer: state, tabs and the use cases the surface triggers.
//!
//! Key events come in as `crossterm` key codes — that is input, not rendering —
//! but no widget, frame or terminal type appears here.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub mod branch_wizard;
pub mod branches;
pub mod branches_view;
pub mod composer;
pub mod form;
pub mod history_view;
pub mod log;
pub mod navigation;
pub mod picker;
pub mod release_cmds;
pub mod release_view;
mod repo;
pub mod staging;
mod sync;
pub mod tabs;
#[cfg(test)]
pub mod test_support;

use std::collections::HashMap;

use crate::commit::Commit;

use branches_view::BranchesView;
use form::Form;
use history_view::History;
use release_view::Release;
use repo::RepoState;

/// Reads a commit's file list when the history preview needs it.
use crate::config::Config;
use crate::git::{PastCommit, ScopeInfo};
use crate::validate::{self, Issue, Rules, Severity};
use branch_wizard::BranchWizard;
use branches::BranchList;
use commitsmith::domain::branch::ConventionalBranchName;
use commitsmith::domain::ports::{ConfigStore, GitRepository};
use log::LogState;
use staging::{Confirm, Staging};

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
    /// Body, breaking and footer can be folded away; type, scope and subject
    /// carry the commit and are always on screen.
    pub fn optional(self) -> bool {
        matches!(self, Field::Body | Field::Breaking | Field::Footer)
    }

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
            Field::Scope => "optional · C-Space suggestions",
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
    /// What accepting an entry does. A suggestion fills the field it was opened
    /// from; Ctrl+R replaces the whole form. Same list, two meanings — choosing
    /// a subject used to wipe the scope that was already typed.
    pub full_reuse: bool,
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
        self.previews.get(idx).map(std::string::String::as_str)
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

fn plural(count: usize) -> &'static str {
    commitsmith::domain::text::plural_suffix(count)
}

/// Git says what happened in its own words; the footer has room for one line that
/// says what to do about it.
fn explain_sync_error(
    error: &commitsmith::domain::errors::GitError,
    pull: bool,
    branch: &str,
) -> String {
    let text = error.to_string();
    let lower = text.to_lowercase();
    if !pull
        && (lower.contains("non-fast-forward")
            || lower.contains("rejected")
            || lower.contains("fetch first"))
    {
        return format!("push rejected · {branch} is behind its upstream · pull first");
    }
    if pull
        && (lower.contains("not possible to fast-forward")
            || lower.contains("diverg")
            || lower.contains("would be overwritten")
            || lower.contains("local changes"))
    {
        return format!(
            "pull stopped · {branch} diverged or has local changes · commit, then pull --rebase"
        );
    }
    if lower.contains("no such remote") || lower.contains("does not appear to be a git repository")
    {
        return "remote not reachable · check `git remote -v`".to_string();
    }
    let first = text.lines().next().unwrap_or("").trim();
    format!("{} failed · {first}", if pull { "pull" } else { "push" })
}

/// Which pane has the keyboard on tab 2: branches, the file list, or the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    Branches,
    Files,
    Diff,
}

pub struct App {
    pub config: Config,
    /// The repository: branch, summary line, sync state.
    pub repo: RepoState,
    /// The commits the tool knows about.
    pub history: History,
    /// Tab 3: the log and the release toolbox.
    pub release: Release,
    /// Tab 2: branches, panes, wizard.
    pub branches: BranchesView,
    pub rules: Rules,
    pub scopes: ScopeInfo,
    /// The commit being written and where the cursor sits.
    pub form: Form,
    pub issues: Vec<Issue>,
    pub picker: Option<Picker>,
    pub status: String,
    /// True when `status` is a passing note (a key was pressed, a plan was
    /// rebuilt) rather than an answer the user has to act on. Notes clear on the
    /// next keypress, problems stay until something replaces them — one rule for
    /// all three tabs instead of each tab deciding for itself.
    pub status_is_note: bool,
    /// Set when the user asks to hand the message to git.
    pub output: Option<String>,
    /// True when the program was started with `--commit`, so Ctrl+S really
    /// commits. The footer says which of the two happens.
    pub commit_mode: bool,
    /// Set by `S`: the caller runs the setup dialog and starts over with the
    /// new config, which is what makes the change take effect immediately.
    pub setup_request: bool,
    /// Which of the three surfaces is showing.
    pub tab: tabs::Tab,
    /// Where the toggle is written, once the caller supplied one.
    config_store: Option<Box<dyn ConfigStore>>,
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
        let compact = config.compact_form;
        let rules = crate::validate::rules_from_config(&config);
        let scopes = crate::git::empty_scope_info(&config.scopes);
        let kind = config
            .types()
            .first()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        let mut app = Self {
            form: Form {
                compact,
                ..Form::with_kind(kind)
            },
            config,
            repo: RepoState::default(),
            history: History::default(),
            release: Release::default(),
            branches: BranchesView::default(),
            rules,
            scopes,
            config_store: None,
            issues: Vec::new(),
            picker: None,
            status: warnings.join(" · "),
            status_is_note: false,
            output: None,
            commit_mode: false,
            setup_request: false,
            tab: tabs::Tab::Commit,
            should_quit: false,
            show_help: false,
        };
        app.revalidate();
        app.seed_footer();
        app
    }

    /// The ticket of the branch we are on, when its name carries one.
    pub fn branch_ticket(&self) -> Option<String> {
        let name = self.repo.branch_name.as_deref()?;
        // The template first, so its rules win; then a plain look for a
        // ticket-shaped word, because a renamed branch still carries one.
        ConventionalBranchName::parse(name)
            .ok()
            .and_then(|parsed| parsed.ticket().map(std::string::ToString::to_string))
            .or_else(|| commitsmith::domain::ticket::in_text(name))
    }

    /// Who the co-author line names: the configured text, else the handle.
    pub fn co_author_name(&self) -> Option<String> {
        self.config
            .co_author_name
            .clone()
            .or_else(|| self.config.prefix.clone())
            .filter(|name| !name.trim().is_empty())
    }

    /// Suggested footer for a new message: the branch's ticket as the reference
    /// and — unless switched off — the co-author line.
    pub fn suggested_footer(&self) -> Option<String> {
        let mut lines = Vec::new();
        if let Some(ticket) = self.branch_ticket() {
            lines.push(format!("{} {ticket}", self.config.reference));
        }
        if self.config.co_author {
            if let Some(name) = self.co_author_name() {
                lines.push(format!("Co-authored-by: {name}"));
            }
        }
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    /// Fill the footer with the suggestion while it is still untouched.
    ///
    /// "Untouched" means empty or exactly what an earlier suggestion wrote, so
    /// switching branches updates the reference but never overwrites typing.
    /// Say something that is only worth reading until the next keypress.
    pub fn note(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_is_note = true;
    }

    /// Say something the user has to act on: it stays until it is replaced.
    pub fn problem(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_is_note = false;
    }

    pub fn seed_footer(&mut self) {
        let untouched = self.form.commit.footer.trim().is_empty()
            || self.form.footer_seed.as_deref() == Some(self.form.commit.footer.as_str());
        if !untouched {
            return;
        }
        match self.suggested_footer() {
            Some(footer) => {
                self.form.commit.footer.clone_from(&footer);
                self.form.cursors[FIELDS.iter().position(|f| *f == Field::Footer).unwrap_or(0)] =
                    self.form.commit.footer.chars().count();
                self.form.footer_seed = Some(footer);
            }
            None => {
                // The suggestion is gone (no ticket, no co-author): clear what it
                // wrote, but only that.
                if self.form.footer_seed.is_some() {
                    self.form.commit.footer.clear();
                    self.form.footer_seed = None;
                }
            }
        }
        self.revalidate();
    }

    /// What the header shows about the repository on the right.
    pub fn set_repo_line(&mut self, line: String) {
        self.repo.line = line;
    }

    /// Replace the available scopes, usually after reading git history.
    pub fn set_scopes(&mut self, scopes: ScopeInfo) {
        self.scopes = scopes;
    }

    /// Replace the list of recent commits offered as templates.
    pub fn set_history(&mut self, history: Vec<PastCommit>) {
        self.history.commits = history;
        self.history.rebuild_marks(&self.rules);
    }

    /// Hand the application the repository it may act on, and fill the branch
    /// list from it. Called once at startup.
    pub fn set_git(&mut self, git: Box<dyn GitRepository>) {
        self.repo.git = Some(git);
        self.reload_branches();
        self.reload_staging();
        self.refresh_sync();
        self.refresh_plan();
    }

    /// Short state for the footer: `↑2 ↓1`, `in sync`, or `no upstream`.
    pub fn sync_label(&self) -> Option<String> {
        self.repo.git.as_ref()?;
        commitsmith::domain::branch::sync_label(self.repo.sync)
    }

    /// Keys of the file list on tab 2: the same Space stages, the same arrows move.
    fn files_key(&mut self, key: KeyEvent) {
        // A destructive action asks first: `y` does it, anything else drops it.
        if let Some(pending) = self.branches.files.confirm.take() {
            self.answer_confirm(pending, key);
            return;
        }
        match key.code {
            KeyCode::Down | KeyCode::Up => {
                // A message about the last action must not follow the cursor.
                self.status.clear();
                if let Some(git) = self.repo.git.as_deref() {
                    let delta = if key.code == KeyCode::Down { 1 } else { -1 };
                    self.branches.files.move_by(delta, git);
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let outcome = match self.repo.git.as_deref() {
                    Some(git) => self
                        .branches
                        .files
                        .toggle(git)
                        .map(|message| (message, self.branches.files.show_diff)),
                    None => return,
                };
                match outcome {
                    Ok((message, diff_open)) => {
                        self.status = message;
                        // The list cleared its diff when it reloaded; with the
                        // pane open that looked like an empty file.
                        if diff_open {
                            if let Some(git) = self.repo.git.as_deref() {
                                let _ = self.branches.files.refresh_diff(git);
                            }
                        }
                        self.refresh_sync();
                        self.refresh_repo_line();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            KeyCode::Char('v') => {
                if self.branches.files.show_diff {
                    // Second press: close it and keep the list.
                    self.branches.files.show_diff = false;
                    self.branches.panel = Panel::Files;
                    return;
                }
                // Only now is the diff worth a process.
                self.branches.files.show_diff = true;
                self.branches.panel = Panel::Diff;
                if let Some(git) = self.repo.git.as_deref() {
                    let _ = self.branches.files.refresh_diff(git);
                }
            }
            KeyCode::Esc => self.branches.panel = Panel::Branches,
            // `r` takes the file out of the index, `x` throws its changes away,
            // `z` parks everything and `Z` takes the newest stash back.
            KeyCode::Char('r') => self.unstage_selected(),
            KeyCode::Char('x') => self.ask_confirm(),
            KeyCode::Char('z') => self.stash_now(),
            KeyCode::Char('Z') => {
                self.branches.files.confirm = Some(Confirm::StashPop);
                self.status = "take the newest stash back? y confirms".to_string();
            }
            _ => {}
        }
    }

    /// `r`: the file leaves the index, the working tree keeps its changes.
    fn unstage_selected(&mut self) {
        let Some(git) = self.repo.git.as_deref() else {
            return;
        };
        let Some(path) = self.branches.files.selected_path() else {
            self.status = "nothing selected".to_string();
            return;
        };
        if !self.branches.files.selected_is_staged() {
            self.status = format!("{path} is not staged");
            return;
        }
        if let Err(error) = git.unstage(&path) {
            self.status = error.to_string();
            return;
        }
        self.status = format!("{path} left the index, the changes stayed");
        self.reload_staging();
        self.refresh_sync();
        self.refresh_repo_line();
    }

    /// `x`: ask before the working-tree changes of the file are thrown away.
    fn ask_confirm(&mut self) {
        let Some(path) = self.branches.files.selected_path() else {
            self.status = "nothing selected".to_string();
            return;
        };
        if !self.branches.files.selected_has_worktree_changes() {
            self.status = format!("{path} has no unstaged changes to throw away");
            return;
        }
        self.status = format!("throw away the changes in {path}? y confirms");
        self.branches.files.confirm = Some(Confirm::Discard(path));
    }

    fn answer_confirm(&mut self, pending: Confirm, key: KeyEvent) {
        if key.code != KeyCode::Char('y') {
            self.status = "cancelled".to_string();
            return;
        }
        let Some(git) = self.repo.git.as_deref() else {
            return;
        };
        let outcome = match pending {
            Confirm::Discard(path) => git.discard(&path).map(|_| format!("discarded {path}")),
            Confirm::StashPop => git
                .stash_pop()
                .map(|_| "took the newest stash back".to_string()),
        };
        match outcome {
            Ok(message) => {
                self.status = message;
                self.reload_staging();
                self.refresh_sync();
                self.refresh_repo_line();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// `z`: park every change, tracked or not, in one stash entry.
    fn stash_now(&mut self) {
        let branch = self
            .repo
            .branch_name
            .clone()
            .unwrap_or_else(|| "detached".to_string());
        let message = format!("commitsmith {branch}");
        let Some(git) = self.repo.git.as_deref() else {
            return;
        };
        match git.stash(&message) {
            Ok(()) => {
                self.status = "parked in a stash · Z takes it back".to_string();
                self.reload_staging();
                self.refresh_sync();
                self.refresh_repo_line();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// Re-read the changed files and the diff of the highlighted one.
    pub fn reload_staging(&mut self) {
        let (git, staging) = (&self.repo.git, &mut self.branches.files);
        let Some(git) = git.as_deref() else {
            return;
        };
        if let Err(error) = staging.reload(git) {
            self.status = error.to_string();
        }
    }

    /// Re-read the branch list, the branch line in the footer, and the context
    /// the composer takes from the branch.
    pub fn reload_branches(&mut self) {
        if let Some(branch) = self
            .branches
            .list
            .items
            .iter()
            .find(|branch| branch.is_head)
        {
            let name = Some(branch.name.clone());
            if self.repo.branch_name != name {
                self.repo.branch_name = name;
                self.seed_footer();
            }
        }
        let failure = {
            let (git, branches) = (&self.repo.git, &mut self.branches.list);
            let Some(git) = git.as_deref() else {
                return;
            };
            branches.reload(git).err()
        };
        // The list just loaded already knows where `HEAD` is; asking the
        // repository again was a second walk over every reference.
        let (head, staged) = {
            let Some(git) = self.repo.git.as_deref() else {
                return;
            };
            (
                self.branches
                    .list
                    .items
                    .iter()
                    .find(|branch| branch.is_head)
                    .map(|branch| branch.name.clone()),
                git.changes()
                    .map(|files| files.iter().filter(|file| file.is_staged()).count())
                    .unwrap_or(0),
            )
        };
        self.repo.branch_name = head;
        self.set_repo_summary(staged);
        if let Some(error) = failure {
            self.status = error.to_string();
        }
        self.apply_branch_type();
        self.seed_footer();
    }

    /// Take the commit type from the branch kind — but only while the user has
    /// not chosen a type themselves.
    fn apply_branch_type(&mut self) {
        if self.form.type_touched {
            return;
        }
        let Some(name) = self.repo.branch_name.as_deref() else {
            return;
        };
        let Ok(parsed) = ConventionalBranchName::parse(name) else {
            return;
        };
        let kind = parsed.kind().commit_type().to_string();
        // A profile may not have that type; then the current one stays.
        if !self.config.type_names().contains(&kind) {
            return;
        }
        if self.form.commit.kind != kind {
            self.form.commit.kind = kind;
            self.revalidate();
        }
    }

    /// Keys of the second and third tab. The composer's keys must not fire here.
    fn branch_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.branches.wizard.open {
            self.wizard_key(key);
            return;
        }
        // Tab and Shift+Tab walk the panes: branches, files, diff. The same key on
        // every tab, so the hand learns one gesture.
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            let back = key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
            if self.branches.panel == Panel::Branches {
                // Coming back to the files: they may have changed in the editor.
                self.reload_staging();
            }
            self.branches.panel = match (self.branches.panel, back) {
                (Panel::Branches, false) | (Panel::Diff, true) => Panel::Files,
                (Panel::Files, false) => {
                    if self.branches.files.show_diff {
                        Panel::Diff
                    } else {
                        Panel::Branches
                    }
                }
                (Panel::Branches, true) => {
                    if self.branches.files.show_diff {
                        Panel::Diff
                    } else {
                        Panel::Files
                    }
                }
                (Panel::Diff, false) | (Panel::Files, true) => Panel::Branches,
            };
            return;
        }
        if self.branches.panel == Panel::Files {
            self.files_key(key);
            return;
        }
        if self.branches.panel == Panel::Diff {
            let page = 10usize;
            match key.code {
                KeyCode::Esc | KeyCode::Char('v') => {
                    self.branches.files.show_diff = false;
                    self.branches.panel = Panel::Files;
                }
                KeyCode::Down => self.branches.files.scroll_diff(1, page),
                KeyCode::Up => self.branches.files.scroll_diff(-1, page),
                KeyCode::PageDown => self.branches.files.scroll_diff(page as isize, page),
                KeyCode::PageUp => self.branches.files.scroll_diff(-(page as isize), page),
                KeyCode::Home => self.branches.files.diff_offset = 0,
                KeyCode::End => self.branches.files.scroll_diff(isize::MAX / 2, page),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('c' | 'q') if ctrl => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = true,
            // Only the arrows move through the list: `j`/`k` and `g`/`G` were a
            // second vocabulary nobody asked for.
            KeyCode::Down => {
                self.branches.list.confirming = None;
                self.status.clear();
                self.branches.list.move_by(1);
            }
            KeyCode::Up => {
                self.branches.list.confirming = None;
                self.status.clear();
                self.branches.list.move_by(-1);
            }
            KeyCode::Home => {
                self.branches.list.confirming = None;
                self.branches.list.jump_to_start();
            }
            KeyCode::End => {
                self.branches.list.confirming = None;
                self.branches.list.jump_to_end();
            }
            KeyCode::Enter => self.checkout_selected(),
            KeyCode::Char('m') => self.merge_selected(),
            KeyCode::Char('d') => self.delete_selected(),
            KeyCode::Char('r') => self.reload_branches(),
            KeyCode::Char('f') => self.fetch_remote(),
            KeyCode::Char('n') => self.branches.wizard.start(),
            KeyCode::Esc => {
                // Esc is the cancel key while a deletion waits for its second
                // press; otherwise it leaves the program as everywhere else.
                if self.branches.list.confirming.take().is_none() {
                    self.should_quit = true;
                }
            }
            _ => {}
        }
    }

    /// Keys inside the wizard: typing wins over every list shortcut.
    fn wizard_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.branches.wizard.close(),
            KeyCode::Char('c') if ctrl => {
                self.branches.wizard.close();
                self.should_quit = true;
            }
            KeyCode::Tab => self.branches.wizard.field = self.branches.wizard.field.next(),
            KeyCode::BackTab => self.branches.wizard.field = self.branches.wizard.field.previous(),
            // The same rules as in the composer: `←`/`→` are the text, `↑`/`↓`
            // walk the fields, `Tab` does both the obvious way.
            KeyCode::Left | KeyCode::Right => {
                let has_text = self.branches.wizard.current_text().is_some();
                match navigation::horizontal(key.code, has_text) {
                    Some(navigation::Horizontal::Caret(delta)) => {
                        self.branches.wizard.move_cursor(delta)
                    }
                    Some(navigation::Horizontal::Value(delta)) => {
                        self.branches.wizard.cycle_kind(delta > 0)
                    }
                    None => {}
                }
            }
            KeyCode::Up => self.branches.wizard.step(-1),
            KeyCode::Down => self.branches.wizard.step(1),
            KeyCode::Home => self.branches.wizard.jump(false),
            KeyCode::End => self.branches.wizard.jump(true),
            KeyCode::Delete => self.branches.wizard.delete(),
            KeyCode::Backspace => self.branches.wizard.backspace(),
            KeyCode::Enter => self.create_branch(),
            KeyCode::Char(c) if !ctrl => self.branches.wizard.push_char(c),
            _ => {}
        }
    }

    fn create_branch(&mut self) {
        let author = self
            .config
            .author
            .clone()
            .or_else(|| self.config.prefix.clone());
        let prefix = self.config.ticket_prefix.clone();
        let template = self.config.branch_template.clone();
        let (git, wizard) = (&self.repo.git, &mut self.branches.wizard);
        let Some(git) = git.as_deref() else {
            self.status = "no repository".to_string();
            return;
        };
        match wizard.create(git, author.as_deref(), &prefix, &template) {
            Ok(name) => {
                self.status = format!("created and checked out {name}");
                self.reload_branches();
            }
            Err(message) => self.status = message,
        }
    }

    fn selected_branch(&mut self) -> Option<commitsmith::domain::ports::BranchInfo> {
        self.branches.list.current().cloned()
    }

    fn checkout_selected(&mut self) {
        let Some(branch) = self.selected_branch() else {
            return;
        };
        if branch.is_remote {
            self.status = format!("{} is remote · check out a local branch", branch.name);
            return;
        }
        let Some(git) = self.repo.git.as_deref() else {
            self.status = "no repository".to_string();
            return;
        };
        match git.checkout(&branch.name) {
            Ok(()) => {
                self.status = format!("checked out {}", branch.name);
                self.reload_branches();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn merge_selected(&mut self) {
        let Some(branch) = self.selected_branch() else {
            return;
        };
        if branch.is_head {
            self.status = format!("{} is already checked out", branch.name);
            return;
        }
        let Some(git) = self.repo.git.as_deref() else {
            self.status = "no repository".to_string();
            return;
        };
        match git.merge(&branch.name) {
            Ok(()) => {
                self.status = format!("merged {} into the current branch", branch.name);
                self.reload_branches();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn delete_selected(&mut self) {
        let Some(branch) = self.selected_branch() else {
            return;
        };
        if branch.is_head {
            self.status = format!("{} is checked out · switch first", branch.name);
            return;
        }
        if branch.is_remote {
            self.status = format!(
                "{} lives on the remote · delete it with push --delete",
                branch.name
            );
            return;
        }
        if self.branches.list.confirming.as_deref() != Some(branch.name.as_str()) {
            self.branches.list.confirming = Some(branch.name.clone());
            self.status = format!("press d again to delete {}", branch.name);
            return;
        }
        let Some(git) = self.repo.git.as_deref() else {
            self.status = "no repository".to_string();
            return;
        };
        self.branches.list.confirming = None;
        match git.delete_branch(&branch.name) {
            Ok(()) => {
                self.status = format!("deleted {}", branch.name);
                self.reload_branches();
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// Record whether a repository was found. Called once at startup.
    pub fn set_in_repository(&mut self, found: bool) {
        self.repo.in_repository = found;
        if !found {
            let hint = "not a git repository · `check` works here, committing needs one";
            self.status = if self.status.is_empty() {
                hint.to_string()
            } else {
                format!("{} · {hint}", self.status)
            };
        }
    }

    /// Read the file list for the highlighted history entry, once.
    fn refresh_history_preview(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if picker.kind != PickerKind::History {
            return;
        }
        let visible = picker.filtered();
        let Some(&index) = visible.get(picker.selected.min(visible.len().saturating_sub(1))) else {
            return;
        };
        let Some(id) = picker.values.get(index).cloned() else {
            return;
        };

        if !self.history.cache.contains_key(&id) {
            // The repository the tab already holds answers this; the closure is
            // only for an `App` built without one (tests, the CLI).
            let Some(git) = self.repo.git.as_deref() else {
                return;
            };
            let files = match git.commit_files(&id) {
                Ok(files) => files,
                Err(error) => {
                    self.status = error.to_string();
                    Vec::new()
                }
            };
            self.history.cache.insert(id.clone(), files);
        }
        let files = self.history.cache.get(&id).cloned().unwrap_or_default();
        let Some(commit) = self.history.commits.iter().find(|commit| commit.id == id) else {
            return;
        };
        let mut enriched = commit.clone();
        enriched.files = files;
        if let Some(picker) = self.picker.as_mut() {
            if let Some(slot) = picker.previews.get_mut(index) {
                *slot = enriched.preview();
            }
        }
    }

    /// Tell the UI whether Ctrl+S commits or only prints.
    pub fn set_commit_mode(&mut self, commit: bool) {
        self.commit_mode = commit;
    }

    pub fn active_field(&self) -> Field {
        FIELDS[self.form.field.min(FIELDS.len() - 1)]
    }

    /// Cursor of the active field.
    pub fn cursor(&self) -> usize {
        self.form.cursors[self.form.field]
    }

    /// Value of a field, for rendering.
    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::Type => &self.form.commit.kind,
            Field::Scope => &self.form.commit.scope,
            Field::Subject => &self.form.commit.subject,
            Field::Body => &self.form.commit.body,
            Field::Breaking => &self.form.commit.breaking,
            Field::Footer => &self.form.commit.footer,
        }
    }

    fn value_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Type => &mut self.form.commit.kind,
            Field::Scope => &mut self.form.commit.scope,
            Field::Subject => &mut self.form.commit.subject,
            Field::Body => &mut self.form.commit.body,
            Field::Breaking => &mut self.form.commit.breaking,
            Field::Footer => &mut self.form.commit.footer,
        }
    }

    pub fn revalidate(&mut self) {
        self.issues = validate::validate_with(&self.form.commit, &self.rules);
    }

    /// The emoji of a commit type, when the config wants emojis. One lookup for
    /// both places that show a type: the header on tab 1 and the branch wizard.
    pub fn kind_emoji(&self, kind: &str) -> Option<String> {
        if !self.config.emoji {
            return None;
        }
        self.config
            .types()
            .iter()
            .find(|t| t.name == kind)
            .and_then(|t| t.emoji.clone())
    }

    /// Name, emoji and description of the current type, for the header.
    pub fn type_label(&self) -> String {
        let commit = &self.form.commit;
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

    /// Ctrl+Space asks the field that has the keyboard what it has to offer:
    /// types in the type field, scopes in the scope field, earlier commits in
    /// the subject. Anywhere else it says so instead of opening the wrong list.
    fn open_suggestions(&mut self) {
        match self.active_field() {
            Field::Type => {
                self.status =
                    "type: Enter or Space opens the list, ←/→ switches without it".to_string();
            }
            Field::Scope => self.open_scope_picker(),
            Field::Subject => self.open_history_picker(false),
            Field::Body | Field::Breaking | Field::Footer => {
                self.status = format!(
                    "no suggestions for {} · Ctrl+Space offers types, scopes and earlier subjects",
                    self.active_field().label()
                );
            }
        }
    }

    fn confirm_picker(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        let Some(choice) = picker.current().cloned() else {
            self.status = "nothing selected".to_string();
            return;
        };
        // Read the mode before the picker goes away: it decides whether the
        // choice fills one field or the whole form.
        let full = picker.full_reuse;
        self.apply_picker_choice(picker.kind, choice, full);
    }

    /// `Ctrl+O`: fold the optional fields away, or bring them back. The choice is
    /// written to the user config right away, so it survives the restart.
    fn toggle_compact_form(&mut self) {
        self.form.compact = !self.form.compact;
        self.config.compact_form = self.form.compact;
        if !self.field_visible(self.active_field()) {
            // The subject stays visible in both states; it is where typing
            // continues after folding.
            let subject = FIELDS
                .iter()
                .position(|field| self.field_visible(*field) && *field == Field::Subject)
                .unwrap_or(0);
            self.focus(subject);
        }
        let saved = match &self.config_store {
            Some(store) => store
                .write_user(&self.config.to_toml())
                .map(|_| ())
                .map_err(|error| error.to_string()),
            None => Ok(()),
        };
        self.status = match (self.form.compact, saved) {
            (true, Ok(())) => "compact form · Ctrl+O shows body, breaking and footer".to_string(),
            (false, Ok(())) => "full form · Ctrl+O folds it back".to_string(),
            (_, Err(error)) => format!("compact form not saved: {error}"),
        };
    }

    /// The caller hands in the store that writes the user config.
    pub fn set_config_store(&mut self, store: Box<dyn ConfigStore>) {
        self.config_store = Some(store);
    }

    /// Digits switch tabs, but not while a text field is taking characters:
    /// `1` belongs in a subject like "1st try" more than it belongs to the tab bar.
    fn tab_key(&mut self, key: KeyEvent) -> bool {
        if self.show_help || self.picker.is_some() {
            return false;
        }
        // Digits are text while a field is being typed into — in the composer
        // and in the branch wizard, where a ticket number starts with one.
        let typing =
            (self.tab == tabs::Tab::Commit && self.is_editing()) || self.branches.wizard.open;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(digit @ '1'..='3') if !ctrl && !typing => {
                if let Some(tab) = tabs::Tab::from_digit(digit) {
                    self.tab = tab;
                    if tab == tabs::Tab::History {
                        // The plan is about the current HEAD, so it is read when
                        // the tab is opened, not once at startup.
                        self.refresh_plan();
                    }
                    if tab == tabs::Tab::Branches {
                        // Files change outside the program; the list is read again
                        // whenever it comes into view.
                        self.reload_staging();
                        // `refresh_sync` costs two more `git` calls; the arrows
                        // are read when a branch action needs them, not for a
                        // tab that is merely being looked at.
                        // Every tab opens on its main pane, never on whatever was
                        // focused the last time it was looked at.
                        self.branches.panel = Panel::Branches;
                    }
                    return true;
                }
            }
            KeyCode::PageUp if ctrl => {
                self.tab = self.tab.previous();
                return true;
            }
            KeyCode::PageDown if ctrl => {
                self.tab = self.tab.next();
                return true;
            }
            _ => {}
        }
        false
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
        if self.form.amend {
            if self.form.confirm_amend {
                self.form.confirm_amend = false;
                self.output = Some(self.form.commit.render());
                self.should_quit = true;
                self.status = "replacing the last commit".to_string();
                return;
            }
            if self.head_is_published() {
                self.form.confirm_amend = true;
                self.status = "HEAD is already pushed · Enter or y replaces it anyway, Esc cancels"
                    .to_string();
                return;
            }
        }
        self.output = Some(self.form.commit.render());
        self.should_quit = true;
        self.status = "handing the message over".to_string();
    }

    fn insert(&mut self, c: char) {
        let field = self.active_field();
        if field == Field::Type {
            return;
        }
        let cursor = self.form.cursors[self.form.field];
        let value = self.value_mut(field);
        let mut chars: Vec<char> = value.chars().collect();
        chars.insert(cursor.min(chars.len()), c);
        *value = chars.into_iter().collect();
        self.form.cursors[self.form.field] = (cursor + 1).min(value.chars().count());
        self.revalidate();
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.value(self.active_field()).chars().count();
        let cursor = self.form.cursors[self.form.field];
        let next = if delta < 0 {
            cursor.saturating_sub(delta.unsigned_abs())
        } else {
            (cursor + delta as usize).min(len)
        };
        self.form.cursors[self.form.field] = next;
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
        let cursor = self.form.cursors[self.form.field];
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
            self.form.cursors[self.form.field] = prev_start + column.min(prev_len);
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
                    self.form.cursors[self.form.field] = next_start + column.min(next_len);
                }
                None => self.next_field(),
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // A note has done its job once the next key arrives; a problem stays.
        if self.status_is_note {
            self.status.clear();
            self.status_is_note = false;
        }
        // Replacing a published commit asks once: y/Enter go ahead, every other
        // key is "no" and drops the question (the same rule as the destructive
        // staging actions).
        if self.form.confirm_amend && !self.show_help && self.picker.is_none() {
            self.form.confirm_amend = false;
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.output = Some(self.form.commit.render());
                    self.should_quit = true;
                    self.status = "replacing the last commit".to_string();
                }
                _ => {
                    self.status = "amend cancelled · nothing was rewritten".to_string();
                    self.form.amend = false;
                }
            }
            return;
        }
        // Ctrl+O folds the optional fields away, wherever the keyboard is — the
        // form and its visibility are a property of the tab, not of a field.
        // Alt+P pulls (fast-forward only), Alt+U pushes. Letters alone belong to
        // the composer, so the sync keys need a modifier.
        if self.tab == tabs::Tab::Commit
            && key.modifiers.contains(KeyModifiers::ALT)
            && !self.show_help
            && self.picker.is_none()
        {
            match key.code {
                KeyCode::Char('p' | 'P') => {
                    self.sync_with_remote(true);
                    return;
                }
                KeyCode::Char('u' | 'U') => {
                    self.sync_with_remote(false);
                    return;
                }
                _ => {}
            }
        }
        if key.code == KeyCode::Char('a')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !self.show_help
            && self.picker.is_none()
            && self.tab == tabs::Tab::Commit
        {
            self.begin_amend();
            return;
        }
        if key.code == KeyCode::Char('o')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !self.show_help
            && self.picker.is_none()
            && self.tab == tabs::Tab::Commit
        {
            self.toggle_compact_form();
            return;
        }
        if self.picker_key(key) {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.tab_key(key) {
            return;
        }

        if self.show_help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => self.show_help = false,
                KeyCode::Char('c') if ctrl => self.should_quit = true,
                _ => {}
            }
            return;
        }

        // Tab 3 keeps its own keyboard: the filter line takes typing, the list
        // takes the arrows.
        if self.tab == tabs::Tab::History {
            self.history_key(key);
            return;
        }

        // The other tabs have their own keys; typing a subject there would be
        // a surprise, so the composer never sees these events.
        if self.tab != tabs::Tab::Commit {
            self.branch_key(key);
            return;
        }

        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('S') => {
                self.setup_request = true;
                self.should_quit = true;
            }
            KeyCode::Char('c' | 'q') if ctrl => self.should_quit = true,
            KeyCode::Char('s') if ctrl => self.finish(),
            // The IDE pair: what Ctrl+Space and Ctrl+Enter do in an editor.
            // Terminals send Ctrl+Space as NUL, which some of them report as
            // `Null` and others as `Char('\0')` — both mean "suggestions".
            KeyCode::Null | KeyCode::Char('\0') => self.open_suggestions(),
            KeyCode::Char(' ') if ctrl => self.open_suggestions(),
            KeyCode::Enter if ctrl => self.finish(),
            KeyCode::Char('k') if ctrl => self.clear_field(),
            KeyCode::Char('n') if ctrl => self.start_new_commit(),
            KeyCode::Char('t') if ctrl => self.open_type_picker(),
            KeyCode::Char('p') if ctrl => self.open_profile_picker(),
            KeyCode::Char('r') if ctrl => self.open_history_picker(true),
            KeyCode::Char('q') if self.active_field() == Field::Type => self.should_quit = true,
            KeyCode::Tab => self.next_field(),
            KeyCode::BackTab => self.previous_field(),
            // The type list: Enter (and Space) open it, Ctrl+N is for the
            // things one has to think about.
            KeyCode::Enter if self.active_field() == Field::Type => self.open_type_picker(),
            KeyCode::Enter => self.enter(),
            KeyCode::Up | KeyCode::Down => self.vertical(key),
            KeyCode::Char(' ') if self.active_field() == Field::Type => self.open_type_picker(),
            KeyCode::Left | KeyCode::Right => self.arrow(key),
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
mod tests;
