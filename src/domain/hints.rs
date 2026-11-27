//! Things worth saying before a commit is written, and never worth blocking for.
//!
//! The rules are deliberately few and cheap: they look at the index and the
//! branch name, nothing else. A hint that fires on a normal commit gets ignored
//! within a week, so silence is the default and every rule here has to earn its
//! place with a case that is actually wrong.

use crate::domain::branch::{BranchKind, ConventionalBranchName};

/// Above this many staged files a commit usually holds more than one change.
pub const MANY_FILES: usize = 25;

/// Source suffixes that are unambiguous: a `docs` branch touching these is
/// almost certainly a mistake, while a `chore` branch may legitimately touch
/// anything.
const CODE_SUFFIXES: [&str; 8] = [".rs", ".ts", ".tsx", ".js", ".py", ".go", ".java", ".c"];

/// Every hint worth printing for this index. Empty means: say nothing.
pub fn for_staged(branch: Option<&str>, staged: &[String], diff: &str) -> Vec<String> {
    let mut hints = Vec::new();
    if staged.len() > MANY_FILES {
        hints.push(format!(
            "{} files staged - one commit per change is easier to review",
            staged.len()
        ));
    }
    if let Some(branch) = branch {
        if let Some(hint) = branch_mismatch(branch, staged) {
            hints.push(hint);
        }
    }
    for path in conflict_paths(diff) {
        hints.push(format!("conflict markers left in {path}"));
    }
    hints
}

/// A `docs` branch that only touches source files. Any documentation path in the
/// list silences it: a release branch may well change both.
fn branch_mismatch(branch: &str, staged: &[String]) -> Option<String> {
    let kind = ConventionalBranchName::parse(branch)
        .ok()
        .map(|parsed| parsed.kind())
        .or_else(|| {
            let first = branch.split('/').next().unwrap_or_default().to_lowercase();
            BranchKind::ALL
                .iter()
                .copied()
                .find(|kind| kind.as_str() == first)
        })?;
    if kind != BranchKind::Docs || staged.is_empty() {
        return None;
    }
    let all_code = staged
        .iter()
        .all(|path| CODE_SUFFIXES.iter().any(|suffix| path.ends_with(suffix)));
    all_code.then(|| {
        format!(
            "a docs branch, but {} looks like source - wrong type or wrong branch?",
            staged.first().map_or("the file", String::as_str)
        )
    })
}

/// Paths whose *added* lines carry a complete conflict marker pair.
///
/// Both a `<<<<<<<` and a `>>>>>>>` are required: a lone `=======` is how a
/// Markdown setext heading underlines its title, and reporting every heading as
/// a conflict would make the hint worthless.
fn conflict_paths(diff: &str) -> Vec<String> {
    #[derive(Default)]
    struct Marker {
        start: bool,
        end: bool,
    }
    let mut seen: Vec<(String, Marker)> = Vec::new();
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            current = Some(rest.to_string());
            continue;
        }
        if line.starts_with("+++") {
            // `/dev/null` or a mode-only header: no path to name.
            current = None;
            continue;
        }
        let Some(path) = current.as_deref() else {
            continue;
        };
        let added = line.strip_prefix('+').unwrap_or_default();
        let start = added.starts_with("<<<<<<<");
        let end = added.starts_with(">>>>>>>");
        if !start && !end {
            continue;
        }
        let entry = match seen.iter_mut().find(|(seen, _)| seen == path) {
            Some((_, marker)) => marker,
            None => {
                seen.push((path.to_string(), Marker::default()));
                &mut seen.last_mut().expect("just pushed").1
            }
        };
        entry.start |= start;
        entry.end |= end;
    }
    seen.into_iter()
        .filter(|(_, marker)| marker.start && marker.end)
        .map(|(path, _)| path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{conflict_paths, for_staged};

    fn files(count: usize) -> Vec<String> {
        (0..count)
            .map(|index| format!("src/file{index}.rs"))
            .collect()
    }

    #[test]
    fn a_normal_commit_gets_no_hints_at_all() {
        assert!(for_staged(Some("feat/hima/TICKET-1/x"), &files(3), "").is_empty());
        assert!(for_staged(None, &files(2), "").is_empty());
        assert!(for_staged(Some("main"), &files(24), "").is_empty());
    }

    #[test]
    fn many_files_are_worth_one_line() {
        let hints = for_staged(Some("feat/hima/TICKET-1/x"), &files(26), "");
        assert_eq!(hints.len(), 1, "{hints:?}");
        assert!(hints[0].contains("26 files"), "{hints:?}");
        assert!(hints[0].chars().count() < 80, "{hints:?}");
    }

    #[test]
    fn a_docs_branch_touching_only_source_is_flagged() {
        let staged = vec!["src/app.rs".to_string(), "src/ui/mod.rs".to_string()];
        let hints = for_staged(Some("docs/hima/TICKET-4/readme"), &staged, "");
        assert_eq!(hints.len(), 1, "{hints:?}");
        assert!(hints[0].contains("docs branch"), "{hints:?}");
        assert!(hints[0].contains("src/app.rs"), "{hints:?}");
    }

    #[test]
    fn a_docs_branch_with_documentation_stays_quiet() {
        let staged = vec!["docs/usage.md".to_string(), "README.md".to_string()];
        assert!(for_staged(Some("docs/hima/TICKET-4/readme"), &staged, "").is_empty());
        // Mixed: one source file among documentation is not unambiguous.
        let staged = vec!["README.md".to_string(), "src/app.rs".to_string()];
        assert!(for_staged(Some("docs/hima/TICKET-4/readme"), &staged, "").is_empty());
    }

    #[test]
    fn conflict_markers_are_named_per_file_once() {
        let diff = "+++ b/src/app.rs\n+<<<<<<< HEAD\n+ours\n+=======\n+>>>>>>> theirs\n\
                    +++ b/src/other.rs\n+fn main() {}\n";
        let paths = conflict_paths(diff);
        assert_eq!(paths, vec!["src/app.rs".to_string()], "{paths:?}");
    }

    #[test]
    fn a_markdown_heading_underline_is_not_a_conflict() {
        // A setext heading adds a `=======` line; without a `<<<<<<<` pair it is
        // not a conflict, and this rule must not cry wolf on documentation.
        let diff = "+++ b/docs/usage.md\n+Usage\n+=======\n";
        assert!(conflict_paths(diff).is_empty());
    }

    #[test]
    fn a_half_marker_is_not_a_conflict() {
        let diff = "+++ b/src/app.rs\n+<<<<<<< HEAD\n+ours\n";
        assert!(conflict_paths(diff).is_empty());
    }

    #[test]
    fn a_marker_that_is_only_removed_is_not_a_hint() {
        let diff = "+++ b/src/app.rs\n-<<<<<<< HEAD\n+fn main() {}\n";
        assert!(conflict_paths(diff).is_empty());
        let hints = for_staged(Some("fix/hima/TICKET-2/x"), &files(1), diff);
        assert!(hints.is_empty(), "{hints:?}");
    }
}
