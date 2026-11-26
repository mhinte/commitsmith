//! Application layer: state, tabs and the use cases the surface triggers.
//!
//! Key events come in as `crossterm` key codes — that is input, not rendering —
//! but no widget, frame or terminal type appears here.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub mod branch_wizard;
pub mod branches;
pub mod log;
pub mod navigation;
pub mod staging;
pub mod tabs;
#[cfg(test)]
pub mod test_support;

use std::collections::HashMap;

use crate::commit::Commit;

/// Reads a commit's file list when the history preview needs it.
pub type FilesLoader = Box<dyn Fn(&str) -> Vec<String> + Send + Sync>;
use crate::config::Config;
use crate::git::{PastCommit, ScopeInfo};
use crate::validate::{self, Issue, Rules, Severity};
use branch_wizard::BranchWizard;
use branches::BranchList;
use commitsmith::domain::branch::ConventionalBranchName;
use commitsmith::domain::ports::{ConfigStore, GitRepository};
use log::LogState;
use staging::{Side, Staging};

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

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
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
    /// Set by `S`: the caller runs the setup dialog and starts over with the
    /// new config, which is what makes the change take effect immediately.
    pub setup_request: bool,
    /// Which of the three surfaces is showing.
    pub tab: tabs::Tab,
    /// The repository behind the actions of tab 2, when there is one.
    pub git: Option<Box<dyn GitRepository>>,
    /// Branch explorer state.
    pub branches: BranchList,
    /// Which pane has the keyboard on tab 2.
    pub panel: Panel,
    /// Log state of tab 3.
    pub log: LogState,
    /// What the next release would be, from the last tag to `HEAD`.
    pub plan: Option<commitsmith::domain::release::ReleasePlan>,
    /// True while the write waits for a second keypress: nothing happens behind
    /// the user's back.
    pub release_confirm: bool,
    /// Where `CHANGELOG.md` lives; set by the caller, `None` outside a repository.
    changelog_root: Option<std::path::PathBuf>,
    /// Staging list of tab 1.
    pub staging: Staging,
    /// Short name of the checked-out branch, for the context bridge.
    pub branch_name: Option<String>,
    /// True once the user chose a type; the branch must not override it.
    type_touched: bool,
    /// What `seed_footer` last wrote, so a later suggestion may replace its own
    /// text but never what the user typed.
    footer_seed: Option<String>,
    /// Body, breaking and footer folded away. Read from the config, written back
    /// when `Ctrl+O` toggles it.
    pub compact: bool,
    /// `(ahead, behind)` against the upstream; `None` when there is no upstream.
    pub sync: Option<(usize, usize)>,
    /// Where the toggle is written, once the caller supplied one.
    config_store: Option<Box<dyn ConfigStore>>,
    /// The conventional branch wizard on tab 2.
    pub wizard: BranchWizard,
    /// False when the working directory is not inside a repository; the header
    /// says so, because scope suggestions and committing need one.
    pub in_repository: bool,
    /// File lists are read on demand: one `git show` per commit at startup made
    /// long histories slow.
    files_loader: Option<FilesLoader>,
    pub files_cache: HashMap<String, Vec<String>>,
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
        let config_compact = compact;
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
            compact: config_compact,
            sync: None,
            config_store: None,
            issues: Vec::new(),
            picker: None,
            history: Vec::new(),
            status: warnings.join(" · "),
            output: None,
            commit_mode: false,
            setup_request: false,
            tab: tabs::Tab::Commit,
            git: None,
            branches: BranchList::default(),
            panel: Panel::default(),
            log: LogState::default(),
            plan: None,
            release_confirm: false,
            changelog_root: None,
            staging: Staging::default(),
            branch_name: None,
            type_touched: false,
            footer_seed: None,
            wizard: BranchWizard::default(),
            in_repository: false,
            files_loader: None,
            files_cache: HashMap::new(),
            should_quit: false,
            show_help: false,
        };
        app.revalidate();
        app.seed_footer();
        app
    }

    /// The ticket of the branch we are on, when its name carries one.
    pub fn branch_ticket(&self) -> Option<String> {
        let name = self.branch_name.as_deref()?;
        ConventionalBranchName::parse(name)
            .ok()
            .and_then(|parsed| parsed.ticket().map(|ticket| ticket.to_string()))
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
    pub fn seed_footer(&mut self) {
        let untouched = self.commit.footer.trim().is_empty()
            || self.footer_seed.as_deref() == Some(self.commit.footer.as_str());
        if !untouched {
            return;
        }
        match self.suggested_footer() {
            Some(footer) => {
                self.commit.footer = footer.clone();
                self.cursors[FIELDS.iter().position(|f| *f == Field::Footer).unwrap_or(0)] =
                    self.commit.footer.chars().count();
                self.footer_seed = Some(footer);
            }
            None => {
                // The suggestion is gone (no ticket, no co-author): clear what it
                // wrote, but only that.
                if self.footer_seed.is_some() {
                    self.commit.footer.clear();
                    self.footer_seed = None;
                }
            }
        }
        self.revalidate();
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

    /// Hand the application the repository it may act on, and fill the branch
    /// list from it. Called once at startup.
    pub fn set_git(&mut self, git: Box<dyn GitRepository>) {
        self.git = Some(git);
        self.reload_branches();
        self.reload_staging();
        self.refresh_sync();
        self.refresh_plan();
    }

    pub fn set_changelog_root(&mut self, root: std::path::PathBuf) {
        self.changelog_root = Some(root);
    }

    /// Write the changelog section and tag the release — but only after `y` armed
    /// it and Enter confirmed.
    fn write_release(&mut self) {
        self.release_confirm = false;
        let Some(plan) = self.plan.clone() else {
            self.status = "no release plan · nothing to write".to_string();
            return;
        };
        let Some(root) = self.changelog_root.clone() else {
            self.status = "no repository · nowhere to write CHANGELOG.md".to_string();
            return;
        };
        let section = commitsmith::domain::release::render_section(
            &plan,
            Some(&commitsmith::infrastructure::clock::today()),
        );
        match commitsmith::infrastructure::changelog::write_section(&root, &plan.next, &section) {
            Ok(path) => {
                // No tag: the project does not use them yet, and inventing one
                // would be a promise nobody made. The file is the release note.
                self.status = format!(
                    "wrote {} · no tag set (git tag v{})",
                    path.display(),
                    plan.next
                );
            }
            Err(error) => self.status = format!("changelog not written: {error}"),
        }
    }

    /// Rebuild the release plan: last version tag, the commits since it, and what
    /// they demand. Reading only — nothing is written until the user asks.
    pub fn refresh_plan(&mut self) {
        let Some(git) = self.git.as_deref() else {
            self.plan = None;
            return;
        };
        let tags = git.tags().unwrap_or_default();
        let last = commitsmith::domain::release::ReleasePlan::latest_tag(&tags);
        let limit = self.config.history_depth.min(2000);
        let commits = git
            .commits_since(last.as_deref(), limit)
            .unwrap_or_default();
        self.plan = Some(commitsmith::domain::release::ReleasePlan::build(
            last.as_deref(),
            commits,
        ));
    }

    /// Read `(ahead, behind)` once, for the footer and for the wording of the
    /// pull/push replies.
    pub fn refresh_sync(&mut self) {
        let Some(git) = self.git.as_deref() else {
            self.sync = None;
            return;
        };
        self.sync = git.ahead_behind().unwrap_or(None);
    }

    /// Short state for the footer: `↑2 ↓1`, `in sync`, or `no upstream`.
    pub fn sync_label(&self) -> Option<String> {
        self.git.as_ref()?;
        Some(match self.sync {
            Some((0, 0)) => "✓".to_string(),
            Some((ahead, 0)) => format!("↑{ahead}"),
            Some((0, behind)) => format!("↓{behind}"),
            Some((ahead, behind)) => format!("↑{ahead}↓{behind}"),
            None => "–".to_string(),
        })
    }

    /// The branch name as the user knows it, for messages.
    fn branch_label(&self) -> String {
        self.branch_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "HEAD".to_string())
    }

    /// Alt+P / Alt+U: fetch and fast-forward, or send. Never merges: a divergent
    /// branch is something to resolve in git, not behind a keypress.
    fn sync_with_remote(&mut self, pull: bool) {
        let Some(git) = self.git.as_deref() else {
            self.status = "not a git repository · nothing to sync".to_string();
            return;
        };
        let branch = self.branch_label();
        if self.sync.is_none() {
            self.status = format!("no upstream for {branch} · git push -u origin {branch}");
            return;
        }
        let (ahead, behind) = self.sync.unwrap_or((0, 0));
        let result = if pull { git.pull() } else { git.push() };
        match result {
            Ok(()) => {
                self.refresh_sync();
                let (new_ahead, new_behind) = self.sync.unwrap_or((0, 0));
                self.status = if pull {
                    match behind {
                        0 => format!("{branch} was already up to date"),
                        count => format!(
                            "pulled {count} commit{} into {branch} · ↑{new_ahead} ↓{new_behind}",
                            plural(count)
                        ),
                    }
                } else {
                    match ahead {
                        0 => format!("nothing to push · {branch} is level with its upstream"),
                        count => format!(
                            "pushed {count} commit{} · {branch} level with upstream",
                            plural(count)
                        ),
                    }
                };
                self.reload_branches();
                self.reload_staging();
            }
            Err(error) => self.status = explain_sync_error(&error, pull, &branch),
        }
    }

    /// Keys of tab 3. While the filter line is open it swallows the characters,
    /// so a ticket with digits cannot switch tabs.
    fn history_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Char('?') && self.log.input.is_none() {
            self.show_help = true;
            return;
        }
        if matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) && ctrl {
            self.should_quit = true;
            return;
        }
        // The release: `y` asks, Enter writes, Esc or `n` backs out.
        if self.release_confirm && self.log.input.is_none() {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') => self.write_release(),
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.release_confirm = false;
                    self.status = "release cancelled · file and tags untouched".to_string();
                }
                _ => {}
            }
            return;
        }
        if self.log.input.is_some() {
            match key.code {
                KeyCode::Esc => self.log.cancel_filter(),
                KeyCode::Enter => {
                    let types = self.config.type_names();
                    self.log.apply_filter(&types);
                    let len = self.log.visible(&self.history).len();
                    self.log.clamp(len);
                    self.status = match self.log.filter.is_empty() {
                        true => "filter cleared".to_string(),
                        false => {
                            let matches = format!("{} matches", len);
                            format!("{} · {}", self.log.filter.describe(), matches)
                        }
                    };
                }
                KeyCode::Backspace => self.log.backspace(),
                KeyCode::Char(c) => self.log.push(c),
                _ => {}
            }
            return;
        }
        let len = self.log.visible(&self.history).len();
        match key.code {
            KeyCode::Up => self.log.move_by(-1, len),
            KeyCode::Down => self.log.move_by(1, len),
            KeyCode::Home => self.log.selected = 0,
            KeyCode::End => self.log.selected = len.saturating_sub(1),
            KeyCode::Char('/') => self.log.open_filter(),
            KeyCode::Char('c') => self.log.clear_filter(),
            KeyCode::Char('y') => {
                self.release_confirm = true;
                let next = self
                    .plan
                    .as_ref()
                    .map(|plan| plan.next.to_string())
                    .unwrap_or_default();
                self.status =
                    format!("write CHANGELOG.md for {next}? Enter confirms · Esc cancels");
            }
            _ => {}
        }
    }

    /// How a past commit's message would be judged today: the same rules the
    /// composer and `commitsmith check` use.
    pub fn log_mark(&self, commit: &PastCommit) -> Severity {
        let issues = self.log_issues(commit);
        if issues.iter().any(|issue| issue.severity == Severity::Error) {
            Severity::Error
        } else if issues
            .iter()
            .any(|issue| issue.severity == Severity::Warning)
        {
            Severity::Warning
        } else {
            Severity::Info
        }
    }

    /// The concrete findings for a past commit, so the detail pane can say which
    /// rule broke instead of only that something did.
    pub fn log_issues(&self, commit: &PastCommit) -> Vec<Issue> {
        let mut text = commit.header();
        if !commit.body.trim().is_empty() {
            text.push_str("\n\n");
            text.push_str(commit.body.trim_end());
        }
        validate::lint_with(&text, &self.rules)
    }

    /// `f` on tab 2: update the remote-tracking branches, then redraw the list so
    /// the sync marks are right. Nothing in the work tree changes.
    fn fetch_remote(&mut self) {
        let Some(git) = self.git.as_deref() else {
            self.status = "not a git repository · nothing to fetch".to_string();
            return;
        };
        match git.fetch() {
            Ok(()) => {
                self.reload_branches();
                self.refresh_sync();
                let changed = self
                    .branches
                    .items
                    .iter()
                    .filter(|branch| !branch.is_remote)
                    .filter(|branch| {
                        matches!(self.branches.sync.get(&branch.name), Some((ahead, behind)) if *ahead > 0 || *behind > 0)
                    })
                    .count();
                self.status = match changed {
                    0 => "fetched · every branch is level with its upstream".to_string(),
                    n => format!("fetched · {n} branch{} with commits to sync", plural(n)),
                };
            }
            Err(error) => self.status = explain_sync_error(&error, true, &self.branch_label()),
        }
    }

    /// Keys of the file list on tab 2: the same Space stages, the same arrows move.
    fn files_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Up => {
                // A message about the last action must not follow the cursor.
                self.status.clear();
                if let Some(git) = self.git.as_deref() {
                    let delta = if key.code == KeyCode::Down { 1 } else { -1 };
                    self.staging.move_by(delta, git);
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let Some(git) = self.git.as_deref() else {
                    return;
                };
                match self.staging.toggle(git) {
                    Ok(message) => {
                        self.status = message;
                        self.refresh_sync();
                    }
                    Err(error) => self.status = error.to_string(),
                }
            }
            KeyCode::Char('v') => {
                // Only now is the diff worth a process.
                self.staging.show_diff = true;
                self.panel = Panel::Diff;
                if let Some(git) = self.git.as_deref() {
                    let _ = self.staging.refresh_diff(git);
                }
            }
            KeyCode::Esc => self.panel = Panel::Branches,
            _ => {}
        }
    }

    /// Re-read the changed files and the diff of the highlighted one.
    pub fn reload_staging(&mut self) {
        let (git, staging) = (&self.git, &mut self.staging);
        let Some(git) = git.as_deref() else {
            return;
        };
        if let Err(error) = staging.reload(git) {
            self.status = error.to_string();
        }
    }

    /// Keys while the staging list has the keyboard.
    fn staging_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up => self.move_staging(-1),
            KeyCode::Down => self.move_staging(1),
            KeyCode::Char(' ') => self.toggle_staged(),
            KeyCode::Char('v') => {
                self.staging.show_diff = !self.staging.show_diff;
            }
            KeyCode::Esc => self.staging.side = Side::Composer,
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            _ => {}
        }
    }

    fn move_staging(&mut self, delta: isize) {
        let (git, staging) = (&self.git, &mut self.staging);
        if let Some(git) = git.as_deref() {
            staging.move_by(delta, git);
        }
    }

    fn toggle_staged(&mut self) {
        let (git, staging) = (&self.git, &mut self.staging);
        let Some(git) = git.as_deref() else {
            return;
        };
        match staging.toggle(git) {
            Ok(message) => {
                self.status = message;
                // The footer's staged count changed with the action.
                let staged = git
                    .changes()
                    .map(|files| files.iter().filter(|file| file.is_staged()).count())
                    .unwrap_or(0);
                let branch = self
                    .branch_name
                    .clone()
                    .unwrap_or_else(|| "detached".to_string());
                self.repo_line = format!("{branch} · {staged} staged");
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    /// Re-read the branch list, the branch line in the footer, and the context
    /// the composer takes from the branch.
    pub fn reload_branches(&mut self) {
        let failure = {
            let (git, branches) = (&self.git, &mut self.branches);
            let Some(git) = git.as_deref() else {
                return;
            };
            branches.reload(git).err()
        };
        let (head, staged) = {
            let Some(git) = self.git.as_deref() else {
                return;
            };
            (
                git.current_branch().ok().flatten(),
                git.changes()
                    .map(|files| files.iter().filter(|file| file.is_staged()).count())
                    .unwrap_or(0),
            )
        };
        self.branch_name = head.map(|info| info.name);
        let branch = self
            .branch_name
            .clone()
            .unwrap_or_else(|| "detached".to_string());
        self.repo_line = format!("{branch} · {staged} staged");
        if let Some(error) = failure {
            self.status = error.to_string();
        }
        self.apply_branch_type();
        self.seed_footer();
    }

    /// Take the commit type from the branch kind — but only while the user has
    /// not chosen a type themselves.
    fn apply_branch_type(&mut self) {
        if self.type_touched {
            return;
        }
        let Some(name) = self.branch_name.as_deref() else {
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
        if self.commit.kind != kind {
            self.commit.kind = kind;
            self.revalidate();
        }
    }

    /// Keys of the second and third tab. The composer's keys must not fire here.
    fn branch_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.wizard.open {
            self.wizard_key(key);
            return;
        }
        // Tab and Shift+Tab walk the panes: branches, files, diff. The same key on
        // every tab, so the hand learns one gesture.
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            let back = key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
            if self.panel == Panel::Branches {
                // Coming back to the files: they may have changed in the editor.
                self.reload_staging();
            }
            self.panel = match (self.panel, back) {
                (Panel::Branches, false) => Panel::Files,
                (Panel::Files, false) => {
                    if self.staging.show_diff {
                        Panel::Diff
                    } else {
                        Panel::Branches
                    }
                }
                (Panel::Diff, false) => Panel::Branches,
                (Panel::Branches, true) => {
                    if self.staging.show_diff {
                        Panel::Diff
                    } else {
                        Panel::Files
                    }
                }
                (Panel::Files, true) => Panel::Branches,
                (Panel::Diff, true) => Panel::Files,
            };
            return;
        }
        if self.panel == Panel::Files {
            self.files_key(key);
            return;
        }
        if self.panel == Panel::Diff {
            let page = 10usize;
            match key.code {
                KeyCode::Esc | KeyCode::Char('v') => {
                    self.staging.show_diff = false;
                    self.panel = Panel::Files;
                }
                KeyCode::Down => self.staging.scroll_diff(1, page),
                KeyCode::Up => self.staging.scroll_diff(-1, page),
                KeyCode::PageDown => self.staging.scroll_diff(page as isize, page),
                KeyCode::PageUp => self.staging.scroll_diff(-(page as isize), page),
                KeyCode::Home => self.staging.diff_offset = 0,
                KeyCode::End => self.staging.scroll_diff(isize::MAX / 2, page),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') if ctrl => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = true,
            // Only the arrows move through the list: `j`/`k` and `g`/`G` were a
            // second vocabulary nobody asked for.
            KeyCode::Down => {
                self.branches.confirming = None;
                self.branches.move_by(1);
            }
            KeyCode::Up => {
                self.branches.confirming = None;
                self.branches.move_by(-1);
            }
            KeyCode::Home => {
                self.branches.confirming = None;
                self.branches.jump_to_start();
            }
            KeyCode::End => {
                self.branches.confirming = None;
                self.branches.jump_to_end();
            }
            KeyCode::Enter => self.checkout_selected(),
            KeyCode::Char('m') => self.merge_selected(),
            KeyCode::Char('d') => self.delete_selected(),
            KeyCode::Char('r') => self.reload_branches(),
            KeyCode::Char('f') => self.fetch_remote(),
            KeyCode::Char('n') => self.wizard.start(),
            KeyCode::Esc => {
                // Esc is the cancel key while a deletion waits for its second
                // press; otherwise it leaves the program as everywhere else.
                if self.branches.confirming.take().is_none() {
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
            KeyCode::Esc => self.wizard.close(),
            KeyCode::Char('c') if ctrl => {
                self.wizard.close();
                self.should_quit = true;
            }
            KeyCode::Tab => self.wizard.field = self.wizard.field.next(),
            KeyCode::BackTab => self.wizard.field = self.wizard.field.previous(),
            // The same rules as in the composer: `←`/`→` are the text, `↑`/`↓`
            // walk the fields, `Tab` does both the obvious way.
            KeyCode::Left | KeyCode::Right => {
                let has_text = self.wizard.current_text().is_some();
                match navigation::horizontal(key.code, has_text) {
                    Some(navigation::Horizontal::Caret(delta)) => self.wizard.move_cursor(delta),
                    Some(navigation::Horizontal::Value(delta)) => self.wizard.cycle_kind(delta > 0),
                    None => {}
                }
            }
            KeyCode::Up => self.wizard.step(-1),
            KeyCode::Down => self.wizard.step(1),
            KeyCode::Home => self.wizard.jump(false),
            KeyCode::End => self.wizard.jump(true),
            KeyCode::Delete => self.wizard.delete(),
            KeyCode::Backspace => self.wizard.backspace(),
            KeyCode::Enter => self.create_branch(),
            KeyCode::Char(c) if !ctrl => self.wizard.push_char(c),
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
        let (git, wizard) = (&self.git, &mut self.wizard);
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
        self.branches.current().cloned()
    }

    fn checkout_selected(&mut self) {
        let Some(branch) = self.selected_branch() else {
            return;
        };
        if branch.is_remote {
            self.status = format!("{} is remote · check out a local branch", branch.name);
            return;
        }
        let Some(git) = self.git.as_deref() else {
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
        let Some(git) = self.git.as_deref() else {
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
        if self.branches.confirming.as_deref() != Some(branch.name.as_str()) {
            self.branches.confirming = Some(branch.name.clone());
            self.status = format!("press d again to delete {}", branch.name);
            return;
        }
        let Some(git) = self.git.as_deref() else {
            self.status = "no repository".to_string();
            return;
        };
        self.branches.confirming = None;
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
        self.in_repository = found;
        if !found {
            let hint = "not a git repository · `check` works here, committing needs one";
            self.status = if self.status.is_empty() {
                hint.to_string()
            } else {
                format!("{} · {hint}", self.status)
            };
        }
    }

    /// Hand the composer a way to read a commit's file list when it is needed.
    pub fn set_files_loader(&mut self, loader: FilesLoader) {
        self.files_loader = Some(loader);
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

        if !self.files_cache.contains_key(&id) {
            let Some(loader) = self.files_loader.as_ref() else {
                return;
            };
            let files = loader(&id);
            self.files_cache.insert(id.clone(), files);
        }
        let files = self.files_cache.get(&id).cloned().unwrap_or_default();
        let Some(commit) = self.history.iter().find(|commit| commit.id == id) else {
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
        self.type_touched = true;
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
        // The preview for the highlighted commit gets its file list now; the
        // rest is fetched when the selection moves there.
        self.refresh_history_preview();
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
                self.type_touched = true;
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
                self.type_touched = true;
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

    fn focus(&mut self, index: usize) {
        self.field = index % FIELDS.len();
        let len = self.value(self.active_field()).chars().count();
        self.cursors[self.field] = self.cursors[self.field].min(len);
    }

    /// A field is on screen unless it is optional and the form is compact.
    pub fn field_visible(&self, field: Field) -> bool {
        !self.compact || !field.optional()
    }

    fn next_field(&mut self) {
        self.step_field(1);
    }

    fn previous_field(&mut self) {
        self.step_field(-1);
    }

    /// Move to the next field that is actually on screen. Hidden fields keep
    /// their content, so walking past them must not clear or skip anything else.
    fn step_field(&mut self, delta: isize) {
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

    /// `Ctrl+O`: fold the optional fields away, or bring them back. The choice is
    /// written to the user config right away, so it survives the restart.
    fn toggle_compact_form(&mut self) {
        self.compact = !self.compact;
        self.config.compact_form = self.compact;
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
        self.status = match (self.compact, saved) {
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
        let typing = (self.tab == tabs::Tab::Commit && self.is_editing()) || self.wizard.open;
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
                        self.refresh_sync();
                        // Every tab opens on its main pane, never on whatever was
                        // focused the last time it was looked at.
                        self.panel = Panel::Branches;
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

    /// `←`/`→`: always the text, on every tab.
    fn arrow(&mut self, key: KeyEvent) {
        let has_text = self.active_field() != Field::Type;
        match navigation::horizontal(key.code, has_text) {
            Some(navigation::Horizontal::Caret(delta)) => self.move_cursor(delta),
            Some(navigation::Horizontal::Value(delta)) => self.cycle_type(delta > 0),
            None => {}
        }
    }

    /// `↑`/`↓`: between fields, or through the lines of a multiline field.
    fn vertical(&mut self, key: KeyEvent) {
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
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    self.sync_with_remote(true);
                    return;
                }
                KeyCode::Char('u') | KeyCode::Char('U') => {
                    self.sync_with_remote(false);
                    return;
                }
                _ => {}
            }
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

        // Tab 1 has two sides: the staging list and the composer. Ctrl+L hands
        // the keyboard over and Esc gives it back.
        if self.tab == tabs::Tab::Commit {
            if key.code == KeyCode::Char('l') && ctrl {
                self.staging.side = match self.staging.side {
                    Side::Composer => Side::Staging,
                    Side::Staging => Side::Composer,
                };
                return;
            }
            if self.staging.side == Side::Staging {
                self.staging_key(key);
                return;
            }
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
            KeyCode::Up | KeyCode::Down => self.vertical(key),
            // `h`/`l` on the type field cycle the value, as vi users expect; the
            // arrow keys go through the shared rule below.
            KeyCode::Char('l') if self.active_field() == Field::Type => self.cycle_type(true),
            KeyCode::Char('h') if self.active_field() == Field::Type => self.cycle_type(false),
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
mod tests {
    use super::*;
    use crate::application::test_support::RecordingGit;

    fn app_on_the_branch_tab(git: RecordingGit) -> App {
        let mut app = App::default();
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        press(&mut app, KeyCode::Char('2'));
        app
    }

    fn staging_app() -> (App, std::rc::Rc<std::cell::RefCell<Vec<String>>>) {
        let git = RecordingGit::new(&["main"], "main");
        git.files
            .borrow_mut()
            .push(commitsmith::domain::ports::ChangedFile {
                path: "a.txt".to_string(),
                index: None,
                worktree: Some(commitsmith::domain::ports::FileState::Modified),
            });
        let log = git.calls();
        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        (app, log)
    }

    #[test]
    fn ctrl_l_hands_the_keyboard_to_the_staging_list() {
        let (mut app, _) = staging_app();
        assert_eq!(app.staging.side, staging::Side::Composer);
        press(&mut app, KeyCode::Char('l')); // plain l is text in the composer
        assert_eq!(app.staging.side, staging::Side::Composer);
        ctrl(&mut app, 'l');
        assert_eq!(app.staging.side, staging::Side::Staging);
        press(&mut app, KeyCode::Esc);
        assert_eq!(
            app.staging.side,
            staging::Side::Composer,
            "Esc gives it back"
        );
    }

    #[test]
    fn space_stages_and_v_shows_the_diff() {
        let (mut app, log) = staging_app();
        // The files live on tab 2 now; Tab puts the keyboard on them.
        app.tab = tabs::Tab::Branches;
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char(' '));
        assert!(log.borrow().iter().any(|call| call == "stage a.txt"));
        // The footer names the file; the status only says what happened.
        assert_eq!(app.status, "staged");

        press(&mut app, KeyCode::Char('v'));
        assert!(app.staging.show_diff);
        assert!(app.staging.diff.contains("diff --git"));
    }

    #[test]
    fn the_arrows_move_through_the_branch_list() {
        let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
        assert_eq!(app.branches.selected, 0, "HEAD stays on top");
        press(&mut app, KeyCode::Down);
        assert_eq!(app.branches.selected, 1);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.branches.selected, 1, "the list ends here");
        press(&mut app, KeyCode::Up);
        assert_eq!(app.branches.selected, 0);
        press(&mut app, KeyCode::End);
        assert_eq!(app.branches.selected, 1);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.branches.selected, 0);
    }

    #[test]
    fn j_and_k_no_longer_move_the_branch_list() {
        let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
        type_text(&mut app, "jk");
        assert_eq!(app.branches.selected, 0, "letters are not navigation here");
    }

    #[test]
    fn enter_checks_out_the_selected_branch() {
        let git = RecordingGit::new(&["main", "feature"], "main");
        let mut app = app_on_the_branch_tab(git);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.status, "checked out feature");
        assert_eq!(app.repo_line, "feature · 0 staged");
        assert!(app.branches.current().unwrap().is_head, "list reloaded");
    }

    #[test]
    fn deleting_a_branch_takes_a_second_press() {
        let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(
            app.branches.confirming.as_deref(),
            Some("feature"),
            "the first press only asks"
        );
        assert!(app.status.contains("press d again"), "{}", app.status);
        assert_eq!(app.branches.len(), 2, "nothing deleted yet");

        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.branches.len(), 1, "the second press deletes");
        assert_eq!(app.status, "deleted feature");
        assert!(app.branches.confirming.is_none());
    }

    #[test]
    fn esc_cancels_the_deletion_instead_of_quitting() {
        let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Esc);
        assert!(!app.should_quit, "Esc only cancels the pending deletion");
        assert!(app.branches.confirming.is_none());
        assert_eq!(app.branches.len(), 2);

        press(&mut app, KeyCode::Esc);
        assert!(app.should_quit, "a second Esc leaves");
    }

    #[test]
    fn the_checked_out_branch_cannot_be_deleted() {
        let git = RecordingGit::new(&["main", "feature"], "main");
        let mut app = app_on_the_branch_tab(git);
        press(&mut app, KeyCode::Char('d'));
        assert!(app.status.contains("is checked out"), "{}", app.status);
        assert_eq!(app.branches.len(), 2);
    }

    #[test]
    fn a_merge_conflict_is_reported_and_the_app_stays_usable() {
        let mut git = RecordingGit::new(&["main", "feature"], "main");
        git.merge_fails = true;
        let mut app = app_on_the_branch_tab(git);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('m'));
        assert!(!app.status.is_empty());
        assert!(!app.should_quit);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.branches.selected, 0, "still navigable");
    }

    #[test]
    fn the_actions_go_through_the_repository_port() {
        let git = RecordingGit::new(&["main", "feature"], "main");
        let log = git.calls();
        let mut app = App::default();
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        press(&mut app, KeyCode::Char('2'));

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        // `feature` is checked out now, so the next row is `main`; merging it
        // into the current branch is the normal direction.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('m'));
        assert_eq!(
            log.borrow().as_slice(),
            ["checkout feature", "merge main"],
            "git must be told about both, in that order"
        );
    }

    fn wizard_app(git: RecordingGit) -> App {
        let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
        app.config.author = Some("hima".to_string());
        app.config.ticket_prefix = "TICKET".to_string();
        app.config.branch_template = "{type}/{author}/{ticket}/{slug}".to_string();
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        press(&mut app, KeyCode::Char('2'));
        app
    }

    #[test]
    fn n_opens_the_wizard_and_esc_closes_it_again() {
        let mut app = wizard_app(RecordingGit::new(&["main"], "main"));
        press(&mut app, KeyCode::Char('n'));
        assert!(app.wizard.open);
        press(&mut app, KeyCode::Esc);
        assert!(!app.wizard.open, "Esc closes the wizard");
        assert!(!app.should_quit, "and does not leave the program");
    }

    #[test]
    fn enter_creates_the_branch_and_checks_it_out() {
        let git = RecordingGit::new(&["main"], "main");
        let log = git.calls();
        let mut app = wizard_app(git);
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Tab); // ticket
        press(&mut app, KeyCode::Tab); // description
        type_text(&mut app, "Add a thing");
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.status, "created and checked out feat/hima/add-a-thing");
        assert!(!app.wizard.open, "the wizard closes on success");
        assert_eq!(
            log.borrow().as_slice(),
            [
                "create feat/hima/add-a-thing",
                "checkout feat/hima/add-a-thing"
            ]
        );
    }

    #[test]
    fn a_ticket_lands_in_the_name() {
        let git = RecordingGit::new(&["main"], "main");
        let log = git.calls();
        let mut app = wizard_app(git);
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "1234");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "add a thing");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.status,
            "created and checked out feat/hima/TICKET-1234/add-a-thing"
        );
        assert_eq!(log.borrow()[0], "create feat/hima/TICKET-1234/add-a-thing");
    }

    #[test]
    fn a_duplicate_branch_keeps_the_wizard_open() {
        let mut git = RecordingGit::new(&["main"], "main");
        git.create_fails = true;
        let mut app = wizard_app(git);
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "thing");
        press(&mut app, KeyCode::Enter);

        assert!(app.wizard.open, "the user can correct and try again");
        assert!(app.wizard.error.is_some());
        assert!(app.status.contains("already exists"), "{}", app.status);
    }

    #[test]
    fn the_wizard_takes_the_keys_away_from_the_list() {
        let mut app = wizard_app(RecordingGit::new(&["main", "feature"], "main"));
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.branches.selected, 0, "j does not move the list");
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.wizard.description, "j");
        // `j` is text, not navigation — in the wizard and outside it.
    }

    fn wizard_on_description(git: RecordingGit) -> App {
        let mut app = wizard_app(git);
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        app
    }

    #[test]
    fn arrows_in_the_wizard_move_through_the_text() {
        let mut app = wizard_on_description(RecordingGit::new(&["main"], "main"));
        type_text(&mut app, "abc");
        let field = app.wizard.field;
        press(&mut app, KeyCode::Left);
        assert_eq!(app.wizard.field, field, "left stays in the field");
        assert_eq!(app.wizard.cursor(), 2);
        press(&mut app, KeyCode::Char('X'));
        assert_eq!(
            app.wizard.description, "abXc",
            "typing happens at the caret"
        );
        press(&mut app, KeyCode::Home);
        assert_eq!(app.wizard.cursor(), 0);
        press(&mut app, KeyCode::Delete);
        assert_eq!(app.wizard.description, "bXc");
        press(&mut app, KeyCode::End);
        assert_eq!(app.wizard.cursor(), 3);
    }

    #[test]
    fn arrows_in_an_empty_wizard_field_move_between_fields() {
        let mut app = wizard_on_description(RecordingGit::new(&["main"], "main"));
        // Down/Up are the field keys, also inside an empty field.
        press(&mut app, KeyCode::Up);
        assert_eq!(app.wizard.field, branch_wizard::WizardField::Ticket);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.wizard.field, branch_wizard::WizardField::Description);
        // Left/Right stay on the (empty) text.
        press(&mut app, KeyCode::Left);
        assert_eq!(app.wizard.field, branch_wizard::WizardField::Description);
        assert_eq!(app.wizard.cursor(), 0);
    }

    #[test]
    fn arrows_on_the_kind_field_cycle_it() {
        let mut app = wizard_app(RecordingGit::new(&["main"], "main"));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(
            app.wizard.kind,
            commitsmith::domain::branch::BranchKind::Feat
        );
        press(&mut app, KeyCode::Right);
        assert_eq!(
            app.wizard.kind,
            commitsmith::domain::branch::BranchKind::Fix
        );
        press(&mut app, KeyCode::Left);
        assert_eq!(
            app.wizard.kind,
            commitsmith::domain::branch::BranchKind::Feat
        );
        assert_eq!(
            app.wizard.field,
            branch_wizard::WizardField::Kind,
            "and stay"
        );
    }

    #[test]
    fn the_composer_follows_the_same_rules() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.active_field(), Field::Scope);

        // Left/Right are the text — also when it is empty.
        press(&mut app, KeyCode::Right);
        assert_eq!(app.active_field(), Field::Scope, "still in the field");

        type_text(&mut app, "abc");
        press(&mut app, KeyCode::Left);
        assert_eq!(app.active_field(), Field::Scope);
        assert_eq!(app.cursor(), 2);
        press(&mut app, KeyCode::Char('X'));
        assert_eq!(app.commit.scope, "abXc");

        // Up/Down are the fields.
        press(&mut app, KeyCode::Down);
        assert_eq!(app.active_field(), Field::Subject);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.active_field(), Field::Scope);
    }

    #[test]
    fn tab_and_shift_tab_step_through_every_field() {
        let mut app = App::default();
        for expected in [
            Field::Scope,
            Field::Subject,
            Field::Body,
            Field::Breaking,
            Field::Footer,
        ] {
            press(&mut app, KeyCode::Tab);
            assert_eq!(app.active_field(), expected);
        }
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.active_field(), Field::Type, "wraps to the start");
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.active_field(), Field::Footer, "and back");
    }

    #[test]
    fn the_composer_never_sees_keys_from_the_branch_tab() {
        let mut app = app_on_the_branch_tab(RecordingGit::new(&["main"], "main"));
        type_text(&mut app, "wxyz");
        assert_eq!(app.commit.subject, "", "typing must not reach the subject");
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn ctrl(app: &mut App, c: char) {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    #[test]
    fn ctrl_o_folds_the_optional_fields_and_keeps_their_text() {
        let mut app = App::with_config(Config::conventional(), Vec::new());
        // Body is field 3.
        while app.active_field() != Field::Body {
            app.next_field();
        }
        app.handle_paste("body text");

        ctrl(&mut app, 'o');
        assert!(app.compact, "Ctrl+O folds");
        assert_eq!(app.value(Field::Body), "body text");
        assert_ne!(
            app.active_field(),
            Field::Body,
            "the focus leaves a field that is no longer drawn"
        );

        // The folded text is still part of the message and still validated.
        app.commit.subject = "add a thing".to_string();
        let rendered = app.commit.render();
        assert!(
            rendered.contains("body text"),
            "a folded body still goes into the commit: {rendered}"
        );

        ctrl(&mut app, 'o');
        assert!(!app.compact, "Ctrl+O unfolds again");
        assert_eq!(app.value(Field::Body), "body text");
    }

    #[test]
    fn tab_skips_folded_fields_in_both_directions() {
        let mut app = App::with_config(Config::conventional(), Vec::new());
        ctrl(&mut app, 'o');
        assert!(app.compact);

        let order: Vec<Field> = (0..4)
            .map(|_| {
                let field = app.active_field();
                app.next_field();
                field
            })
            .collect();
        assert_eq!(
            order,
            vec![Field::Type, Field::Scope, Field::Subject, Field::Type],
            "forward: only the three visible fields"
        );

        let back: Vec<Field> = (0..3)
            .map(|_| {
                app.previous_field();
                app.active_field()
            })
            .collect();
        assert_eq!(
            back,
            vec![Field::Type, Field::Subject, Field::Scope],
            "backward: the same three, in reverse"
        );
    }

    #[test]
    fn the_compact_choice_is_written_to_the_config() {
        let mut app = App::with_config(Config::conventional(), Vec::new());
        let store = crate::application::test_support::RecordingConfig::default();
        let written = store.written.clone();
        app.set_config_store(Box::new(store));

        ctrl(&mut app, 'o');
        let text = written.borrow().clone();
        assert!(text.contains("compact_form = true"), "{text}");
        assert!(app.status.contains("compact"), "{}", app.status);
    }

    fn app_with_remote(
        sync: Option<(usize, usize)>,
    ) -> (App, std::rc::Rc<std::cell::RefCell<Vec<String>>>) {
        use crate::application::test_support::RecordingGit;
        let mut git = RecordingGit::new(&["main"], "main");
        git.sync = sync;
        let calls = git.calls();
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.branch_name = Some("main".to_string());
        app.set_git(Box::new(git));
        (app, calls)
    }

    fn alt(app: &mut App, c: char) {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
    }

    #[test]
    fn a_pull_without_upstream_names_the_command_that_creates_one() {
        let (mut app, _) = app_with_remote(None);
        alt(&mut app, 'p');
        assert_eq!(app.status, "no upstream for main · git push -u origin main");
    }

    #[test]
    fn a_pull_reports_how_many_commits_arrived() {
        let (mut app, calls) = app_with_remote(Some((0, 3)));
        alt(&mut app, 'p');
        assert!(app.status.contains("pulled 3 commits"), "{}", app.status);
        assert!(app.status.contains("into main"), "{}", app.status);
        assert!(calls.borrow().iter().any(|call| call == "pull"));
    }

    #[test]
    fn a_push_reports_how_many_commits_went_out() {
        let (mut app, calls) = app_with_remote(Some((2, 0)));
        alt(&mut app, 'u');
        assert!(app.status.contains("pushed 2 commits"), "{}", app.status);
        assert!(calls.borrow().iter().any(|call| call == "push"));
    }

    #[test]
    fn a_rejected_push_says_to_pull_first() {
        use crate::application::test_support::RecordingGit;
        let mut git = RecordingGit::new(&["main"], "main");
        git.sync = Some((1, 2));
        git.sync_error =
            Some("! [rejected] main -> main (non-fast-forward)\nerror: failed to push".to_string());
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.branch_name = Some("main".to_string());
        app.set_git(Box::new(git));

        alt(&mut app, 'u');
        assert!(app.status.contains("push rejected"), "{}", app.status);
        assert!(app.status.contains("pull first"), "{}", app.status);
    }

    #[test]
    fn a_diverged_pull_explains_the_conflict_without_merging() {
        use crate::application::test_support::RecordingGit;
        let mut git = RecordingGit::new(&["main"], "main");
        git.sync = Some((1, 1));
        git.sync_error = Some("fatal: Not possible to fast-forward, aborting.".to_string());
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.branch_name = Some("main".to_string());
        app.set_git(Box::new(git));

        alt(&mut app, 'p');
        assert!(app.status.contains("pull stopped"), "{}", app.status);
        assert!(app.status.contains("pull --rebase"), "{}", app.status);
    }

    #[test]
    fn the_sync_keys_belong_to_the_first_tab() {
        let (mut app, calls) = app_with_remote(Some((1, 0)));
        app.tab = tabs::Tab::Branches;
        alt(&mut app, 'u');
        assert!(
            !calls.borrow().iter().any(|call| call == "push"),
            "Alt+U on tab 2 must not push"
        );
    }

    #[test]
    fn the_footer_label_says_how_far_the_branch_is() {
        let (app, _) = app_with_remote(Some((2, 1)));
        assert_eq!(app.sync_label().as_deref(), Some("↑2↓1"));
        let (app, _) = app_with_remote(Some((0, 0)));
        assert_eq!(app.sync_label().as_deref(), Some("✓"));
        let (app, _) = app_with_remote(None);
        assert_eq!(app.sync_label().as_deref(), Some("–"));
    }

    #[test]
    fn a_cancelled_release_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("commitsmith-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        use crate::application::test_support::RecordingGit;
        let git = RecordingGit::new(&["main"], "main");
        let calls = git.calls();
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.tab = tabs::Tab::History;
        app.set_changelog_root(dir.clone());
        app.set_git(Box::new(git));
        app.refresh_plan();

        press(&mut app, KeyCode::Char('y'));
        assert!(app.release_confirm);
        press(&mut app, KeyCode::Esc);
        assert!(!app.release_confirm);
        assert!(
            calls.borrow().iter().all(|call| !call.starts_with("tag ")),
            "no tag after cancelling: {:?}",
            calls.borrow()
        );
        assert!(
            !dir.join("CHANGELOG.md").exists(),
            "no file after cancelling"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_confirmed_release_writes_the_file_and_no_tag() {
        let dir = std::env::temp_dir().join(format!("commitsmith-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        use crate::application::test_support::RecordingGit;
        let git = RecordingGit::new(&["main"], "main");
        git.commits
            .borrow_mut()
            .push(commitsmith::domain::history::PastCommit {
                short_id: "abc1234".to_string(),
                id: "abc1234".to_string(),
                author: "Marc".to_string(),
                date: "today".to_string(),
                summary: "add a button".to_string(),
                body: String::new(),
                kind: Some("feat".to_string()),
                scope: Some("ui".to_string()),
                files: Vec::new(),
            });
        let calls = git.calls();
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.tab = tabs::Tab::History;
        app.set_changelog_root(dir.clone());
        app.set_git(Box::new(git));
        app.refresh_plan();

        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Enter);

        let text = std::fs::read_to_string(dir.join("CHANGELOG.md")).unwrap();
        assert!(text.contains("## 0.1.0"), "{text}");
        assert!(text.contains("**ui**: add a button"), "{text}");
        // No tags until the project decides to use them.
        assert!(
            calls.borrow().iter().all(|call| !call.starts_with("tag ")),
            "no tag created: {:?}",
            calls.borrow()
        );
        assert!(app.status.contains("no tag set"), "{}", app.status);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_plan_names_the_next_version() {
        use crate::application::test_support::RecordingGit;
        let git = RecordingGit::new(&["main"], "main");
        git.commits
            .borrow_mut()
            .push(commitsmith::domain::history::PastCommit {
                short_id: "abc1234".to_string(),
                id: "abc1234".to_string(),
                author: "Marc".to_string(),
                date: "today".to_string(),
                summary: "add a button".to_string(),
                body: String::new(),
                kind: Some("feat".to_string()),
                scope: Some("ui".to_string()),
                files: Vec::new(),
            });
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.set_git(Box::new(git));
        app.refresh_plan();

        let plan = app.plan.as_ref().expect("a plan without tags");
        assert_eq!(plan.next.to_string(), "0.1.0", "no tag: the first release");
        assert_eq!(plan.commits.len(), 1);
    }

    #[test]
    fn fetch_is_a_tab_two_key_that_reloads_the_marks() {
        use crate::application::test_support::RecordingGit;
        let mut git = RecordingGit::new(&["main", "feature"], "main");
        git.sync_map.insert("main".to_string(), (0, 2));
        let calls = git.calls();
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.tab = tabs::Tab::Branches;
        app.set_git(Box::new(git));

        press(&mut app, KeyCode::Char('f'));
        assert!(
            calls.borrow().iter().any(|call| call == "fetch"),
            "{:?}",
            calls.borrow()
        );
        assert!(app.status.starts_with("fetched"), "{}", app.status);
        assert!(app.status.contains("1 branch"), "{}", app.status);
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
    fn digits_switch_tabs_when_no_field_is_typing() {
        let mut app = App::default();
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.tab, tabs::Tab::Branches);
        press(&mut app, KeyCode::Char('3'));
        assert_eq!(app.tab, tabs::Tab::History);
        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.tab, tabs::Tab::Commit);
    }

    #[test]
    fn a_digit_inside_a_field_stays_text() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "1st try");
        assert_eq!(app.commit.subject, "1st try");
        assert_eq!(app.tab, tabs::Tab::Commit, "typing must not change the tab");
    }

    #[test]
    fn the_tab_keeps_what_was_typed_in_it() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "add composer");
        // While a field is focused the digits are text, so the page keys are the
        // way out; the round trip must not lose the subject.
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
        assert_eq!(app.tab, tabs::Tab::Branches);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
        assert_eq!(app.tab, tabs::Tab::Commit);
        assert_eq!(app.commit.subject, "add composer");
    }

    #[test]
    fn a_digit_typed_in_a_field_does_not_leak_into_the_tab_bar() {
        let mut app = App::default();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "fix 2 bugs");
        assert_eq!(app.commit.subject, "fix 2 bugs");
        assert_eq!(app.tab, tabs::Tab::Commit);
    }

    #[test]
    fn ctrl_page_keys_cycle_tabs() {
        let mut app = App::default();
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
        assert_eq!(app.tab, tabs::Tab::Branches);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
        assert_eq!(app.tab, tabs::Tab::Commit);
        app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
        assert_eq!(app.tab, tabs::Tab::History, "wraps around");
    }

    #[test]
    fn digits_do_not_switch_tabs_while_a_picker_is_open() {
        let mut app = App::default();
        press(&mut app, KeyCode::Char(' ')); // type picker
        press(&mut app, KeyCode::Char('2'));
        assert_eq!(app.tab, tabs::Tab::Commit);
    }

    #[test]
    fn the_history_preview_loads_files_lazily_and_once() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let mut app = App::default();
        app.set_files_loader(Box::new(move |id: &str| {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![format!("M {id}.rs")]
        }));
        app.set_history(vec![
            PastCommit {
                id: "aaaa111".into(),
                short_id: "aaaa111".into(),
                summary: "first".into(),
                kind: Some("feat".into()),
                ..Default::default()
            },
            PastCommit {
                id: "bbbb222".into(),
                short_id: "bbbb222".into(),
                summary: "second".into(),
                kind: Some("fix".into()),
                ..Default::default()
            },
        ]);

        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one commit read on open");
        let preview = app.picker.as_ref().unwrap().preview().unwrap().to_string();
        assert!(preview.contains("M aaaa111.rs"), "{preview}");

        // Moving to the next commit reads exactly one more.
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let preview = app.picker.as_ref().unwrap().preview().unwrap().to_string();
        assert!(preview.contains("M bbbb222.rs"), "{preview}");

        // Coming back is a cache hit.
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "no second read for a cached commit"
        );
    }

    #[test]
    fn a_commit_without_files_still_previews() {
        let mut app = App::default();
        app.set_files_loader(Box::new(|_: &str| Vec::new()));
        app.set_history(vec![PastCommit {
            id: "cccc333".into(),
            short_id: "cccc333".into(),
            summary: "no files".into(),
            kind: Some("docs".into()),
            ..Default::default()
        }]);
        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        let preview = app.picker.as_ref().unwrap().preview().unwrap();
        assert!(preview.contains("docs: no files"), "{preview}");
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

    fn app_on_branch(config: Config, branch: &str) -> App {
        let git = RecordingGit::new(&[branch], branch);
        let mut app = App::with_config(config, Vec::new());
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        app
    }

    #[test]
    fn the_branch_ticket_lands_in_the_footer() {
        let mut config = Config::conventional();
        config.prefix = Some("hima".into());
        let app = app_on_branch(config, "feat/hima/TICKET-42/add-a-thing");
        assert_eq!(app.commit.footer, "Refs TICKET-42\nCo-authored-by: hima");
    }

    #[test]
    fn the_co_author_can_be_switched_off() {
        let mut config = Config::conventional();
        config.prefix = Some("hima".into());
        config.co_author = false;
        let app = app_on_branch(config, "feat/hima/TICKET-42/add-a-thing");
        assert_eq!(app.commit.footer, "Refs TICKET-42");
    }

    #[test]
    fn a_custom_co_author_name_is_used() {
        let mut config = Config::conventional();
        config.prefix = Some("hima".into());
        config.co_author_name = Some("Marc Hinte <marc@example.com>".into());
        let app = app_on_branch(config, "main");
        assert_eq!(
            app.commit.footer,
            "Co-authored-by: Marc Hinte <marc@example.com>"
        );
    }

    #[test]
    fn a_branch_without_a_ticket_carries_no_reference() {
        let mut config = Config::conventional();
        config.prefix = Some("hima".into());
        let app = app_on_branch(config, "fix/old");
        assert_eq!(app.commit.footer, "Co-authored-by: hima");
    }

    #[test]
    fn typing_in_the_footer_survives_a_branch_change() {
        let mut config = Config::conventional();
        config.prefix = Some("hima".into());
        let git = RecordingGit::new(&["main", "feat/hima/TICKET-7/next"], "main");
        let mut app = App::with_config(config, Vec::new());
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        app.commit.footer = "Closes OTHER-9".to_string();

        press(&mut app, KeyCode::Char('2'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.commit.footer, "Closes OTHER-9", "typing wins");
    }

    #[test]
    fn the_footer_follows_a_branch_the_wizard_created() {
        let mut config = Config::conventional();
        config.prefix = Some("hima".into());
        config.author = Some("hima".into());
        config.ticket_prefix = "TICKET".into();
        let mut app = app_on_branch(config, "main");
        assert_eq!(app.commit.footer, "Co-authored-by: hima");

        press(&mut app, KeyCode::Char('2'));
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "42");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "add a thing");
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.commit.footer, "Refs TICKET-42\nCo-authored-by: hima",
            "the branch's ticket and the co-author follow each other"
        );
    }

    #[test]
    fn the_branch_kind_sets_the_commit_type() {
        let config = Config::conventional();
        let app = app_on_branch(config, "hotfix/hima/TICKET-1/urgent");
        assert_eq!(app.commit.kind, "fix", "a hotfix is a fix for the message");
    }

    #[test]
    fn a_docs_branch_sets_docs() {
        let app = app_on_branch(Config::conventional(), "docs/hima/TICKET-2/readme");
        assert_eq!(app.commit.kind, "docs");
    }

    #[test]
    fn a_typed_type_is_never_overwritten_by_a_branch() {
        let git = RecordingGit::new(&["main", "chore/hima/TICKET-3/tidy"], "main");
        let mut app = App::with_config(Config::conventional(), Vec::new());
        app.set_in_repository(true);
        app.set_git(Box::new(git));
        // The user picks a type themselves.
        press(&mut app, KeyCode::Right);
        let chosen = app.commit.kind.clone();
        assert_ne!(chosen, "chore");

        press(&mut app, KeyCode::Char('2'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.commit.kind, chosen,
            "the branch must not win over the user"
        );
    }

    #[test]
    fn a_branch_kind_missing_from_the_profile_is_ignored() {
        let mut config = Config::conventional();
        // A house profile without `docs`: the branch kind has no counterpart.
        let mut profile = config.active().clone();
        profile.types.retain(|kind| kind.name != "docs");
        config.profiles.insert("house".into(), profile);
        config.profile = "house".into();
        assert!(!config.type_names().contains(&"docs".to_string()));

        let app = app_on_branch(config, "docs/hima/TICKET-4/readme");
        assert_ne!(app.commit.kind, "docs", "the profile has no such type");
        assert_eq!(app.commit.kind, "feat", "the default stays");
    }

    #[test]
    fn a_non_conventional_branch_leaves_the_type_alone() {
        let app = app_on_branch(Config::conventional(), "main");
        assert_eq!(app.commit.kind, "feat", "the default stays");
    }

    #[test]
    fn a_branch_from_the_wizard_sets_the_type_too() {
        let mut config = Config::conventional();
        config.author = Some("hima".into());
        config.prefix = Some("hima".into());
        config.ticket_prefix = "TICKET".into();
        config.branch_template = "{type}/{author}/{ticket}/{slug}".into();
        let mut app = app_on_branch(config, "main");
        assert_eq!(app.commit.kind, "feat");

        press(&mut app, KeyCode::Char('2'));
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Right); // feat -> fix
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "12");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "broken thing");
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.commit.kind, "fix", "the new branch is a fix branch");
        assert_eq!(app.commit.footer, "Refs TICKET-12\nCo-authored-by: hima");
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
