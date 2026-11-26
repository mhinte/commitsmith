//! Tab 3: the log's keyboard, its lint marks and the release write.

use super::release_view::Basis;
use super::*;

impl App {
    /// Write the changelog section and tag the release — but only after `y` armed
    /// it and Enter confirmed.
    pub(crate) fn write_release(&mut self) {
        self.release.confirm = false;
        let Some(plan) = self.release.plan.clone() else {
            self.status = "no release plan · nothing to write".to_string();
            return;
        };
        let Some(root) = self.release.changelog_root.clone() else {
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
    pub(crate) fn refresh_plan(&mut self) {
        let Some(git) = self.repo.git.as_deref() else {
            self.release.plan = None;
            return;
        };
        let tags = git.tags().unwrap_or_default();
        let last = commitsmith::domain::release::ReleasePlan::latest_tag(&tags);
        let (window, limit) = match self.release.basis {
            Basis::Tag => (last.as_deref(), self.config.history_depth.min(2000)),
            Basis::Recent(count) => (None, count),
        };
        let commits = git.commits_since(window, limit).unwrap_or_default();
        let mut plan = commitsmith::domain::release::ReleasePlan::build(last.as_deref(), commits);
        plan.basis = self.release.basis.label();
        self.release.plan = Some(plan);
    }

    /// How a past commit's message would be judged today: the same rules the
    /// composer and `commitsmith check` use.
    pub(crate) fn log_mark(&self, commit: &PastCommit) -> Severity {
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
    pub(crate) fn log_issues(&self, commit: &PastCommit) -> Vec<Issue> {
        let mut text = commit.header();
        if !commit.body.trim().is_empty() {
            text.push_str("\n\n");
            text.push_str(commit.body.trim_end());
        }
        validate::lint_with(&text, &self.rules)
    }

    /// Keys of tab 3. While the filter line is open it swallows the characters,
    /// so a ticket with digits cannot switch tabs.
    pub(crate) fn history_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Char('?') && self.release.log.input.is_none() {
            self.show_help = true;
            return;
        }
        if matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) && ctrl {
            self.should_quit = true;
            return;
        }
        // The release: `y` asks, Enter writes, Esc or `n` backs out.
        if self.release.confirm && self.release.log.input.is_none() {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') => self.write_release(),
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.release.confirm = false;
                    self.status = "release cancelled · file and tags untouched".to_string();
                }
                _ => {}
            }
            return;
        }
        if self.release.log.input.is_some() {
            match key.code {
                KeyCode::Esc => self.release.log.cancel_filter(),
                KeyCode::Enter => {
                    let types = self.config.type_names();
                    self.release.log.apply_filter(&types);
                    let len = self.release.log.visible(&self.history.commits).len();
                    self.release.log.clamp(len);
                    self.status = match self.release.log.filter.is_empty() {
                        true => "filter cleared".to_string(),
                        false => {
                            let matches = format!("{} matches", len);
                            format!("{} · {}", self.release.log.filter.describe(), matches)
                        }
                    };
                }
                KeyCode::Backspace => self.release.log.backspace(),
                KeyCode::Char(c) => self.release.log.push(c),
                _ => {}
            }
            return;
        }
        let len = self.release.log.visible(&self.history.commits).len();
        match key.code {
            KeyCode::Up => self.release.log.move_by(-1, len),
            KeyCode::Down => self.release.log.move_by(1, len),
            KeyCode::Home => self.release.log.selected = 0,
            KeyCode::End => self.release.log.selected = len.saturating_sub(1),
            KeyCode::Char('/') => self.release.log.open_filter(),
            KeyCode::Char('c') => self.release.log.clear_filter(),
            // `b` moves the window: last tag, last 20, last 50. A repository
            // without tags still gets an answer that is not "everything".
            KeyCode::Char('b') => {
                self.release.basis = self.release.basis.next();
                self.refresh_plan();
                let label = self
                    .release
                    .plan
                    .as_ref()
                    .map(|plan| plan.headline())
                    .unwrap_or_default();
                self.status = label;
            }
            // `e` writes the section to a file: a TUI list cannot be selected
            // with the mouse, and the text is what people paste into a release.
            KeyCode::Char('e') => self.export_plan(),
            KeyCode::Char('y') => {
                self.release.confirm = true;
                let next = self
                    .release
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

    /// Write the plan to `RELEASE-NOTES-<version>.md` next to the changelog.
    pub(crate) fn export_plan(&mut self) {
        let Some(plan) = self.release.plan.clone() else {
            self.status = "no plan to export yet".to_string();
            return;
        };
        let Some(root) = self.release.changelog_root.clone() else {
            self.status = "no directory to write to".to_string();
            return;
        };
        let name = format!("RELEASE-NOTES-{}.md", plan.next);
        let path = root.join(name);
        let text = commitsmith::domain::release::render_section(
            &plan,
            Some(commitsmith::infrastructure::clock::today()).as_deref(),
        );
        match std::fs::write(&path, text) {
            Ok(()) => self.status = format!("wrote {}", path.display()),
            Err(error) => self.status = format!("cannot write {}: {error}", path.display()),
        }
    }

    pub(crate) fn set_changelog_root(&mut self, root: std::path::PathBuf) {
        self.release.changelog_root = Some(root);
    }
}
