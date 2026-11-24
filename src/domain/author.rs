//! Author shortcuts for branch names, e.g. `mmu`.
//!
//! Deliberately strict: the slug ends up in a branch name and therefore in every
//! path a person types, so anything that needs quoting or looks like a path
//! separator is rejected at the door.

use std::fmt;
use std::str::FromStr;

/// A short, lower-case author handle, 2 to 10 characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuthorSlug(String);

/// Why an author slug was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorSlugError {
    /// Nothing but spaces.
    Empty,
    /// Shorter than the minimum or longer than the maximum.
    Length(usize),
    /// Upper case letters: branch names stay lower case.
    UpperCase,
    /// Anything outside `a-z0-9._-`.
    BadCharacter(char),
    /// A leading or trailing separator produces `feat//x` or `feat/-x`.
    BadEdge(char),
}

impl fmt::Display for AuthorSlugError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(out, "an author slug must not be empty"),
            Self::Length(len) => write!(out, "{len} characters: an author slug has 2 to 10"),
            Self::UpperCase => write!(out, "write the author slug in lower case"),
            Self::BadCharacter(c) => write!(
                out,
                "`{c}` is not allowed in an author slug: a-z, 0-9, dot, dash, underscore"
            ),
            Self::BadEdge(c) => write!(out, "an author slug must not start or end with `{c}`"),
        }
    }
}

impl std::error::Error for AuthorSlugError {}

impl AuthorSlug {
    pub const MIN: usize = 2;
    pub const MAX: usize = 10;

    pub fn parse(input: &str) -> Result<Self, AuthorSlugError> {
        let text = input.trim();
        if text.is_empty() {
            return Err(AuthorSlugError::Empty);
        }
        if text.chars().any(char::is_whitespace) {
            let offending = text.chars().find(|c| c.is_whitespace()).unwrap_or(' ');
            return Err(AuthorSlugError::BadCharacter(offending));
        }
        if text.chars().any(char::is_uppercase) {
            return Err(AuthorSlugError::UpperCase);
        }
        for c in text.chars() {
            if !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_')) {
                return Err(AuthorSlugError::BadCharacter(c));
            }
        }
        let len = text.chars().count();
        if !(Self::MIN..=Self::MAX).contains(&len) {
            return Err(AuthorSlugError::Length(len));
        }
        for edge in [text.chars().next(), text.chars().last()]
            .into_iter()
            .flatten()
        {
            if matches!(edge, '.' | '-' | '_') {
                return Err(AuthorSlugError::BadEdge(edge));
            }
        }
        Ok(Self(text.to_string()))
    }

    /// Suggest a slug from a full name: initials of the given names plus the
    /// start of the surname, e.g. `Marc Hinte` -> `mhi`.
    ///
    /// This is a convenience for the setup wizard; the result still has to pass
    /// `parse`, so an unusable name simply yields `None`.
    pub fn suggest(full_name: &str) -> Option<Self> {
        let parts: Vec<&str> = full_name.split_whitespace().collect();
        let letters =
            |part: &str| -> Vec<char> { part.chars().filter(char::is_ascii_alphabetic).collect() };
        let candidate: String = match parts.as_slice() {
            [] => return None,
            // A single name has no initials to build from.
            [only] => letters(only).into_iter().take(3).collect(),
            [given @ .., surname] => {
                let mut out: String = given
                    .iter()
                    .filter_map(|part| letters(part).first().copied())
                    .collect();
                out.extend(letters(surname).into_iter().take(2));
                out
            }
        };
        Self::parse(&candidate.to_lowercase()).ok()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AuthorSlug {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&self.0)
    }
}

/// Parsing without a fallback: a slug is either valid as written or rejected.
impl FromStr for AuthorSlug {
    type Err = AuthorSlugError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_short_handles() {
        for input in ["mmu", "mh", "m.h", "m-h", "m_h", "abc123", "abcdefghij"] {
            assert!(AuthorSlug::parse(input).is_ok(), "{input} should be valid");
        }
    }

    #[test]
    fn trims_surrounding_spaces_but_rejects_inner_ones() {
        assert_eq!(AuthorSlug::parse("  mmu ").unwrap().as_str(), "mmu");
        assert!(matches!(
            AuthorSlug::parse("m mu"),
            Err(AuthorSlugError::BadCharacter(' '))
        ));
    }

    #[test]
    fn rejects_upper_case() {
        assert_eq!(AuthorSlug::parse("MMU"), Err(AuthorSlugError::UpperCase));
    }

    #[test]
    fn enforces_the_length_window() {
        assert_eq!(AuthorSlug::parse("m"), Err(AuthorSlugError::Length(1)));
        assert_eq!(
            AuthorSlug::parse("abcdefghijk"),
            Err(AuthorSlugError::Length(11))
        );
    }

    #[test]
    fn rejects_path_like_characters() {
        assert!(matches!(
            AuthorSlug::parse("m/mu"),
            Err(AuthorSlugError::BadCharacter('/'))
        ));
        assert!(matches!(
            AuthorSlug::parse("m\\u"),
            Err(AuthorSlugError::BadCharacter('\\'))
        ));
        assert!(matches!(
            AuthorSlug::parse("mü"),
            Err(AuthorSlugError::BadCharacter('ü'))
        ));
    }

    #[test]
    fn rejects_leading_and_trailing_separators() {
        assert!(matches!(
            AuthorSlug::parse("-mmu"),
            Err(AuthorSlugError::BadEdge('-'))
        ));
        assert!(matches!(
            AuthorSlug::parse("mmu."),
            Err(AuthorSlugError::BadEdge('.'))
        ));
    }

    #[test]
    fn empty_is_reported_as_empty() {
        assert_eq!(AuthorSlug::parse("  "), Err(AuthorSlugError::Empty));
    }

    #[test]
    fn suggests_a_slug_from_a_name() {
        assert_eq!(AuthorSlug::suggest("Marc Hinte").unwrap().as_str(), "mhi");
        assert_eq!(AuthorSlug::suggest("Marc").unwrap().as_str(), "mar");
        assert!(AuthorSlug::suggest("").is_none());
    }

    #[test]
    fn errors_read_as_sentences() {
        assert!(
            AuthorSlugError::UpperCase
                .to_string()
                .contains("lower case")
        );
        assert!(
            AuthorSlugError::BadEdge('-')
                .to_string()
                .contains("start or end")
        );
    }
}
