//! Version numbers and release plans.
//!
//! Pure: a plan is built from commits and a tag, and rendered to text. Writing the
//! file and creating the tag is somebody else's job (later, the git adapter).

use std::fmt;

use super::history::PastCommit;

/// A version as Semantic Versioning defines it: three numbers, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl SemVer {
    pub const START: Self = Self {
        major: 0,
        minor: 1,
        patch: 0,
    };

    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Accepts `1.2.3` and `v1.2.3`; anything else is `None`, so a tag like
    /// `nightly` cannot become a version.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self::new(major, minor, patch))
    }

    /// The next version for a bump. Pre-1.0 a breaking change moves the minor
    /// number: `0.x` is not stable, and jumping to 1.0.0 as a side effect of a
    /// refactor would be a promise nobody made.
    pub fn bump(self, bump: Bump) -> Self {
        match bump {
            Bump::Major if self.major == 0 => Self::new(0, self.minor + 1, 0),
            Bump::Major => Self::new(self.major + 1, 0, 0),
            Bump::Minor => Self::new(self.major, self.minor + 1, 0),
            Bump::Patch => Self::new(self.major, self.minor, self.patch + 1),
            Bump::None => self,
        }
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What the commits since the last tag demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bump {
    Major,
    Minor,
    Patch,
    None,
}

impl Bump {
    pub fn as_str(self) -> &'static str {
        match self {
            Bump::Major => "major",
            Bump::Minor => "minor",
            Bump::Patch => "patch",
            Bump::None => "none",
        }
    }
}

/// A commit group in the changelog, in the order the file lists them.
pub const GROUPS: [&str; 6] = ["feat", "fix", "perf", "refactor", "docs", "chore"];

/// Does this commit break something?
///
/// The `!` marker is gone once a message is parsed into kind and scope, so the
/// footer is what remains — and that is where `BREAKING CHANGE:` belongs anyway.
pub fn is_breaking(commit: &PastCommit) -> bool {
    commit.body.to_lowercase().contains("breaking change")
}

/// The ticket a commit mentions, if any: the first `WORD-123` in subject or body.
pub fn ticket_of(commit: &PastCommit) -> Option<String> {
    for text in [&commit.summary, &commit.body] {
        for word in text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
            let word = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            let mut parts = word.split('-');
            let Some(prefix) = parts.next() else { continue };
            let Some(number) = parts.next() else { continue };
            if prefix.len() >= 2
                && prefix.chars().all(|c| c.is_ascii_uppercase())
                && !number.is_empty()
                && number.chars().all(|c| c.is_ascii_digit())
            {
                return Some(format!("{prefix}-{number}"));
            }
        }
    }
    None
}

/// Everything the release needs to know, without touching anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePlan {
    pub last_tag: Option<String>,
    pub current: SemVer,
    pub next: SemVer,
    pub bump: Bump,
    /// Newest first, as they come from the log.
    pub commits: Vec<PastCommit>,
}

impl ReleasePlan {
    /// Build a plan from the commits since `last_tag`.
    pub fn build(last_tag: Option<&str>, commits: Vec<PastCommit>) -> Self {
        let current = last_tag.and_then(SemVer::parse).unwrap_or(SemVer::START);
        let bump = bump_for(&commits);
        // Nothing was ever released: the first version is 0.1.0, whatever the
        // commits are. Bumping on top of a version nobody saw would be fiction.
        let next = if last_tag.is_none() {
            SemVer::START
        } else {
            current.bump(bump)
        };
        Self {
            last_tag: last_tag.map(|tag| tag.to_string()),
            current,
            next,
            bump,
            commits,
        }
    }

    pub fn breaking(&self) -> Vec<&PastCommit> {
        self.commits
            .iter()
            .filter(|commit| is_breaking(commit))
            .collect()
    }

    /// The commits of one type, in log order.
    pub fn group(&self, kind: &str) -> Vec<&PastCommit> {
        self.commits
            .iter()
            .filter(|commit| commit.kind.as_deref() == Some(kind))
            .collect()
    }

    /// The highest version among `tags`; non-version tags are ignored.
    pub fn latest_tag(tags: &[String]) -> Option<String> {
        tags.iter()
            .filter_map(|tag| SemVer::parse(tag).map(|version| (version, tag.clone())))
            .max_by_key(|(version, _)| *version)
            .map(|(_, tag)| tag)
    }

    /// Title of the preview: what would be written and why.
    pub fn headline(&self) -> String {
        let since = match &self.last_tag {
            Some(tag) => format!("since {tag}"),
            None => "no tag yet · starting at 0.1.0".to_string(),
        };
        format!(
            "{} → {} ({}) · {} · {} commits",
            self.current,
            self.next,
            self.bump.as_str(),
            since,
            self.commits.len()
        )
    }
}

/// The highest bump any commit demands.
pub fn bump_for(commits: &[PastCommit]) -> Bump {
    let mut bump = Bump::None;
    for commit in commits {
        let wanted = if is_breaking(commit) {
            Bump::Major
        } else {
            match commit.kind.as_deref() {
                Some("feat") => Bump::Minor,
                Some("fix") | Some("perf") => Bump::Patch,
                _ => Bump::None,
            }
        };
        bump = match (bump, wanted) {
            (Bump::Major, _) | (_, Bump::Major) => Bump::Major,
            (Bump::Minor, _) | (_, Bump::Minor) => Bump::Minor,
            (Bump::Patch, _) | (_, Bump::Patch) => Bump::Patch,
            _ => Bump::None,
        };
    }
    bump
}

/// The markdown section for a plan: `## 0.16.0`, then one block per type.
pub fn render_section(plan: &ReleasePlan) -> String {
    let mut out = format!("## {}\n", plan.next);
    let breaking = plan.breaking();
    if !breaking.is_empty() {
        out.push_str("\n### Breaking changes\n\n");
        for commit in breaking {
            out.push_str(&format!("- {}\n", entry(commit)));
        }
    }
    for kind in GROUPS {
        let commits = plan.group(kind);
        if commits.is_empty() {
            continue;
        }
        out.push_str(&format!("\n### {}\n\n", title_for(kind)));
        for commit in commits {
            out.push_str(&format!("- {}\n", entry(commit)));
        }
    }
    out
}

fn title_for(kind: &str) -> &'static str {
    match kind {
        "feat" => "Features",
        "fix" => "Fixes",
        "perf" => "Performance",
        "refactor" => "Refactoring",
        "docs" => "Documentation",
        "chore" => "Chores",
        _ => "Other",
    }
}

/// One bullet: scope, subject and the ticket it belongs to.
fn entry(commit: &PastCommit) -> String {
    let mut text = match &commit.scope {
        Some(scope) => format!("**{scope}**: {}", commit.summary),
        None => commit.summary.clone(),
    };
    // Only add the ticket when the subject does not already name it.
    if let Some(ticket) = ticket_of(commit) {
        if !commit
            .summary
            .to_uppercase()
            .contains(&ticket.to_uppercase())
        {
            text.push_str(&format!(" ({ticket})"));
        }
    }
    text
}

/// Where a new section goes in an existing changelog: directly under the title,
/// above everything that is already there. Existing sections are never touched,
/// and a version that is already written is not written twice.
pub fn prepend(existing: &str, version: &SemVer, section: &str) -> String {
    let marker = format!("## {version}");
    if existing.contains(&marker) {
        return existing.to_string();
    }
    let section = section.trim_end();
    if existing.trim().is_empty() {
        return format!("# Changelog\n\n{section}\n");
    }
    // Keep the title (if any) on top, then the new section, then the rest.
    let (head, rest) = match existing.find("\n## ") {
        Some(position) => (&existing[..position], &existing[position..]),
        None => (existing, ""),
    };
    let head = head.trim_end();
    let rest = rest.trim_start_matches('\n');
    if rest.is_empty() {
        format!("{head}\n\n{section}\n")
    } else {
        format!("{head}\n\n{section}\n\n{rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(kind: &str, scope: Option<&str>, summary: &str, body: &str) -> PastCommit {
        PastCommit {
            short_id: "abc1234".to_string(),
            id: "abc1234".to_string(),
            author: "Marc".to_string(),
            date: "today".to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            kind: Some(kind.to_string()),
            scope: scope.map(|value| value.to_string()),
            files: Vec::new(),
        }
    }

    #[test]
    fn versions_parse_and_print() {
        assert_eq!(SemVer::parse("v1.2.3"), Some(SemVer::new(1, 2, 3)));
        assert_eq!(SemVer::parse("1.2"), None);
        assert_eq!(SemVer::parse("nightly"), None);
        assert_eq!(SemVer::new(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn the_bump_follows_the_commits() {
        let fixes = vec![commit("fix", None, "a", "")];
        assert_eq!(bump_for(&fixes), Bump::Patch);
        let feature = vec![commit("feat", None, "b", ""), commit("fix", None, "a", "")];
        assert_eq!(bump_for(&feature), Bump::Minor);
        let breaking = vec![commit("feat", None, "c", "BREAKING CHANGE: api")];
        assert_eq!(bump_for(&breaking), Bump::Major);
        let docs = vec![commit("docs", None, "d", "")];
        assert_eq!(bump_for(&docs), Bump::None);
    }

    #[test]
    fn pre_one_zero_a_breaking_change_moves_the_minor_number() {
        let plan = ReleasePlan::build(
            Some("v0.15.0"),
            vec![commit("feat", None, "x", "BREAKING CHANGE: yes")],
        );
        assert_eq!(plan.next, SemVer::new(0, 16, 0));
    }

    #[test]
    fn without_a_tag_the_plan_starts_at_zero_one_zero() {
        let plan = ReleasePlan::build(None, vec![commit("fix", None, "a", "")]);
        assert_eq!(plan.current, SemVer::START);
        assert_eq!(plan.next, SemVer::START, "the first release is 0.1.0");
        assert!(plan.headline().contains("no tag yet"));
    }

    #[test]
    fn the_section_groups_and_carries_tickets() {
        let plan = ReleasePlan::build(
            Some("v0.15.0"),
            vec![
                commit("feat", Some("ui"), "add the button (TICKET-42)", ""),
                commit("fix", None, "repair it", "Refs TICKET-7"),
                commit("chore", None, "bump", ""),
            ],
        );
        let text = render_section(&plan);
        assert!(text.starts_with("## 0.16.0"), "{text}");
        assert!(text.contains("### Features"), "{text}");
        // The subject already names the ticket; it is not repeated.
        assert!(
            text.contains("**ui**: add the button (TICKET-42)"),
            "{text}"
        );
        assert_eq!(text.matches("TICKET-42").count(), 1, "{text}");
        assert!(text.contains("### Fixes"), "{text}");
        assert!(text.contains("repair it (TICKET-7)"), "{text}");
    }

    #[test]
    fn breaking_changes_get_their_own_section() {
        let plan = ReleasePlan::build(
            Some("v0.15.0"),
            vec![commit(
                "feat",
                None,
                "change the api",
                "BREAKING CHANGE: renamed",
            )],
        );
        let text = render_section(&plan);
        assert!(text.contains("### Breaking changes"), "{text}");
        let breaking_at = text.find("### Breaking changes").unwrap();
        let features_at = text.find("### Features").unwrap();
        assert!(breaking_at < features_at, "breaking comes first");
    }

    #[test]
    fn a_new_section_goes_under_the_title_and_keeps_the_old_ones() {
        let existing = "# Changelog\n\n## 0.15.0\n\n### Features\n\n- old thing\n";
        let section = "## 0.16.0\n\n### Fixes\n\n- new thing";
        let text = prepend(existing, &SemVer::new(0, 16, 0), section);
        let title = text.find("# Changelog").unwrap();
        let new_at = text.find("## 0.16.0").unwrap();
        let old_at = text.find("## 0.15.0").unwrap();
        assert!(title < new_at && new_at < old_at, "{text}");
        assert!(text.contains("- old thing"), "old section kept: {text}");
    }

    #[test]
    fn a_version_that_is_already_there_is_not_written_twice() {
        let existing = "# Changelog\n\n## 0.16.0\n\n- done\n";
        let text = prepend(existing, &SemVer::new(0, 16, 0), "## 0.16.0\n- again");
        assert_eq!(text, existing);
        assert_eq!(text.matches("## 0.16.0").count(), 1);
    }

    #[test]
    fn an_empty_file_becomes_a_changelog() {
        let text = prepend("", &SemVer::new(0, 1, 0), "## 0.1.0\n\n- first");
        assert!(text.starts_with("# Changelog"), "{text}");
        assert!(text.contains("## 0.1.0"), "{text}");
    }

    #[test]
    fn a_ticket_is_recognised_in_both_places() {
        assert_eq!(
            ticket_of(&commit("feat", None, "add TICKET-42", "")).as_deref(),
            Some("TICKET-42")
        );
        assert_eq!(
            ticket_of(&commit("fix", None, "x", "Refs ABC-9")).as_deref(),
            Some("ABC-9")
        );
        assert_eq!(ticket_of(&commit("fix", None, "no ticket here", "")), None);
    }
}
