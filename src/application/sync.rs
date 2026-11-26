//! Talking to the remote: pull, push, fetch, and what the footer says about it.
//!
//! Split out of `application/mod.rs`, which had grown past the point where one
//! file could be read in a sitting. Behaviour is unchanged.

use super::*;

impl App {
    /// Read `(ahead, behind)` once, for the footer and for the wording of the
    /// pull/push replies.
    pub(crate) fn refresh_sync(&mut self) {
        let Some(git) = self.git.as_deref() else {
            self.sync = None;
            return;
        };
        self.sync = git.ahead_behind().unwrap_or(None);
    }

    /// Alt+P / Alt+U: fetch and fast-forward, or send. Never merges: a divergent
    /// branch is something to resolve in git, not behind a keypress.
    pub(crate) fn sync_with_remote(&mut self, pull: bool) {
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

    /// `f` on tab 2: update the remote-tracking branches, then redraw the list so
    /// the sync marks are right. Nothing in the work tree changes.
    pub(crate) fn fetch_remote(&mut self) {
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

    /// The branch name as the user knows it, for messages.
    pub(crate) fn branch_label(&self) -> String {
        self.branch_name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "HEAD".to_string())
    }

    /// Rewrite the "branch · N staged" line after something was staged.
    pub(crate) fn refresh_repo_line(&mut self) {
        let Some(git) = self.git.as_deref() else {
            return;
        };
        let staged = git
            .changes()
            .map(|files| files.iter().filter(|file| file.is_staged()).count())
            .unwrap_or(0);
        let branch = self.branch_label();
        self.repo_line = format!("{branch} · {staged} staged");
    }
}
