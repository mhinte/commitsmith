//! Reading the repository: scopes from history, staged changes and recent
//! commits to reuse as a template.

use std::collections::BTreeSet;
use std::path::Path;

use crate::domain::errors::GitError;
use crate::domain::history::{PastCommit, ScopeInfo};
use crate::domain::ports::{BranchInfo, FileState};
use gix::bstr::ByteSlice;

/// Open the repository at `path`, if there is one.
pub fn open(path: &Path) -> Option<gix::Repository> {
    gix::discover(path).ok()
}

// How often a message went through the parser, counted in tests only: the walk
// must parse each header once, and a counter is the only honest proof.
//
// Per thread, because the suite runs its tests in parallel and a global counter
// mixed two walks into one number.
#[cfg(test)]
thread_local! {
    static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn count_parse() {
    PARSES.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
fn parses() -> usize {
    PARSES.with(std::cell::Cell::get)
}

#[cfg(test)]
fn reset_parses() {
    PARSES.with(|count| count.set(0));
}

/// Parse a raw commit message with the same parser the linter uses.
pub fn parse_scope(message: &str) -> Option<String> {
    parse_event(message).and_then(|(_, scope, _, _)| scope)
}

/// Type, scope and breaking flag of a Conventional Commit header.
fn parse_event(message: &str) -> Option<(String, Option<String>, bool, String)> {
    use conventional_commit_parser::commit::CommitType;

    #[cfg(test)]
    count_parse();

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

/// The last `limit` commits, but not the ones reachable from `stop`.
///
/// `stop` is the tag or merge base the changelog has to start after. It is
/// passed to the walk as a hidden tip, so the walk never reads the commits below
/// it: reading `limit` commits and cutting the list afterwards read up to 2000
/// messages for a plan that needed three.
pub fn commits_until(
    repo: &gix::Repository,
    stop: Option<gix::ObjectId>,
    limit: usize,
) -> Vec<PastCommit> {
    let mut commits = Vec::new();
    let Ok(head) = repo.head_id() else {
        return commits;
    };
    let Ok(walk) = repo
        .rev_walk([head.detach()])
        .with_hidden(stop)
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
    else {
        return commits;
    };
    for info in walk.take(limit).flatten() {
        if let Some(walked) = walked_commit(repo, info.id) {
            commits.push(walked.entry);
        }
    }
    commits
}

/// Commits since a tag, so the changelog does not repeat history.
///
/// The tag is resolved to a commit and the walk stops right before it; an unknown
/// tag (or none) yields the whole window instead of nothing.
pub fn commits_since(repo: &gix::Repository, since: Option<&str>, limit: usize) -> Vec<PastCommit> {
    let stop = since.and_then(|tag| {
        repo.rev_parse_single(format!("{tag}^{{commit}}").as_str())
            .ok()
            .map(gix::Id::detach)
    });
    commits_until(repo, stop, limit)
}

/// One commit as the tool needs it: the template form and the scope it shows.
pub struct Walked {
    pub entry: PastCommit,
    /// The scope from the header, for the picker's list.
    pub scope: Option<String>,
}

/// Read one commit once.
///
/// `snapshot` and `recent_commits` both used to walk the same commits and each
/// parsed the header up to three times (once for the scope, once for the type,
/// once to strip it), and built the id string twice. One pass answers all three
/// questions, and the scope comes out of the same parse as the type.
fn walked_commit(repo: &gix::Repository, id: gix::ObjectId) -> Option<Walked> {
    let commit = repo.find_commit(id).ok()?;
    let message = commit.message().ok()?;
    let title = message.title.to_str_lossy().trim().to_string();
    if title.is_empty() {
        return None;
    }
    let parsed = parse_event(&title);
    let scope = parsed.as_ref().and_then(|(_, scope, _, _)| scope.clone());
    let (kind, summary) = match parsed {
        Some((kind, _, _, plain)) => (Some(kind), plain),
        None => (None, title),
    };
    let body = message
        .body
        .map(|body| body.to_str_lossy().to_string())
        .unwrap_or_default();

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
    let id = id.to_string();
    let merge = commit.parent_ids().count() > 1;

    Some(Walked {
        entry: PastCommit {
            short_id: id[..7].to_string(),
            id,
            author,
            date,
            summary,
            body: clean_body(&body),
            kind,
            scope: scope.clone(),
            files: Vec::new(),
            merge,
        },
        scope,
    })
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
        // File lists are not read here: one `git show` per commit is what made
        // the start in a long history take seconds. They are fetched for the
        // commit the picker highlights, on demand.
        if let Some(walked) = walked_commit(repo, info.id) {
            commits.push(walked.entry);
        }
    }

    commits
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

/// Files changed by one commit, read when someone looks at it.
pub fn commit_files(repo: &gix::Repository, id: gix::ObjectId) -> Vec<String> {
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

/// A repository opened for reading.
///
/// The free functions above are what the older code calls directly; the struct
/// is the adapter's entry point, so the application never sees `gix` types.
pub struct GixRepo {
    repo: gix::Repository,
    workdir: std::path::PathBuf,
}

impl GixRepo {
    pub fn open(path: &Path) -> Result<Self, GitError> {
        // Not `GitError::not_found`: that one talks about a reference inside a
        // repository, and here there is no repository at all.
        let repo = gix::discover(path).map_err(|_| {
            GitError::invalid_state(format!("no git repository at {}", path.display()))
        })?;
        let workdir = repo
            .workdir()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf());
        Ok(Self { repo, workdir })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Local branches, then remote ones, `HEAD` first among the locals.
    pub fn branches(&self) -> Result<Vec<BranchInfo>, GitError> {
        let head = self
            .repo
            .head_name()
            .ok()
            .flatten()
            .map(|name| name.shorten().to_string());
        let mut branches = Vec::new();
        for reference in self
            .repo
            .references()
            .map_err(|error| GitError::invalid_state(error.to_string()))?
            .local_branches()
            .map_err(|error| GitError::invalid_state(error.to_string()))?
            .flatten()
        {
            let name = reference.name().shorten().to_string();
            let is_head = head.as_deref() == Some(name.as_str());
            branches.push(BranchInfo::local(name, is_head));
        }
        for reference in self
            .repo
            .references()
            .map_err(|error| GitError::invalid_state(error.to_string()))?
            .remote_branches()
            .map_err(|error| GitError::invalid_state(error.to_string()))?
            .flatten()
        {
            branches.push(BranchInfo::remote(reference.name().shorten().to_string()));
        }
        // HEAD first, then locals alphabetically, then remotes.
        branches.sort_by(|a, b| {
            b.is_head
                .cmp(&a.is_head)
                .then(a.is_remote.cmp(&b.is_remote))
                .then(a.name.cmp(&b.name))
        });
        Ok(branches)
    }

    /// The branch `HEAD` points at, or `None` for a detached head.
    pub fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
        Ok(self.branches()?.into_iter().find(|b| b.is_head))
    }

    pub fn recent_commits(&self, limit: usize) -> Result<Vec<PastCommit>, GitError> {
        Ok(recent_commits(&self.repo, limit))
    }

    pub fn scopes(&self, configured: &[String], depth: usize) -> Result<ScopeInfo, GitError> {
        Ok(scope_info(&self.repo, configured, depth))
    }

    /// Commits from `HEAD` back to (not including) the commit `stop`.
    pub fn commits_before(&self, stop: &str, limit: usize) -> Result<Vec<PastCommit>, GitError> {
        let stop = self
            .repo
            .rev_parse_single(format!("{stop}^{{commit}}").as_str())
            .ok()
            .map(gix::Id::detach);
        Ok(commits_until(&self.repo, stop, limit))
    }

    pub fn commits_since(
        &self,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PastCommit>, GitError> {
        Ok(commits_since(&self.repo, since, limit))
    }

    /// Tags, newest first is not guaranteed: git sorts them by name here.
    pub fn tags(&self) -> Result<Vec<String>, GitError> {
        let mut tags: Vec<String> = self
            .repo
            .references()
            .map_err(|error| GitError::invalid_state(error.to_string()))?
            .tags()
            .map_err(|error| GitError::invalid_state(error.to_string()))?
            .flatten()
            // `shorten()` already drops `refs/tags/`; trimming it again only
            // hid the case where it did not.
            .map(|tag| tag.name().shorten().to_string())
            .collect();
        tags.sort();
        Ok(tags)
    }
}

/// Turn a `git status --porcelain` code into a file state.
pub fn file_state(code: char) -> FileState {
    match code {
        'A' => FileState::Added,
        'D' => FileState::Deleted,
        'R' | 'C' => FileState::Renamed,
        'U' => FileState::Conflicted,
        '?' => FileState::Untracked,
        _ => FileState::Modified,
    }
}

/// Scopes and recent commits from a single walk.
///
/// Two walks (one for scopes, one for the picker) opened up to 700 commits at
/// startup. One walk over the larger depth answers both questions.
pub fn snapshot(
    repo: &gix::Repository,
    configured: &[String],
    scope_depth: usize,
    commit_limit: usize,
) -> (ScopeInfo, Vec<PastCommit>) {
    let mut scopes: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut commits = Vec::new();
    let depth = scope_depth.max(commit_limit);

    if let Ok(head) = repo.head_id() {
        if let Ok(walk) = repo
            .rev_walk([head.detach()])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
        {
            for (index, info) in walk.take(depth).flatten().enumerate() {
                let Some(walked) = walked_commit(repo, info.id) else {
                    continue;
                };
                if let Some(scope) = walked.scope {
                    if seen.insert(scope.clone()) {
                        scopes.push(scope);
                    }
                }
                // Beyond the commit window only the scopes are wanted.
                if index < commit_limit {
                    commits.push(walked.entry);
                }
            }
        }
    }

    (
        ScopeInfo {
            history: scopes,
            configured: configured.to_vec(),
        },
        commits,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(name: &str) -> (std::path::PathBuf, gix::Repository) {
        let dir = std::env::temp_dir().join(format!(
            "commitsmith-snapshot-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "s@example.com"],
            vec!["config", "user.name", "S"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .current_dir(&dir)
                .status()
                .unwrap();
        }
        for (subject, file) in [
            ("feat(api): first", "a.txt"),
            ("fix(ui): second", "b.txt"),
            ("chore: third", "c.txt"),
        ] {
            std::fs::write(dir.join(file), subject).unwrap();
            std::process::Command::new("git")
                .args(["add", file])
                .current_dir(&dir)
                .status()
                .unwrap();
            std::process::Command::new("git")
                .args(["commit", "-q", "-m", subject])
                .current_dir(&dir)
                .status()
                .unwrap();
        }
        let repo = gix::discover(&dir).unwrap();
        (dir, repo)
    }

    #[test]
    fn one_walk_fills_scopes_and_history() {
        let (dir, repo) = temp_repo("walk");
        let (scopes, commits) = snapshot(&repo, &["configured".to_string()], 50, 10);
        assert_eq!(commits.len(), 3);
        assert_eq!(commits[0].summary, "third", "newest first");
        assert!(scopes.all().contains(&"api".to_string()), "{:?}", scopes);
        assert!(scopes.all().contains(&"ui".to_string()));
        assert!(scopes.all().contains(&"configured".to_string()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_merge_commit_is_marked_as_one() {
        let (dir, _repo) = temp_repo("merge-mark");
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(&dir)
                    .args(args)
                    .status()
                    .unwrap()
                    .success(),
                "{args:?}"
            );
        };
        git(&["checkout", "-q", "-b", "side"]);
        std::fs::write(dir.join("side.txt"), "x").unwrap();
        git(&["add", "side.txt"]);
        git(&["commit", "-q", "-m", "feat(side): a side change"]);
        git(&["checkout", "-q", "main"]);
        std::fs::write(dir.join("main.txt"), "y").unwrap();
        git(&["add", "main.txt"]);
        git(&["commit", "-q", "-m", "fix(main): a main change"]);
        git(&[
            "merge",
            "--no-ff",
            "-q",
            "-m",
            "Merge branch 'side' into 'main'",
            "side",
        ]);

        let repo = gix::discover(&dir).unwrap();
        let (_scopes, commits) = snapshot(&repo, &[], 10, 10);
        let merge = commits.iter().find(|commit| commit.merge).expect("a merge");
        assert!(merge.summary.contains("Merge branch"), "{}", merge.summary);
        assert_eq!(
            commits.iter().filter(|commit| commit.merge).count(),
            1,
            "only the merge commit itself"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_plan_stops_reading_at_the_tag() {
        // Three commits, a tag on the oldest: the walk must read the two above it
        // and stop, not read a 300-commit window and cut the list afterwards.
        let (dir, repo) = temp_repo("stop-at-tag");
        let status = std::process::Command::new("git")
            .current_dir(&dir)
            .args(["tag", "-a", "v1.0.0", "-m", "first", "HEAD~2"])
            .status()
            .unwrap();
        assert!(status.success());

        reset_parses();
        let commits = commits_since(&repo, Some("v1.0.0"), 300);
        assert_eq!(
            commits
                .iter()
                .map(|c| c.summary.as_str())
                .collect::<Vec<_>>(),
            ["third", "second"],
            "the tagged commit itself is not part of the plan"
        );
        assert_eq!(parses(), 2, "only the commits above the tag were parsed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unknown_tag_still_yields_the_whole_window() {
        let (dir, repo) = temp_repo("unknown-tag");
        let commits = commits_since(&repo, Some("v9.9.9"), 10);
        assert_eq!(commits.len(), 3, "no tag found: nothing to subtract");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_walk_parses_each_header_once() {
        // The snapshot used to parse the same header three times: once for the
        // scope, once for the type, once to strip it. Three commits, three
        // parses is the contract.
        let (dir, repo) = temp_repo("parse-once");
        reset_parses();
        let (scopes, commits) = snapshot(&repo, &[], 50, 10);
        assert_eq!(commits.len(), 3);
        assert_eq!(scopes.history.len(), 2, "api and ui, not chore");
        assert_eq!(parses(), 3, "one parse per commit and not one more");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_snapshot_does_not_run_git_show_per_commit() {
        // The old reader spawned `git show` once per commit, which is what made
        // a long history slow to open. A snapshot must leave the lists empty.
        let (dir, repo) = temp_repo("nofiles");
        let (_, commits) = snapshot(&repo, &[], 50, 10);
        assert!(commits.iter().all(|commit| commit.files.is_empty()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn files_are_read_for_one_commit_on_demand() {
        let (dir, repo) = temp_repo("files");
        let (_, commits) = snapshot(&repo, &[], 50, 10);
        let id = gix::ObjectId::from_hex(commits[0].id.as_bytes()).unwrap();
        let files = commit_files(&repo, id);
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(files[0].contains("c.txt"), "{files:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

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
            merge: false,
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
