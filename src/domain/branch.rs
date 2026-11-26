//! Conventional branch names: `feat/mmu/TICKET-1234/neue-funktion-xyz`.
//!
//! A branch name is an aggregate of four value objects — kind, author, ticket and
//! slug — plus the template that decides how they are written down. Both
//! directions live here so a name can be built and read back with the same rules.

use std::fmt;

use crate::domain::author::{AuthorSlug, AuthorSlugError};
use crate::domain::slug::{BranchSlug, SlugError};
use crate::domain::ticket::{TicketId, TicketIdError};

/// The branch kinds the wizard offers. Deliberately shorter than the commit
/// types: a branch is either work in progress, a fix, or a release errand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BranchKind {
    Feat,
    Fix,
    Hotfix,
    Release,
    Chore,
    Docs,
}

impl BranchKind {
    pub const ALL: [Self; 6] = [
        Self::Feat,
        Self::Fix,
        Self::Hotfix,
        Self::Release,
        Self::Chore,
        Self::Docs,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feat => "feat",
            Self::Fix => "fix",
            Self::Hotfix => "hotfix",
            Self::Release => "release",
            Self::Chore => "chore",
            Self::Docs => "docs",
        }
    }

    /// What this kind of branch is for, one line. Shown in the wizard so one does
    /// not have to remember what `hotfix` means as opposed to `fix`.
    pub fn description(self) -> &'static str {
        match self {
            Self::Feat => "new functionality",
            Self::Fix => "a bug fix on develop",
            Self::Hotfix => "an urgent fix on a release",
            Self::Release => "prepare a version",
            Self::Chore => "housekeeping",
            Self::Docs => "documentation only",
        }
    }

    /// The commit type this branch maps to, for the context bridge into the
    /// composer. A hotfix is still a fix, and a release branch carries chores.
    pub fn commit_type(self) -> &'static str {
        match self {
            Self::Feat => "feat",
            Self::Fix | Self::Hotfix => "fix",
            Self::Docs => "docs",
            Self::Release | Self::Chore => "chore",
        }
    }
}

impl fmt::Display for BranchKind {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.as_str())
    }
}

/// Why a branch name or template was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchError {
    /// The kind is not one of [`BranchKind::ALL`].
    UnknownKind(String),
    /// Too few or too many `/`-separated parts.
    Shape { expected: String, found: usize },
    /// The template uses a placeholder nobody can fill, or omits `{slug}`.
    Template(String),
    /// `{ticket}` is in the template but no ticket was given.
    MissingTicket,
    /// A part failed its own rule.
    Part(&'static str, String),
}

impl fmt::Display for BranchError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKind(kind) => write!(
                out,
                "`{kind}` is not a branch kind ({})",
                BranchKind::ALL
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Shape { expected, found } => {
                write!(out, "expected {expected}, got {found} parts")
            }
            Self::Template(problem) => write!(out, "branch template: {problem}"),
            Self::MissingTicket => write!(
                out,
                "the template contains {{ticket}} but no ticket was given"
            ),
            Self::Part(part, problem) => write!(out, "{part}: {problem}"),
        }
    }
}

impl std::error::Error for BranchError {}

impl From<AuthorSlugError> for BranchError {
    fn from(error: AuthorSlugError) -> Self {
        Self::Part("author", error.to_string())
    }
}

impl From<TicketIdError> for BranchError {
    fn from(error: TicketIdError) -> Self {
        Self::Part("ticket", error.to_string())
    }
}

impl From<SlugError> for BranchError {
    fn from(error: SlugError) -> Self {
        Self::Part("slug", error.to_string())
    }
}

/// How the four parts are written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTemplate {
    raw: String,
}

impl BranchTemplate {
    /// `{type}/{author}/{ticket}/{slug}`
    pub fn standard() -> Self {
        Self {
            raw: "{type}/{author}/{ticket}/{slug}".to_string(),
        }
    }

    /// `{type}/{author}/{slug}`, for work without a ticket.
    pub fn without_ticket() -> Self {
        Self {
            raw: "{type}/{author}/{slug}".to_string(),
        }
    }

    pub fn parse(raw: &str) -> Result<Self, BranchError> {
        let template = raw.trim();
        if template.is_empty() {
            return Err(BranchError::Template("must not be empty".into()));
        }
        let known = ["type", "author", "ticket", "slug"];
        let mut seen = Vec::new();
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}') else {
                return Err(BranchError::Template("unclosed placeholder".into()));
            };
            let name = &rest[start + 1..start + end];
            if !known.contains(&name) {
                return Err(BranchError::Template(format!(
                    "`{{{name}}}` is not a placeholder, use {}",
                    known
                        .iter()
                        .map(|k| format!("{{{k}}}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            if seen.contains(&name) {
                return Err(BranchError::Template(format!("{{{name}}} appears twice")));
            }
            seen.push(name);
            rest = &rest[start + end + 1..];
        }
        for required in ["type", "slug"] {
            if !seen.contains(&required) {
                return Err(BranchError::Template(format!("{{{required}}} is required")));
            }
        }
        Ok(Self {
            raw: template.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// True when the template writes the ticket into the name.
    pub fn uses_ticket(&self) -> bool {
        self.raw.contains("{ticket}")
    }
}

impl Default for BranchTemplate {
    fn default() -> Self {
        Self::standard()
    }
}

impl fmt::Display for BranchTemplate {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&self.raw)
    }
}

/// A branch name decomposed into what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionalBranchName {
    kind: BranchKind,
    author: AuthorSlug,
    ticket: Option<TicketId>,
    slug: BranchSlug,
}

impl ConventionalBranchName {
    pub fn new(
        kind: BranchKind,
        author: AuthorSlug,
        ticket: Option<TicketId>,
        slug: BranchSlug,
    ) -> Self {
        Self {
            kind,
            author,
            ticket,
            slug,
        }
    }

    /// Build from typed text: the description is slugified, the rest validated.
    pub fn from_parts(
        kind: BranchKind,
        author: &str,
        ticket: Option<(&str, &str)>,
        description: &str,
    ) -> Result<Self, BranchError> {
        let author = AuthorSlug::parse(author)?;
        let ticket = match ticket {
            Some((value, prefix)) => Some(TicketId::parse(value, prefix)?),
            None => None,
        };
        let slug = BranchSlug::from_text(description)?;
        Ok(Self::new(kind, author, ticket, slug))
    }

    pub fn kind(&self) -> BranchKind {
        self.kind
    }

    pub fn author(&self) -> &AuthorSlug {
        &self.author
    }

    pub fn ticket(&self) -> Option<&TicketId> {
        self.ticket.as_ref()
    }

    pub fn slug(&self) -> &BranchSlug {
        &self.slug
    }

    /// Write the name using `template`.
    pub fn render(&self, template: &BranchTemplate) -> Result<String, BranchError> {
        if template.uses_ticket() && self.ticket.is_none() {
            return Err(BranchError::MissingTicket);
        }
        let mut name = template.as_str().to_string();
        name = name.replace("{type}", self.kind.as_str());
        name = name.replace("{author}", self.author.as_str());
        name = name.replace("{slug}", self.slug.as_str());
        if let Some(ticket) = &self.ticket {
            name = name.replace("{ticket}", &ticket.to_string());
        }
        Ok(name)
    }

    /// Read a branch name back. Accepts the three-part and four-part form and
    /// ignores a leading `refs/heads/` or `origin/`.
    pub fn parse(name: &str) -> Result<Self, BranchError> {
        let cleaned = name
            .trim()
            .trim_start_matches("refs/heads/")
            .trim_start_matches("origin/")
            .trim_start_matches("remotes/origin/");
        let parts: Vec<&str> = cleaned.split('/').filter(|p| !p.is_empty()).collect();

        let (kind, author, ticket, slug_text) = match parts.as_slice() {
            [kind, author, slug] => (kind, author, None, slug),
            [kind, author, ticket, slug] => (kind, author, Some(*ticket), slug),
            _ => {
                return Err(BranchError::Shape {
                    expected: "type/author/[ticket/]slug".to_string(),
                    found: parts.len(),
                });
            }
        };

        let kind = BranchKind::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == kind.to_lowercase())
            .ok_or_else(|| BranchError::UnknownKind((*kind).to_string()))?;

        let author = AuthorSlug::parse(author)?;
        let ticket = match ticket {
            Some(value) => Some(TicketId::parse(value, "")?),
            None => None,
        };
        // The slug has to be exactly what `from_text` would produce, so a
        // hand-made `Foo_Bar` is rejected instead of silently normalised.
        let slug = BranchSlug::from_text(slug_text)?;
        if slug.as_str() != *slug_text {
            return Err(BranchError::Part(
                "slug",
                format!("`{slug_text}` is not kebab-case"),
            ));
        }

        Ok(Self::new(kind, author, ticket, slug))
    }
}

impl fmt::Display for ConventionalBranchName {
    /// The standard template, for the common case; use `render` for anything else.
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.render(&BranchTemplate::standard()) {
            Ok(name) => out.write_str(&name),
            Err(_) => write!(out, "{}/{}/{}", self.kind, self.author, self.slug),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(
        kind: BranchKind,
        ticket: Option<(&str, &str)>,
        description: &str,
    ) -> ConventionalBranchName {
        ConventionalBranchName::from_parts(kind, "mmu", ticket, description).unwrap()
    }

    #[test]
    fn renders_the_standard_template_with_a_ticket() {
        let branch = name(
            BranchKind::Feat,
            Some(("1234", "TICKET")),
            "New feature XYZ",
        );
        let rendered = branch.render(&BranchTemplate::standard()).unwrap();
        assert_eq!(rendered, "feat/mmu/TICKET-1234/new-feature-xyz");
    }

    #[test]
    fn renders_without_a_ticket() {
        let branch = name(BranchKind::Fix, None, "typo im Readme");
        let rendered = branch.render(&BranchTemplate::without_ticket()).unwrap();
        assert_eq!(rendered, "fix/mmu/typo-im-readme");
    }

    #[test]
    fn a_template_that_wants_a_ticket_refuses_a_branch_without_one() {
        let branch = name(BranchKind::Fix, None, "small fix");
        assert_eq!(
            branch.render(&BranchTemplate::standard()),
            Err(BranchError::MissingTicket)
        );
    }

    #[test]
    fn parse_is_the_inverse_of_render() {
        let branch = name(BranchKind::Hotfix, Some(("ABC-12", "")), "Größe & Maß");
        let rendered = branch.render(&BranchTemplate::standard()).unwrap();
        assert_eq!(rendered, "hotfix/mmu/ABC-12/groesse-mass");
        assert_eq!(ConventionalBranchName::parse(&rendered).unwrap(), branch);

        let plain = name(BranchKind::Chore, None, "deps");
        let rendered = plain.render(&BranchTemplate::without_ticket()).unwrap();
        assert_eq!(ConventionalBranchName::parse(&rendered).unwrap(), plain);
    }

    #[test]
    fn parse_accepts_a_remote_prefix() {
        let parsed = ConventionalBranchName::parse("origin/feat/mmu/TICKET-1/x").unwrap();
        assert_eq!(parsed.kind(), BranchKind::Feat);
        assert_eq!(parsed.ticket().unwrap().to_string(), "TICKET-1");
    }

    #[test]
    fn unknown_kinds_are_rejected() {
        assert!(matches!(
            ConventionalBranchName::parse("feature/mmu/x"),
            Err(BranchError::UnknownKind(_))
        ));
    }

    #[test]
    fn the_wrong_number_of_parts_is_rejected() {
        assert!(matches!(
            ConventionalBranchName::parse("main"),
            Err(BranchError::Shape { .. })
        ));
        assert!(matches!(
            ConventionalBranchName::parse("feat/mmu/a/b/c"),
            Err(BranchError::Shape { .. })
        ));
    }

    #[test]
    fn templates_are_validated() {
        assert!(BranchTemplate::parse("{type}/{author}/{slug}").is_ok());
        assert!(matches!(
            BranchTemplate::parse("{type}/{owner}/{slug}"),
            Err(BranchError::Template(_))
        ));
        assert!(matches!(
            BranchTemplate::parse("{type}/{author}"),
            Err(BranchError::Template(_))
        ));
        assert!(matches!(
            BranchTemplate::parse("{type}/{type}/{slug}"),
            Err(BranchError::Template(_))
        ));
        assert!(
            BranchTemplate::parse("{type}/{author}/{ticket}/{slug}")
                .unwrap()
                .uses_ticket()
        );
    }

    #[test]
    fn a_branch_maps_to_a_commit_type() {
        assert_eq!(BranchKind::Feat.commit_type(), "feat");
        assert_eq!(BranchKind::Hotfix.commit_type(), "fix");
        assert_eq!(BranchKind::Release.commit_type(), "chore");
        assert_eq!(BranchKind::Docs.commit_type(), "docs");
    }

    #[test]
    fn display_uses_the_standard_template() {
        let branch = name(BranchKind::Docs, Some(("7", "T")), "Readme");
        assert_eq!(branch.to_string(), "docs/mmu/T-7/readme");
    }
}

/// How a branch stands against its upstream, in the two cells the pane has.
///
/// `None` means there is no upstream to compare with. Shared by the footer and
/// the branch list, which used to format this differently in two places.
pub fn sync_label(sync: Option<(usize, usize)>) -> Option<String> {
    Some(match sync {
        Some((0, 0)) => "✓".to_string(),
        Some((ahead, 0)) => format!("↑{ahead}"),
        Some((0, behind)) => format!("↓{behind}"),
        Some((ahead, behind)) => format!("↑{ahead}↓{behind}"),
        None => "–".to_string(),
    })
}

#[cfg(test)]
mod sync_label_tests {
    use super::sync_label;

    #[test]
    fn the_short_forms_say_the_same_as_the_long_ones() {
        assert_eq!(sync_label(Some((0, 0))).as_deref(), Some("✓"));
        assert_eq!(sync_label(Some((2, 0))).as_deref(), Some("↑2"));
        assert_eq!(sync_label(Some((0, 3))).as_deref(), Some("↓3"));
        assert_eq!(sync_label(Some((2, 1))).as_deref(), Some("↑2↓1"));
        assert_eq!(sync_label(None).as_deref(), Some("–"));
    }
}
