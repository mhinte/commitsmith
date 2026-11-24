//! Reading the repository: scopes from history and files touched by a commit.

use std::collections::BTreeSet;
use std::path::Path;

use gix::bstr::ByteSlice;

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

    /// Scopes matching `prefix`, case-insensitive, best match first.
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

/// Open the repository at `path`, if there is one.
pub fn open(path: &Path) -> Option<gix::Repository> {
    gix::discover(path).ok()
}

/// Parse a raw commit message with the same parser the linter uses.
pub fn parse_scope(message: &str) -> Option<String> {
    conventional_commit_parser::parse(message.trim())
        .ok()
        .and_then(|commit| {
            commit
                .scope
                .filter(|scope| !scope.trim().is_empty())
                .map(|scope| scope.trim().to_string())
        })
}

/// Collect scopes from the last `depth` commits, newest first.
pub fn scope_info(repo: &gix::Repository, configured: &[String], depth: usize) -> ScopeInfo {
    let mut history = Vec::new();
    let mut seen = BTreeSet::new();

    if let Ok(head) = repo.head_id() {
        let walk = repo
            .rev_walk([head.detach()])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
            .ok();

        if let Some(walk) = walk {
            for info in walk.take(depth).flatten() {
                let Ok(commit) = repo.find_commit(info.id) else {
                    continue;
                };
                let Ok(message) = commit.message() else {
                    continue;
                };
                if let Some(scope) = parse_scope(message.title.to_str_lossy().as_ref()) {
                    if seen.insert(scope.clone()) {
                        history.push(scope);
                    }
                }
            }
        }
    }

    ScopeInfo {
        history,
        configured: configured.to_vec(),
    }
}

/// Markdown of the staged change set, e.g. `M src/app.rs`, one line per file.
/// Shelling out to git keeps this honest across index formats and is fast for
/// the handful of files a commit touches.
pub fn staged_summary(workdir: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["diff", "--cached", "--name-status"])
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.replace('\t', "  "))
            .collect(),
        _ => Vec::new(),
    }
}

/// Fallback used by tests and the CLI when no repository is present.
pub fn empty_scope_info(configured: &[String]) -> ScopeInfo {
    ScopeInfo {
        history: Vec::new(),
        configured: configured.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scope_from_message() {
        assert_eq!(
            parse_scope("feat(api): add rate limit").as_deref(),
            Some("api")
        );
        assert_eq!(parse_scope("feat: no scope").as_deref(), None);
        assert_eq!(parse_scope("not a commit"), None);
        assert_eq!(
            parse_scope("fix(ui)!: drop menu\n\nBREAKING CHANGE: gone").as_deref(),
            Some("ui")
        );
    }

    #[test]
    fn configured_scopes_come_first_and_dedupe() {
        let info = ScopeInfo {
            history: vec!["api".into(), "ui".into()],
            configured: vec!["ui".into(), "db".into()],
        };
        assert_eq!(info.all(), vec!["ui", "db", "api"]);
    }

    #[test]
    fn matching_prefers_prefixes() {
        let info = ScopeInfo {
            history: vec!["web-api".into(), "api".into(), "ui".into()],
            configured: Vec::new(),
        };
        let matches = info.matching("api");
        assert_eq!(matches, vec!["api", "web-api"]);
        assert_eq!(info.matching("").len(), 3);
        assert!(info.matching("zzz").is_empty());
    }

    #[test]
    fn empty_input_offers_everything() {
        let info = empty_scope_info(&["db".into()]);
        assert_eq!(info.matching(""), vec!["db"]);
    }
}
