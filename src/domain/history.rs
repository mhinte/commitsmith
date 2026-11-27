//! History and scopes as plain data.
//!
//! These types carry what a repository read produced; they know nothing about
//! `gix`, so the ports and the application can use them directly.

use std::collections::BTreeSet;

use super::text;

/// What a scope picker needs to offer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ScopeInfo {
    /// Scopes seen in commit history, most recent first.
    pub history: Vec<String>,
    /// Extra scopes from the config file.
    pub configured: Vec<String>,
}

impl ScopeInfo {
    /// Configured scopes first, then history, without duplicates.
    pub fn all(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.configured
            .iter()
            .chain(self.history.iter())
            .filter(|scope| !scope.is_empty())
            .filter(|scope| seen.insert((*scope).clone()))
            .cloned()
            .collect()
    }

    /// Scopes matching `prefix`, case-insensitive, prefixes first.
    pub fn matching(&self, prefix: &str) -> Vec<String> {
        let needle = prefix.to_lowercase();
        let mut matches: Vec<(usize, String)> = self
            .all()
            .into_iter()
            .filter_map(|scope| {
                let lower = scope.to_lowercase();
                if needle.is_empty() || lower.starts_with(&needle) {
                    Some((lower.find(&needle).unwrap_or(0), scope))
                } else if lower.contains(&needle) {
                    Some((1, scope))
                } else {
                    None
                }
            })
            .collect();
        matches.sort();
        matches.into_iter().map(|(_, scope)| scope).collect()
    }
}

/// A commit from history, offered as a template for the message being written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PastCommit {
    /// Abbreviated hash, 7 characters.
    pub short_id: String,
    /// Full hash, used to look the commit up again.
    pub id: String,
    pub author: String,
    pub date: String,
    /// First line of the message.
    pub summary: String,
    /// Body, without footer lines and without the summary.
    pub body: String,
    /// Commit type, when the message parses as Conventional Commit.
    pub kind: Option<String>,
    pub scope: Option<String>,
    /// Files changed, as `M src/app.rs` style lines.
    pub files: Vec<String>,
    /// True for a commit with more than one parent: `git merge`, or the commit
    /// GitLab writes when it merges a request. It carries no conventional
    /// message, so it must not be counted as a rule violation.
    pub merge: bool,
}

impl PastCommit {
    /// Conventional, scope, breaking marker and summary, or the raw first line.
    pub fn header(&self) -> String {
        match (&self.kind, &self.scope) {
            (Some(kind), Some(scope)) => format!("{kind}({scope}): {}", self.summary),
            (Some(kind), None) => format!("{kind}: {}", self.summary),
            _ => self.summary.clone(),
        }
    }

    /// True when the message is a Conventional Commit.
    pub fn conventional(&self) -> bool {
        self.kind.is_some()
    }

    /// Picker label: hash, type and summary, cropped to `width`.
    pub fn label(&self, width: usize) -> String {
        let kind = match (&self.kind, &self.scope) {
            (Some(kind), Some(scope)) => format!("{kind}({scope})"),
            (Some(kind), None) => kind.clone(),
            _ if self.conventional() => "-".to_string(),
            _ => "plain".to_string(),
        };
        let summary = text::fit(&self.summary, width.saturating_sub(28));
        format!("{} {:<14} {}", self.short_id, kind, summary)
    }

    /// What the preview pane shows for this commit.
    pub fn preview(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.header());
        if !self.body.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(self.body.trim_end());
        }
        out.push_str(&format!("\n\n{} · {}", self.author, self.date));
        if !self.files.is_empty() {
            out.push('\n');
            for file in self.files.iter().take(8) {
                out.push_str(&format!("  {file}\n"));
            }
            if self.files.len() > 8 {
                out.push_str(&format!("  … {} more\n", self.files.len() - 8));
            }
        }
        out.trim_end().to_string()
    }

    /// The pieces a writer can copy: type, scope, body, breaking change.
    pub fn template(&self) -> Template {
        Template {
            kind: self.kind.clone(),
            scope: self.scope.clone(),
            body: self.body.clone(),
            breaking: String::new(),
        }
    }
}

/// Fields taken over from a past commit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Template {
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub body: String,
    pub breaking: String,
}
