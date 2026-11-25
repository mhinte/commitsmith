//! Reading the repository: scopes from history, staged changes and recent
//! commits to reuse as a template.

use std::collections::BTreeSet;
use std::path::Path;

pub use commitsmith::domain::history::{PastCommit, ScopeInfo};
use gix::bstr::ByteSlice;

/// Open the repository at `path`, if there is one.
pub fn open(path: &Path) -> Option<gix::Repository> {
    gix::discover(path).ok()
}

/// Parse a raw commit message with the same parser the linter uses.
pub fn parse_scope(message: &str) -> Option<String> {
    parse_event(message).and_then(|(_, scope, _, _)| scope)
}

/// Type, scope and breaking flag of a Conventional Commit header.
fn parse_event(message: &str) -> Option<(String, Option<String>, bool, String)> {
    use conventional_commit_parser::commit::CommitType;

    let parsed = conventional_commit_parser::parse(message.trim()).ok()?;
    let breaking = parsed.is_breaking_change;
    let summary = parsed.summary.clone();
    let kind = match parsed.commit_type {
        // The library treats `refactor` as a custom type, so normalise the few
        // names it does not know itself.
        CommitType::Custom(name) => name,
        other => other.to_string(),
    };
    Some((kind, parsed.scope, breaking, summary))
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

/// The last `depth` commits, newest first, as templates.
pub fn recent_commits(repo: &gix::Repository, depth: usize) -> Vec<PastCommit> {
    let mut commits = Vec::new();
    let Ok(head) = repo.head_id() else {
        return commits;
    };
    let Ok(walk) = repo
        .rev_walk([head.detach()])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
    else {
        return commits;
    };

    for info in walk.take(depth).flatten() {
        let Ok(commit) = repo.find_commit(info.id) else {
            continue;
        };
        let summary = commit
            .message()
            .map(|message| message.title.to_str_lossy().trim().to_string())
            .unwrap_or_default();
        if summary.is_empty() {
            continue;
        }
        let full_message = commit
            .message()
            .map(|message| {
                message
                    .body
                    .map(|body| body.to_str_lossy().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        let (kind, scope) = match parse_event(&summary) {
            Some((kind, scope, _, _)) => (Some(kind), scope),
            None => (None, None),
        };

        let author = commit
            .author()
            .ok()
            .map(|author| author.name.to_str_lossy().to_string())
            .unwrap_or_default();
        let date = commit
            .time()
            .ok()
            .map(|time| {
                time.format(gix::date::time::format::ISO8601)
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let date = date.split('T').next().unwrap_or(&date).to_string();

        let files = commit_files(repo, info.id);

        commits.push(PastCommit {
            short_id: info.id.to_string()[..7].to_string(),
            id: info.id.to_string(),
            author,
            date,
            summary: strip_header(&summary),
            body: clean_body(&full_message),
            kind,
            scope,
            files,
        });
    }

    commits
}

/// Drop a `type(scope): ` prefix from a summary for display.
fn strip_header(summary: &str) -> String {
    match parse_event(summary) {
        Some((_, _, _, plain)) => plain,
        None => summary.to_string(),
    }
}

/// Remove footer lines such as `BREAKING CHANGE:` or `Refs:` from a body.
///
/// Footers only start after a blank line and at the end of the message, which is
/// what the spec asks for; a body line like `Fixes: the churn` stays.
fn clean_body(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut start = lines.len();

    for idx in (0..lines.len()).rev() {
        let line = lines[idx];
        if line.trim().is_empty() {
            if start == lines.len() && idx + 1 == lines.len() {
                continue;
            }
            start = idx;
            break;
        }
        if !is_footer_line(line) {
            start = idx + 1;
            break;
        }
        start = idx;
    }

    lines[..start].join("\n").trim().to_string()
}

fn is_footer_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("BREAKING CHANGE:") || upper.starts_with("BREAKING-CHANGE:") {
        return true;
    }
    match trimmed.split_once(": ") {
        Some((token, _)) => {
            !token.is_empty()
                && token.len() <= 24
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && token
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// Files touched by a commit, from `git show --name-status`.
fn commit_files(repo: &gix::Repository, id: gix::ObjectId) -> Vec<String> {
    let Some(workdir) = repo.workdir() else {
        return Vec::new();
    };
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["show", "--no-commit-id", "--name-status", "--format=", "-M"])
        .arg(id.to_string())
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.replace('\t', " "))
            .take(12)
            .collect(),
        _ => Vec::new(),
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

/// Shorten `text` to `width` characters, adding an ellipsis.
#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PastCommit {
        PastCommit {
            short_id: "abc1234".into(),
            id: "abc1234fedcba".into(),
            author: "Marc".into(),
            date: "2026-11-24".into(),
            summary: "add picker preview".into(),
            body: "The picker now previews the selected commit.".into(),
            kind: Some("feat".into()),
            scope: Some("composer".into()),
            files: vec!["M src/app.rs".into()],
        }
    }

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
    fn parses_known_and_custom_types() {
        let (kind, scope, breaking, summary) =
            parse_event("refactor(core)!: drop v1\n\nBREAKING CHANGE: gone").unwrap();
        assert_eq!(kind, "refactor");
        assert_eq!(scope.as_deref(), Some("core"));
        assert!(breaking);
        assert_eq!(summary, "drop v1");
    }

    #[test]
    fn header_uses_type_and_scope() {
        assert_eq!(sample().header(), "feat(composer): add picker preview");
        let mut plain = sample();
        plain.scope = None;
        plain.kind = None;
        assert_eq!(plain.header(), "add picker preview");
        assert!(!plain.conventional());
    }

    #[test]
    fn label_contains_hash_and_crops_long_summaries() {
        let mut commit = sample();
        commit.summary = "x".repeat(100);
        let label = commit.label(60);
        assert!(label.starts_with("abc1234 feat(composer)"));
        assert!(label.chars().count() <= 70);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn preview_lists_author_and_files() {
        let text = sample().preview();
        assert!(text.starts_with("feat(composer): add picker preview"));
        assert!(text.contains("Marc · 2026-11-24"));
        assert!(text.contains("M src/app.rs"));
    }

    #[test]
    fn template_carries_type_scope_and_body() {
        let template = sample().template();
        assert_eq!(template.kind.as_deref(), Some("feat"));
        assert_eq!(template.scope.as_deref(), Some("composer"));
        assert!(template.body.starts_with("The picker"));
    }

    #[test]
    fn body_cleaning_drops_footers() {
        let body = "why it changed\n\nRefs: #12\nSigned-off-by: Marc <m@example.com>";
        assert_eq!(clean_body(body), "why it changed");
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
        assert_eq!(info.matching("api"), vec!["api", "web-api"]);
        assert_eq!(info.matching("").len(), 3);
        assert!(info.matching("zzz").is_empty());
    }

    #[test]
    fn empty_input_offers_everything() {
        let info = empty_scope_info(&["db".into()]);
        assert_eq!(info.matching(""), vec!["db"]);
    }
}
