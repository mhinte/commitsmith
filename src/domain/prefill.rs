//! What a branch name says about a commit message, in one place.
//!
//! Two surfaces ask the same question: the composer fills its fields from the
//! branch, and the `prepare-commit-msg` hook fills the first line of a message a
//! person is about to type in an editor. They used to answer it separately and
//! drifted apart within a day — the hook put the ticket in the *scope*, the
//! composer put it in the *footer*, and only one of them understood the branch
//! aliases (`feature`, `bugfix`, `documentation`). Everything a branch tells us
//! lives here now; each surface decides how to write it down.
//!
//! Conventional Commits has no place for a ticket in the scope: the scope names
//! the part of the codebase (`auth`, `ui`), the reference belongs in the footer
//! (`Refs: TICKET-12`). The Type(Ticket) form the hook used to write was wrong.

use crate::domain::branch::{BranchKind, ConventionalBranchName};
use crate::domain::ticket;

/// Branch words that mean the same as a [`BranchKind`], for branches named
/// outside the template. Lower-case, and only the obvious ones: guessing `bug`
/// to be a `fix` is fine, guessing what `wip` means is not.
const KIND_WORDS: [(&str, BranchKind); 3] = [
    ("feature", BranchKind::Feat),
    ("bugfix", BranchKind::Fix),
    ("documentation", BranchKind::Docs),
];

/// What a branch says about the commits on it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    /// The commit type the branch implies, when it names one we know.
    pub kind: Option<&'static str>,
    /// The ticket in the branch name, in its canonical spelling.
    pub ticket: Option<String>,
}

impl Context {
    pub fn is_empty(&self) -> bool {
        self.kind.is_none() && self.ticket.is_none()
    }
}

/// Read a branch name. An empty result means the branch says nothing usable, and
/// every surface should then stay out of the way.
pub fn context(branch: &str) -> Context {
    let name = branch.trim();
    if name.is_empty() {
        return Context::default();
    }
    // The template knows best: the first segment is the kind, and a ticket sits
    // where the branch says it sits.
    if let Ok(parsed) = ConventionalBranchName::parse(name) {
        return Context {
            kind: Some(parsed.kind().commit_type()),
            ticket: parsed.ticket().map(ToString::to_string),
        };
    }
    // A renamed or foreign branch still starts with its kind often enough, and a
    // ticket-shaped word anywhere in the name is worth taking.
    let first = name.split('/').next().unwrap_or_default().to_lowercase();
    let kind = BranchKind::ALL
        .iter()
        .copied()
        .find(|kind| kind.as_str() == first)
        .or_else(|| {
            KIND_WORDS
                .iter()
                .find(|(word, _)| *word == first)
                .map(|(_, kind)| *kind)
        });
    Context {
        kind: kind.map(BranchKind::commit_type),
        ticket: ticket::in_text(name),
    }
}

/// The first line the hook writes: `feat: `, never `feat(TICKET-12): `.
///
/// The ticket goes into the footer, the way the composer does it. Without a kind
/// there is nothing worth writing — a bare ticket in the header is not a commit
/// message.
pub fn prefix(branch: &str) -> Option<String> {
    let context = context(branch);
    context.kind.map(|kind| format!("{kind}: "))
}

/// The footer lines a branch implies: the reference and, if asked, the co-author.
///
/// Shared with the composer, which used to build this string itself.
pub fn footer_lines(branch: &str, reference_keyword: &str, co_author: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(ticket) = context(branch).ticket {
        // The keyword is configured as the bare word (`Refs`); the line keeps the
        // spelling the composer always produced, because `validate` and the
        // example footers in the docs expect it.
        lines.push(format!("{reference_keyword} {ticket}"));
    }
    if let Some(name) = co_author.filter(|name| !name.trim().is_empty()) {
        lines.push(format!("Co-authored-by: {name}"));
    }
    lines
}

/// True when the message file holds nothing of the writer's own: blank lines and
/// comments only. Git fills `.git/COMMIT_EDITMSG` with comments, so "empty" here
/// cannot mean "zero bytes".
pub fn is_free(text: &str) -> bool {
    text.lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with('#')
    })
}

/// `text` with `prefix` in front, or `None` when there is writing to protect.
pub fn with_prefix(text: &str, prefix: &str) -> Option<String> {
    if !is_free(text) {
        return None;
    }
    let rest = text.trim_start_matches('\n');
    Some(match rest.trim().is_empty() {
        true => prefix.trim_end().to_string(),
        false => format!("{prefix}\n{rest}"),
    })
}

#[cfg(test)]
mod tests {
    use super::{context, footer_lines, is_free, prefix, with_prefix};

    #[test]
    fn a_conventional_branch_names_type_and_ticket() {
        let read = context("feat/mmu/TICKET-12/add-a-thing");
        assert_eq!(read.kind, Some("feat"));
        assert_eq!(read.ticket.as_deref(), Some("TICKET-12"));
        let read = context("fix/hima/PROJ-7");
        assert_eq!(read.kind, Some("fix"));
        assert_eq!(read.ticket.as_deref(), Some("PROJ-7"));
    }

    #[test]
    fn a_hotfix_branch_becomes_a_fix() {
        assert_eq!(context("hotfix/mmu/OPS-99/urgent").kind, Some("fix"));
    }

    #[test]
    fn the_branch_aliases_are_understood_everywhere() {
        // The composer used to miss these while the hook knew them.
        let read = context("feature/ABC-9");
        assert_eq!(read.kind, Some("feat"));
        assert_eq!(read.ticket.as_deref(), Some("ABC-9"));
        assert_eq!(context("bugfix/team/BUG-2").kind, Some("fix"));
        assert_eq!(context("documentation/readme").kind, Some("docs"));
        assert_eq!(context("docs/mmu/readme").kind, Some("docs"));
    }

    #[test]
    fn a_main_branch_says_nothing() {
        for branch in ["main", "master", "origin/main", "", "wip"] {
            assert!(context(branch).is_empty(), "{branch}");
            assert_eq!(prefix(branch), None, "{branch}");
        }
    }

    #[test]
    fn the_prefix_carries_the_type_and_never_the_ticket() {
        // Conventional Commits: the scope names the codebase, not a ticket.
        assert_eq!(
            prefix("feat/mmu/TICKET-12/add-a-thing").as_deref(),
            Some("feat: ")
        );
        assert_eq!(prefix("docs/mmu/readme").as_deref(), Some("docs: "));
        assert!(!prefix("feat/TICKET-3").unwrap().contains("TICKET-3"));
    }

    #[test]
    fn the_footer_takes_the_ticket_and_the_co_author() {
        assert_eq!(
            footer_lines("feat/mmu/TICKET-12/x", "Refs", None),
            vec!["Refs TICKET-12".to_string()]
        );
        assert_eq!(
            footer_lines("feat/mmu/TICKET-12/x", "Closes", Some("Marc H.")),
            vec![
                "Closes TICKET-12".to_string(),
                "Co-authored-by: Marc H.".to_string()
            ]
        );
        assert!(footer_lines("main", "Refs", None).is_empty());
        // An empty co-author name is not a line.
        assert_eq!(
            footer_lines("feat/mmu/TICKET-1/x", "Refs", Some("  ")),
            vec!["Refs TICKET-1".to_string()]
        );
    }

    #[test]
    fn comments_alone_are_not_content() {
        assert!(is_free(""));
        assert!(is_free("\n\n"));
        assert!(is_free("# Please enter the commit message\n#\n"));
        assert!(!is_free("feat: already written\n# comment\n"));
        assert!(!is_free("   something\n"));
    }

    #[test]
    fn the_prefix_lands_above_the_comments_and_never_over_writing() {
        let file = "# Please enter the commit message\n";
        assert_eq!(
            with_prefix(file, "feat: ").as_deref(),
            Some("feat: \n# Please enter the commit message\n")
        );
        assert_eq!(with_prefix("", "docs: ").as_deref(), Some("docs:"));
        assert_eq!(with_prefix("fix: mine\n", "feat: "), None);
    }
}
