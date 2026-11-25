//! What the setup wizard collects, and the rules each answer has to pass.
//!
//! The answers are a value object: once built, every field is already valid, so
//! the writer that stores them and the surface that asks for them cannot disagree
//! about what a legal prefix or template looks like.

use crate::domain::author::{AuthorSlug, AuthorSlugError};
use crate::domain::branch::{BranchError, BranchTemplate};
use crate::domain::ticket::TicketId;

/// How a commit points at its ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reference {
    Refs,
    Closes,
}

impl Reference {
    pub const ALL: [Self; 2] = [Self::Refs, Self::Closes];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Refs => "Refs",
            Self::Closes => "Closes",
        }
    }

    pub fn parse(text: &str) -> Result<Self, SetupError> {
        match text.trim().to_lowercase().as_str() {
            "refs" | "ref" | "references" => Ok(Self::Refs),
            "closes" | "close" | "fixes" | "fix" => Ok(Self::Closes),
            other => Err(SetupError::Reference(other.to_string())),
        }
    }

    /// The footer line a composer may prefill, e.g. `Refs TICKET-1234`.
    pub fn footer_for(self, ticket: &TicketId) -> String {
        format!("{} {}", self.as_str(), ticket)
    }
}

impl std::fmt::Display for Reference {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(self.as_str())
    }
}

/// Why an answer was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    Author(AuthorSlugError),
    Template(BranchError),
    Prefix(String),
    Reference(String),
    Profile(String),
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Author(problem) => write!(out, "{problem}"),
            Self::Template(problem) => write!(out, "{problem}"),
            Self::Prefix(prefix) => write!(
                out,
                "`{prefix}` is not a ticket prefix: 2-16 letters, digits or underscores, starting with a letter"
            ),
            Self::Reference(text) => {
                write!(out, "`{text}` is not a reference keyword: Refs or Closes")
            }
            Self::Profile(name) => write!(out, "`{name}` is not a known profile"),
        }
    }
}

impl std::error::Error for SetupError {}

impl From<AuthorSlugError> for SetupError {
    fn from(error: AuthorSlugError) -> Self {
        Self::Author(error)
    }
}

impl From<BranchError> for SetupError {
    fn from(error: BranchError) -> Self {
        Self::Template(error)
    }
}

/// Everything the wizard asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupAnswers {
    /// Short author handle used in branch names, e.g. `mmu`.
    pub author: AuthorSlug,
    /// Prefix for ticket ids, e.g. `TICKET`.
    pub ticket_prefix: String,
    pub reference: Reference,
    pub branch_template: BranchTemplate,
    /// Name of the commit profile to start with.
    pub profile: String,
    /// Show emojis in the type picker.
    pub emoji: bool,
    /// Add `Co-authored-by:` to the commit footer.
    pub co_author: bool,
    /// Offer to install the `git smith` alias.
    pub git_alias: bool,
}

impl SetupAnswers {
    /// Values that need no further questions, used by `init --defaults` and as
    /// the starting point of the dialog.
    pub fn defaults(profiles: &[String]) -> Self {
        Self {
            author: AuthorSlug::parse("mmu").unwrap_or_else(|_| unreachable!("built in")),
            ticket_prefix: "TICKET".to_string(),
            reference: Reference::Refs,
            branch_template: BranchTemplate::standard(),
            // `profiles` comes from a sorted map, so "conventional" has to be
            // asked for by name rather than taken from the front.
            profile: if profiles.iter().any(|name| name == "conventional") {
                "conventional".to_string()
            } else {
                profiles
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "conventional".to_string())
            },
            emoji: true,
            co_author: true,
            git_alias: false,
        }
    }

    /// Validate a raw author string and keep the rest.
    pub fn with_author(mut self, author: &str) -> Result<Self, SetupError> {
        self.author = AuthorSlug::parse(author)?;
        Ok(self)
    }

    pub fn with_ticket_prefix(mut self, prefix: &str) -> Result<Self, SetupError> {
        let cleaned = prefix.trim().to_uppercase();
        if !crate::domain::ticket::valid_prefix(&cleaned) {
            return Err(SetupError::Prefix(cleaned));
        }
        self.ticket_prefix = cleaned;
        Ok(self)
    }

    pub fn with_reference(mut self, text: &str) -> Result<Self, SetupError> {
        self.reference = Reference::parse(text)?;
        Ok(self)
    }

    pub fn with_template(mut self, template: &str) -> Result<Self, SetupError> {
        self.branch_template = BranchTemplate::parse(template)?;
        Ok(self)
    }

    pub fn with_profile(mut self, profile: &str, known: &[String]) -> Result<Self, SetupError> {
        if !known.iter().any(|candidate| candidate == profile) {
            return Err(SetupError::Profile(profile.to_string()));
        }
        self.profile = profile.to_string();
        Ok(self)
    }

    /// The template without the ticket segment, for work that has no ticket.
    pub fn short_template(&self) -> BranchTemplate {
        BranchTemplate::without_ticket()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Vec<String> {
        vec!["conventional".into(), "angular".into(), "atomic".into()]
    }

    #[test]
    fn defaults_are_valid_and_usable() {
        let answers = SetupAnswers::defaults(&profiles());
        assert_eq!(answers.author.as_str(), "mmu");
        assert_eq!(answers.ticket_prefix, "TICKET");
        assert_eq!(answers.reference, Reference::Refs);
        assert_eq!(
            answers.branch_template.as_str(),
            "{type}/{author}/{ticket}/{slug}"
        );
        assert_eq!(answers.profile, "conventional");
    }

    #[test]
    fn the_author_has_to_pass_its_own_rule() {
        let answers = SetupAnswers::defaults(&profiles());
        assert!(answers.clone().with_author("MH").is_err());
        assert_eq!(answers.with_author("mhi").unwrap().author.as_str(), "mhi");
    }

    #[test]
    fn the_ticket_prefix_is_normalised_and_checked() {
        let answers = SetupAnswers::defaults(&profiles());
        assert_eq!(
            answers
                .clone()
                .with_ticket_prefix("ticket")
                .unwrap()
                .ticket_prefix,
            "TICKET"
        );
        assert!(answers.clone().with_ticket_prefix("1TICKET").is_err());
        assert!(answers.with_ticket_prefix("TI CKET").is_err());
    }

    #[test]
    fn the_reference_keyword_accepts_common_spellings() {
        for text in ["Refs", "ref", "references"] {
            assert_eq!(Reference::parse(text).unwrap(), Reference::Refs);
        }
        for text in ["Closes", "close", "fixes"] {
            assert_eq!(Reference::parse(text).unwrap(), Reference::Closes);
        }
        assert!(Reference::parse("mentions").is_err());
    }

    #[test]
    fn the_footer_line_is_built_from_the_keyword() {
        let ticket = TicketId::parse("1234", "TICKET").unwrap();
        assert_eq!(Reference::Refs.footer_for(&ticket), "Refs TICKET-1234");
        assert_eq!(Reference::Closes.footer_for(&ticket), "Closes TICKET-1234");
    }

    #[test]
    fn templates_and_profiles_are_validated() {
        let answers = SetupAnswers::defaults(&profiles());
        assert!(
            answers
                .clone()
                .with_template("{type}/{owner}/{slug}")
                .is_err()
        );
        assert!(
            answers
                .clone()
                .with_profile("unknown", &profiles())
                .is_err()
        );
        assert_eq!(
            answers
                .with_profile("angular", &profiles())
                .unwrap()
                .profile,
            "angular"
        );
    }

    #[test]
    fn the_short_template_has_no_ticket_segment() {
        let answers = SetupAnswers::defaults(&profiles());
        assert_eq!(answers.short_template().as_str(), "{type}/{author}/{slug}");
    }
}
