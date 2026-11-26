//! Talking to the remote: pull, push, fetch, and what the footer says about it.
//!
//! Split out of `application/mod.rs`, which had grown past the point where one
//! file could be read in a sitting. Behaviour is unchanged.

use super::*;

impl App {
    /// Read `(ahead, behind)` once, for the footer and for the wording of the
    /// pull/push replies.
    pub(crate) fn refresh_sync(&mut self) {
        let Some(git) = self.repo.git.as_deref() else {
            self.repo.sync = None;
            return;
        };
        self.repo.sync = git.ahead_behind().unwrap_or(None);
    }

    /// Alt+P / Alt+U: fetch and fast-forward, or send. Never merges: a divergent
    /// branch is something to resolve in git, not behind a keypress.
    pub(crate) fn sync_with_remote(&mut self, pull: bool) {
        let Some(git) = self.repo.git.as_deref() else {
            self.status = "not a git repository · nothing to sync".to_string();
            return;
        };
        let branch = self.branch_label();
        if self.repo.sync.is_none() {
            self.status = format!("no upstream for {branch} · git push -u origin {branch}");
            return;
        }
        let (ahead, behind) = self.repo.sync.unwrap_or((0, 0));
        let result = if pull { git.pull() } else { git.push() };
        match result {
            Ok(()) => {
                self.refresh_sync();
                let (new_ahead, new_behind) = self.repo.sync.unwrap_or((0, 0));
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

    /// `f` on tab 2: update the remote-tracking branches, then redraw the list so
    /// the sync marks are right. Nothing in the work tree changes.
    pub(crate) fn fetch_remote(&mut self) {
        let Some(git) = self.repo.git.as_deref() else {
            self.status = "not a git repository · nothing to fetch".to_string();
            return;
        };
        match git.fetch() {
            Ok(()) => {
                self.reload_branches();
                self.refresh_sync();
                let changed = self
                    .branches
                    .list
                    .items
                    .iter()
                    .filter(|branch| !branch.is_remote)
                    .filter(|branch| {
                        matches!(self.branches.list.sync.get(&branch.name), Some((ahead, behind)) if *ahead > 0 || *behind > 0)
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

    /// The branch name as the user knows it, for messages.
    pub(crate) fn branch_label(&self) -> String {
        self.repo
            .branch_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "HEAD".to_string())
    }

    /// How many files are staged right now, read from the index.
    pub(crate) fn staged_count(git: &dyn GitRepository) -> usize {
        git.changes()
            .map(|files| files.iter().filter(|file| file.is_staged()).count())
            .unwrap_or(0)
    }

    /// The "branch · N staged" line. One place builds it, so the header cannot
    /// disagree with itself between startup and the first stage.
    pub(crate) fn set_repo_summary(&mut self, staged: usize) {
        let branch = self.branch_label();
        self.repo.line = format!("{branch} · {staged} staged");
    }

    /// Rewrite the "branch · N staged" line after something was staged.
    pub(crate) fn refresh_repo_line(&mut self) {
        let Some(staged) = self.repo.git.as_deref().map(Self::staged_count) else {
            return;
        };
        self.set_repo_summary(staged);
    }
}
