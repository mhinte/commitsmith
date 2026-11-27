//! What a new commit message starts with, read from the branch name.
//!
//! The rule is deliberately shy: only a branch that names both a kind and a
//! ticket produces a prefix, and only an empty message gets one. A branch called
//! `main` yields nothing at all — a wrong guess costs more keystrokes than it
//! saves.

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

/// The one-line start of a message, e.g. `feat(TICKET-12): `.
///
/// `None` when the branch says nothing usable — no kind, or no name at all.
pub fn prefix(branch: &str) -> Option<String> {
    let name = branch.trim();
    if name.is_empty() {
        return None;
    }
    // The template knows best: the first segment is the kind, a ticket sits
    // where the branch says it sits.
    if let Ok(parsed) = ConventionalBranchName::parse(name) {
        return Some(join(
            parsed.kind().commit_type(),
            parsed.ticket().map(ToString::to_string),
        ));
    }
    // A renamed or foreign branch still starts with its kind often enough, and
    // a ticket-shaped word anywhere in the name is worth taking.
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
        })?;
    Some(join(kind.commit_type(), ticket::in_text(name)))
}

fn join(kind: &str, ticket: Option<String>) -> String {
    match ticket {
        Some(ticket) => format!("{kind}({ticket}): "),
        None => format!("{kind}: "),
    }
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
    use super::{is_free, prefix, with_prefix};

    #[test]
    fn a_conventional_branch_names_type_and_ticket() {
        assert_eq!(
            prefix("feat/mmu/TICKET-12/add-a-thing").as_deref(),
            Some("feat(TICKET-12): ")
        );
        assert_eq!(prefix("fix/hima/PROJ-7").as_deref(), Some("fix(PROJ-7): "));
    }

    #[test]
    fn a_hotfix_branch_becomes_a_fix() {
        assert_eq!(
            prefix("hotfix/mmu/OPS-99/urgent").as_deref(),
            Some("fix(OPS-99): ")
        );
    }

    #[test]
    fn a_branch_without_a_ticket_gets_the_type_only() {
        assert_eq!(prefix("docs/mmu/readme").as_deref(), Some("docs: "));
        assert_eq!(prefix("feat/TICKET-3").as_deref(), Some("feat(TICKET-3): "));
        assert_eq!(prefix("feature/ABC-9").as_deref(), Some("feat(ABC-9): "));
    }

    #[test]
    fn a_main_branch_gets_no_suggestion_at_all() {
        assert_eq!(prefix("main"), None);
        assert_eq!(prefix("master"), None);
        assert_eq!(prefix("origin/main"), None);
        assert_eq!(prefix(""), None);
        assert_eq!(prefix("wip"), None);
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
            with_prefix(file, "feat(TICKET-12): ").as_deref(),
            Some("feat(TICKET-12): \n# Please enter the commit message\n")
        );
        assert_eq!(with_prefix("", "docs: ").as_deref(), Some("docs:"));
        assert_eq!(with_prefix("fix: mine\n", "feat: "), None);
    }
}
