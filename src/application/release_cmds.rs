//! Tab 3: the log's keyboard, its lint marks and the release write.

use super::release_view::Basis;
use super::*;

impl App {
    /// Rebuild the release plan: last version tag, the commits since it, and what
    /// they demand. Reading only — nothing is written until the user asks.
    pub(crate) fn refresh_plan(&mut self) {
        let Some(git) = self.repo.git.as_deref() else {
            self.release.plan = None;
            return;
        };
        let tags = git.tags().unwrap_or_default();
        let last = commitsmith::domain::release::ReleasePlan::latest_tag(&tags);
        let limit = self.config.history_depth.min(2000);
        let (commits, base) = match self.release.basis {
            Basis::Tag => (
                git.commits_since(last.as_deref(), limit)
                    .unwrap_or_default(),
                None,
            ),
            Basis::Recent(count) => (git.commits_since(None, count).unwrap_or_default(), None),
            // Only this branch's own work: the merge base with the main branch is
            // the honest cut, and it needs no tags at all.
            Basis::Branch => match self.main_branch(git) {
                Some(base) => (
                    git.commits_not_on(&base, limit).unwrap_or_default(),
                    Some(base),
                ),
                None => (git.commits_since(None, limit).unwrap_or_default(), None),
            },
        };
        let mut plan = commitsmith::domain::release::ReleasePlan::build(last.as_deref(), commits);
        plan.basis = self.release.basis.label_for(base.as_deref());
        self.release.plan = Some(plan);
    }

    /// How a past commit's message would be judged today: the same rules the
    /// composer and `commitsmith check` use.
    pub(crate) fn log_mark(&self, commit: &PastCommit) -> Severity {
        // A merge commit has no conventional message to check, and marking it
        // red would say the history is broken when it is only a merge.
        if commit.merge {
            return Severity::Info;
        }
        self.history
            .mark(commit)
            .map(|mark| mark.severity)
            .unwrap_or_else(|| history_view::severity_of(&self.log_issues(commit)))
    }

    /// The concrete findings for a past commit, so the detail pane can say which
    /// rule broke instead of only that something did.
    pub(crate) fn log_issues(&self, commit: &PastCommit) -> Vec<Issue> {
        if commit.merge {
            return Vec::new();
        }
        match self.history.mark(commit) {
            Some(mark) => mark.issues.clone(),
            // A commit that arrived after the last rebuild: judge it once, the
            // caller does not care where the answer came from.
            None => validate::lint_with(&history_view::lint_text(commit), &self.rules),
        }
    }

    /// Keys of tab 3. While the filter line is open it swallows the characters,
    /// so a ticket with digits cannot switch tabs.
    pub(crate) fn history_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Char('?') && self.release.log.input.is_none() {
            self.show_help = true;
            return;
        }
        if matches!(key.code, KeyCode::Char('c' | 'q')) && ctrl {
            self.should_quit = true;
            return;
        }
        // Rewording a local commit: the line takes typing until Enter or Esc.
        if self.release.reword.is_some() {
            match key.code {
                KeyCode::Enter => self.apply_reword(),
                KeyCode::Backspace => {
                    if let Some(reword) = self.release.reword.as_mut() {
                        if reword.text.pop().is_none() {
                            self.note("the message must not be empty");
                        }
                    }
                }
                KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                    if let Some(reword) = self.release.reword.as_mut() {
                        reword.text.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        if self.release.log.input.is_some() {
            match key.code {
                KeyCode::Enter => {
                    let types = self.config.type_names();
                    self.release.log.apply_filter(&types);
                    let len = self.release.log.visible(&self.history.commits).len();
                    self.release.log.clamp(len);
                    let text = match self.release.log.filter.is_empty() {
                        true => "filter cleared".to_string(),
                        false => {
                            let matches = format!("{} matches", len);
                            format!("{} · {}", self.release.log.filter.describe(), matches)
                        }
                    };
                    self.note(text);
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
                    .map(commitsmith::domain::ReleasePlan::headline)
                    .unwrap_or_default();
                // A note: it is informative and must not sit in the footer until
                // an unrelated action happens to overwrite it.
                self.note(label);
            }
            // `e` writes the section to a file: a TUI list cannot be selected
            // with the mouse, and the text is what people paste into a release.
            KeyCode::Char('e') => self.export_plan(),
            // `E` rewrites the message of a commit that is not published yet.
            KeyCode::Char('E') => self.start_reword(),
            _ => {}
        }
    }

    /// `E`: open the message of the selected commit for editing, if it is local.
    pub(crate) fn start_reword(&mut self) {
        let Some(commit) = self
            .release
            .log
            .visible(&self.history.commits)
            .get(self.release.log.selected)
            .copied()
            .cloned()
        else {
            self.note("nothing selected");
            return;
        };
        let Some(git) = self.repo.git.as_deref() else {
            self.problem("not a git repository · nothing to rewrite");
            return;
        };
        match git.local_commits() {
            Ok(local) if local.contains(&commit.id) => {
                self.release.reword = Some(release_view::Reword {
                    target: commit.id.clone(),
                    short_id: commit.short_id.clone(),
                    // The whole header: editing only the summary and writing it
                    // back as the message dropped type and scope.
                    text: commit.header(),
                });
                self.note(format!(
                    "rewording {} · Enter writes it, Esc cancels",
                    commit.short_id
                ));
            }
            Ok(_) => self.problem(format!(
                "{} is already published · only local commits can be rewritten",
                commit.short_id
            )),
            Err(error) => self.problem(error.to_string()),
        }
    }

    /// Write the edited message and tell git to replace that commit.
    fn apply_reword(&mut self) {
        let Some(reword) = self.release.reword.clone() else {
            return;
        };
        if reword.text.trim().is_empty() {
            self.problem("the message must not be empty");
            return;
        }
        // The same rules as a new commit: rewriting history to a header the
        // hook would refuse helps nobody.
        let rules = validate::rules_from_config(&self.config);
        let issues = validate::lint_with(&reword.text, &rules);
        if let Some(issue) = issues
            .iter()
            .find(|issue| issue.severity == Severity::Error)
        {
            self.problem(format!("not written · {}", issue.message));
            return;
        }
        let depth = self.config.history_depth;
        let result = {
            let Some(git) = self.repo.git.as_deref() else {
                return;
            };
            match git.reword_header(&reword.target, &reword.text) {
                // The ids changed: the history has to be read again, or the log
                // shows commits that no longer exist under those ids.
                Ok(()) => git.recent_commits(depth).map_err(|error| error.to_string()),
                Err(error) => Err(error.to_string()),
            }
        };
        match result {
            Ok(commits) => {
                self.release.reword = None;
                self.set_history(commits);
                self.refresh_plan();
                self.note(format!(
                    "rewrote {} · the commits after it moved",
                    reword.short_id
                ));
            }
            Err(message) => self.problem(message),
        }
    }

    /// The branch this one was forked from: `main`, `master` or their remote
    /// counterparts, in that order. `Some` even when `HEAD` *is* that branch —
    /// the plan then simply has no commits, which is truthful.
    fn main_branch(&self, _git: &dyn GitRepository) -> Option<String> {
        // The branch list is already loaded for tab 2; reading it again spawned
        // a `for-each-ref` for an answer that is on screen.
        for candidate in ["main", "master", "origin/main", "origin/master"] {
            if self
                .branches
                .list
                .items
                .iter()
                .any(|branch| branch.name == candidate)
            {
                return Some(candidate.to_string());
            }
        }
        None
    }

    /// Write the plan to `RELEASE-NOTES-<window>.md` next to the changelog.
    pub(crate) fn export_plan(&mut self) {
        let Some(plan) = self.release.plan.clone() else {
            self.status = "no plan to export yet".to_string();
            return;
        };
        let Some(root) = self.release.changelog_root.clone() else {
            self.status = "no directory to write to".to_string();
            return;
        };
        // The name says which window produced the file: a release note whose
        // name is only a version cannot be told apart from another window's.
        let name = note_name(
            self.release.basis,
            plan.last_tag.as_deref(),
            self.base_branch(),
        );
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
    /// The branch the plan is measured against, for the file name.
    fn base_branch(&self) -> Option<String> {
        match self.repo.git.as_deref() {
            Some(git) => self.main_branch(git),
            None => None,
        }
    }
}

/// The file name of an exported release note, derived from the window it covers.
///
/// `RELEASE-NOTES-0.1.1.md` said nothing about what was counted; the name now
/// names the window: `RELEASE-NOTES-BRANCH-MAIN.md`,
/// `RELEASE-NOTES-LAST-20-COMMITS.md`, `RELEASE-NOTES-TAG-V1-2-0.md`.
fn note_name(basis: Basis, last_tag: Option<&str>, base: Option<String>) -> String {
    let window = match basis {
        Basis::Branch => format!("branch-{}", base.unwrap_or_else(|| "this".to_string())),
        Basis::Recent(count) => format!("last-{count}-commits"),
        Basis::Tag => match last_tag {
            Some(tag) => format!("tag-{tag}"),
            None => "no-tag".to_string(),
        },
    };
    let slug = commitsmith::domain::slug::BranchSlug::from_text(&window)
        .map(|slug| slug.as_str().to_uppercase())
        .unwrap_or_else(|_| "ALL".to_string());
    format!("RELEASE-NOTES-{slug}.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_name_names_the_window() {
        assert_eq!(
            note_name(Basis::Branch, Some("v1.0.0"), Some("main".into())),
            "RELEASE-NOTES-BRANCH-MAIN.md"
        );
        assert_eq!(
            note_name(Basis::Recent(20), None, Some("main".into())),
            "RELEASE-NOTES-LAST-20-COMMITS.md"
        );
        assert_eq!(
            note_name(Basis::Tag, Some("v1.2.0"), None),
            "RELEASE-NOTES-TAG-V1-2-0.md"
        );
        assert_eq!(
            note_name(Basis::Tag, None, None),
            "RELEASE-NOTES-NO-TAG.md",
            "a repository without tags says so instead of pretending"
        );
        assert_eq!(
            note_name(Basis::Branch, None, None),
            "RELEASE-NOTES-BRANCH-THIS.md"
        );
    }
}
