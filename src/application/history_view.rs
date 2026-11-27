//! The history the tool knows about: the commits, their file lists, the cache.
//!
//! Third step out of `App`. The commit picker on tab 1 and the log on tab 3 read
//! the same list, so it is one object, not a field of each pane.

use super::*;

#[derive(Default)]
pub struct History {
    /// Recent commits, offered by the picker and shown in the log.
    pub commits: Vec<PastCommit>,
    /// One entry per commit whose files have been read.
    pub cache: HashMap<String, Vec<String>>,
    /// How every commit would be judged today, one entry per commit id.
    ///
    /// The log asks this per drawn row and the event loop redraws five times a
    /// second, so judging inside `draw` meant parsing the same thirty messages
    /// over and over. It is a cache of a pure function: rebuilt when the list or
    /// the rules change, keyed by id so a filter cannot shift it.
    pub marks: HashMap<String, LintMark>,
}

/// One commit's verdict: the worst severity and the findings behind it.
#[derive(Debug, Clone)]
pub struct LintMark {
    pub severity: Severity,
    pub issues: Vec<Issue>,
}

impl LintMark {
    fn from_issues(issues: Vec<Issue>) -> Self {
        let severity = severity_of(&issues);
        Self { severity, issues }
    }
}

/// The worst severity in a finding list: an error outranks a warning, which
/// outranks the optional hints.
pub(crate) fn severity_of(issues: &[Issue]) -> Severity {
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

/// The message a past commit would be checked as: header, then the body.
pub(crate) fn lint_text(commit: &PastCommit) -> String {
    let mut text = commit.header();
    if !commit.body.trim().is_empty() {
        text.push_str("\n\n");
        text.push_str(commit.body.trim_end());
    }
    text
}

impl History {
    /// Judge every commit once, so a redraw costs a lookup instead of a parse.
    pub fn rebuild_marks(&mut self, rules: &Rules) {
        self.marks.clear();
        for commit in &self.commits {
            let issues = crate::validate::lint_with(&lint_text(commit), rules);
            self.marks
                .insert(commit.id.clone(), LintMark::from_issues(issues));
        }
    }

    /// The cached verdict, or `None` for a commit that was added after the last
    /// rebuild (a test, or a fresh commit).
    pub fn mark(&self, commit: &PastCommit) -> Option<&LintMark> {
        self.marks.get(&commit.id)
    }
}
