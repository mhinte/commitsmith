//! Ticket ids such as `TICKET-1234`.
//!
//! The id is a value object: it is created through `parse`, which normalises the
//! spelling once so the rest of the program never has to compare case-insensitive
//! strings or wonder where the prefix ends.

use std::fmt;
use std::str::FromStr;

/// A normalised ticket reference: upper-case prefix, dash, upper-case number.
///
/// `Display` is the canonical spelling, `parse` accepts the sloppy ones a person
/// types (`ticket-1234`, `Ticket 1234`, `1234` with a default prefix).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TicketId {
    prefix: String,
    number: String,
}

/// Why a ticket id could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketIdError {
    /// Nothing but spaces was given.
    Empty,
    /// No prefix on the input and none configured.
    MissingPrefix,
    /// The prefix contains characters a ticket system would not use.
    BadPrefix(String),
    /// The part after the dash is empty or unusable.
    BadNumber(String),
}

impl fmt::Display for TicketIdError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(out, "a ticket id must not be empty"),
            Self::MissingPrefix => write!(
                out,
                "no ticket prefix given: write TICKET-1234 or set a default prefix"
            ),
            Self::BadPrefix(prefix) => write!(
                out,
                "`{prefix}` is not a ticket prefix: letters, digits and underscores only"
            ),
            Self::BadNumber(number) => write!(
                out,
                "`{number}` is not a ticket number: letters, digits and dashes only"
            ),
        }
    }
}

impl std::error::Error for TicketIdError {}

impl TicketId {
    /// Build from `input`, using `default_prefix` when the input has none.
    ///
    /// Accepted: `TICKET-1234`, `ticket-1234`, `Ticket 1234`, `TICKET#1234`,
    /// `#TICKET-1234` and a bare `1234` when a default prefix is configured.
    pub fn parse(input: &str, default_prefix: &str) -> Result<Self, TicketIdError> {
        let cleaned = input.trim().trim_start_matches('#').trim();
        if cleaned.is_empty() {
            return Err(TicketIdError::Empty);
        }

        // A dash separates prefix and number; a hash and spaces are accepted as
        // separators too because that is how tickets show up in commit messages.
        let (prefix, number) = match cleaned
            .split_once('-')
            .or_else(|| cleaned.split_once('#'))
            .or_else(|| {
                // `1234 abc` is not a prefix, so a leading number keeps the
                // whole input as the number and lets the prefix check fail.
                cleaned
                    .split_once(char::is_whitespace)
                    .filter(|(left, _)| !left.chars().all(|c| c.is_ascii_digit()))
            }) {
            Some((prefix, number)) => (prefix.trim().to_string(), number.trim().to_string()),
            None => (String::new(), cleaned.to_string()),
        };

        let prefix = if prefix.is_empty() {
            default_prefix.trim().to_string()
        } else {
            prefix
        };

        if prefix.is_empty() {
            return Err(TicketIdError::MissingPrefix);
        }
        if !valid_prefix(&prefix) {
            return Err(TicketIdError::BadPrefix(prefix));
        }

        let number = number.trim().trim_start_matches('-').trim().to_string();
        if number.is_empty() || !valid_number(&number) {
            return Err(TicketIdError::BadNumber(number));
        }

        Ok(Self {
            prefix: prefix.to_uppercase(),
            number: number.to_uppercase(),
        })
    }

    /// The project key, upper case, e.g. `TICKET`.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The number part, upper case, e.g. `1234`.
    pub fn number(&self) -> &str {
        &self.number
    }
}

/// Prefixes start with a letter so a bare number is never mistaken for one.
pub fn valid_prefix(prefix: &str) -> bool {
    let mut chars = prefix.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    prefix.len() <= 16 && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Numbers may carry a suffix such as `ABC12` and further dashes (`12-3`).
fn valid_number(number: &str) -> bool {
    number.len() <= 32
        && number
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !number.starts_with('-')
        && !number.ends_with('-')
}

impl fmt::Display for TicketId {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}-{}", self.prefix, self.number)
    }
}

impl FromStr for TicketId {
    type Err = TicketIdError;

    /// Bare parsing without a default prefix; `parse` carries the configured one.
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_case_and_separators() {
        for input in [
            "TICKET-1234",
            "ticket-1234",
            "Ticket-1234",
            "ticket 1234",
            "TICKET#1234",
            "#TICKET-1234",
            "  ticket-1234  ",
        ] {
            let id = TicketId::parse(input, "").unwrap_or_else(|error| panic!("{input}: {error}"));
            assert_eq!(id.to_string(), "TICKET-1234", "input was {input:?}");
        }
    }

    #[test]
    fn a_bare_number_uses_the_default_prefix() {
        let id = TicketId::parse("1234", "ticket").unwrap();
        assert_eq!(id.to_string(), "TICKET-1234");
        assert_eq!(id.prefix(), "TICKET");
        assert_eq!(id.number(), "1234");
    }

    #[test]
    fn a_bare_number_without_a_prefix_is_an_error() {
        assert_eq!(
            TicketId::parse("1234", ""),
            Err(TicketIdError::MissingPrefix)
        );
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(TicketId::parse("   ", "TICKET"), Err(TicketIdError::Empty));
    }

    #[test]
    fn a_prefix_needs_to_start_with_a_letter() {
        assert!(matches!(
            TicketId::parse("123-456", ""),
            Err(TicketIdError::BadPrefix(_)) | Err(TicketIdError::MissingPrefix)
        ));
        assert!(matches!(
            TicketId::parse("TICK ET-12", ""),
            Err(TicketIdError::BadPrefix(_))
        ));
    }

    #[test]
    fn the_number_must_not_be_empty() {
        assert!(matches!(
            TicketId::parse("TICKET-", ""),
            Err(TicketIdError::BadNumber(_))
        ));
    }

    #[test]
    fn suffix_numbers_are_allowed() {
        let id = TicketId::parse("ticket-abc12", "").unwrap();
        assert_eq!(id.to_string(), "TICKET-ABC12");
        let id = TicketId::parse("TICKET-12-3", "").unwrap();
        assert_eq!(id.to_string(), "TICKET-12-3");
    }

    #[test]
    fn equality_is_canonical() {
        let a = TicketId::parse("ticket-1234", "").unwrap();
        let b = TicketId::from_str("TICKET-1234").unwrap();
        assert_eq!(a, b);
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 1, "same ticket must hash the same");
    }

    #[test]
    fn errors_explain_themselves() {
        let message = TicketIdError::BadNumber("?".into()).to_string();
        assert!(message.contains("not a ticket number"), "{message}");
    }
}
