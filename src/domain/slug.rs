//! Branch slugs: the kebab-case tail of a branch name, e.g. `neue-funktion-xyz`.
//!
//! The slug is derived from free text a person typed, so the rules live here once:
//! transliterate the umlauts, reduce the rest to ASCII, collapse separators and
//! cap the length. A slug is compared and hashed as the string it displays, which
//! is why the constructor is the only way to build one.

use std::fmt;

/// Longest slug we produce.
///
/// A branch name is typed by a person and shown in a narrow terminal column, so
/// the slug is cut to 50 characters rather than growing with the commit subject.
pub const MAX_SLUG_LEN: usize = 50;

/// A kebab-case slug: lower-case ASCII letters and digits, separated by single
/// dashes, never starting or ending with one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BranchSlug(String);

/// Why a slug could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlugError {
    /// Nothing survived normalisation, e.g. the input was only punctuation.
    Empty,
}

impl fmt::Display for SlugError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(out, "a branch slug needs at least one letter or digit"),
        }
    }
}

impl std::error::Error for SlugError {}

impl BranchSlug {
    /// Build a slug from free text: `"Größe & Maß"` becomes `groesse-mass`.
    ///
    /// Fails with [`SlugError::Empty`] when the text contains nothing usable;
    /// every other input yields a slug, so callers never have to handle a panic.
    pub fn from_text(input: &str) -> Result<Self, SlugError> {
        let mut slug = String::with_capacity(input.len());
        for ch in input.chars() {
            // Lower-casing first also covers `Ä`, `Ö`, `Ü` and `ẞ`, so the table
            // below only needs the lower-case spellings.
            for lower in ch.to_lowercase() {
                match lower {
                    // Not diacritics, but they belong to the same family of
                    // names a commit subject may carry.
                    'ä' | 'æ' => push_letters(&mut slug, "ae"),
                    'ö' | 'œ' => push_letters(&mut slug, "oe"),
                    'ü' => push_letters(&mut slug, "ue"),
                    'ß' => push_letters(&mut slug, "ss"),
                    'þ' => push_letters(&mut slug, "th"),
                    'ð' | 'đ' => push_letters(&mut slug, "d"),
                    'ł' => push_letters(&mut slug, "l"),
                    'ı' => push_letters(&mut slug, "i"),
                    ascii if ascii.is_ascii_alphanumeric() => slug.push(ascii),
                    other => match base_letter(other) {
                        Some(base) => slug.push(base),
                        None => push_separator(&mut slug),
                    },
                }
            }
        }

        // A hard cut can land on a separator, so the result is trimmed again:
        // a slug must never end on a dash.
        let slug: String = slug.chars().take(MAX_SLUG_LEN).collect();
        let slug = slug.trim_end_matches('-');
        if slug.is_empty() {
            return Err(SlugError::Empty);
        }
        Ok(Self(slug.to_string()))
    }

    /// The slug itself, e.g. `neue-funktion-xyz`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Append a transliteration, e.g. `ae` for `ä`.
fn push_letters(slug: &mut String, letters: &str) {
    slug.push_str(letters);
}

/// Append a separator unless the slug is still empty or already ends with one.
///
/// Doing both checks here leaves the result free of leading and repeated
/// separators, so a second pass over the string is not needed.
fn push_separator(slug: &mut String) {
    if !slug.is_empty() && !slug.ends_with('-') {
        slug.push('-');
    }
}

/// Reduce a letter that carries a diacritic to the ASCII letter underneath.
///
/// `std` has no Unicode normalisation, and pulling a crate in for this table
/// would cost more than it saves: the letters below are the ones that show up in
/// names, and anything else becomes a separator.
fn base_letter(c: char) -> Option<char> {
    let base = match c {
        'à' | 'á' | 'â' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => 'c',
        'ď' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù' | 'ú' | 'û' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        _ => return None,
    };
    Some(base)
}

impl fmt::Display for BranchSlug {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unwraps for the tests that feed valid input; a mistake should name the input.
    fn slug(input: &str) -> String {
        BranchSlug::from_text(input)
            .unwrap_or_else(|error| panic!("{input:?}: {error}"))
            .to_string()
    }

    #[test]
    fn turns_sentences_into_kebab_case() {
        assert_eq!(slug("Neue Funktion XYZ!"), "neue-funktion-xyz");
        assert_eq!(
            slug("Fix: der doppelte Hinweis"),
            "fix-der-doppelte-hinweis"
        );
        assert_eq!(slug("Tests für den Parser"), "tests-fuer-den-parser");
    }

    #[test]
    fn transliterates_the_german_umlauts() {
        assert_eq!(slug("Größe & Maß"), "groesse-mass");
        assert_eq!(slug("Änderung der Übersicht"), "aenderung-der-uebersicht");
        assert_eq!(slug("Ölstand prüfen"), "oelstand-pruefen");
        assert_eq!(slug("Straße"), "strasse");
    }

    #[test]
    fn reduces_other_diacritics_to_their_base_letter() {
        assert_eq!(slug("Crème brûlée à la carte"), "creme-brulee-a-la-carte");
        assert_eq!(slug("Señor Çedilla"), "senor-cedilla");
        assert_eq!(slug("Łódź"), "lodz");
        assert_eq!(slug("Peña Núñez"), "pena-nunez");
    }

    #[test]
    fn collapses_separator_runs() {
        assert_eq!(slug("a --  b"), "a-b");
        assert_eq!(slug("---a---b---"), "a-b");
        assert_eq!(slug("  leading and trailing  "), "leading-and-trailing");
    }

    #[test]
    fn keeps_digits() {
        assert_eq!(slug("Ticket 1234 beheben"), "ticket-1234-beheben");
        assert_eq!(slug("v2.1.0"), "v2-1-0");
    }

    #[test]
    fn cuts_the_length_and_never_ends_on_a_dash() {
        let long = BranchSlug::from_text(&"a".repeat(MAX_SLUG_LEN * 2)).unwrap();
        assert_eq!(long.as_str().len(), MAX_SLUG_LEN);
        assert!(!long.as_str().ends_with('-'));

        // Every fifth character is a separator, so the cut lands on one.
        let words = BranchSlug::from_text(&"wort ".repeat(40)).unwrap();
        assert!(words.as_str().len() <= MAX_SLUG_LEN);
        assert!(!words.as_str().ends_with('-'), "{}", words.as_str());
    }

    #[test]
    fn an_empty_result_is_an_error_and_not_a_panic() {
        for input in ["", "   ", "!!!", "---", "&%$§", "😀"] {
            assert_eq!(
                BranchSlug::from_text(input),
                Err(SlugError::Empty),
                "input was {input:?}"
            );
        }
    }

    #[test]
    fn display_prints_the_slug() {
        let slug = BranchSlug::from_text("Neue Funktion XYZ!").unwrap();
        assert_eq!(format!("{slug}"), "neue-funktion-xyz");
        assert_eq!(slug.as_str(), "neue-funktion-xyz");
        assert_eq!(format!("feat/{slug}"), "feat/neue-funktion-xyz");
    }

    #[test]
    fn equal_text_compares_and_hashes_the_same() {
        let a = BranchSlug::from_text("Größe & Maß").unwrap();
        let b = BranchSlug::from_text("  groesse  und  mass  ").unwrap_or_else(|error| {
            panic!("{error}");
        });
        assert_ne!(a, b, "different words stay different slugs");

        let same = BranchSlug::from_text("GROESSE--MASS").unwrap();
        let mut set = std::collections::HashSet::new();
        set.insert(a.clone());
        set.insert(same.clone());
        assert_eq!(set.len(), 1, "same slug must hash the same");
        assert_eq!(a, same);
    }

    #[test]
    fn the_error_reads_as_a_sentence() {
        let message = SlugError::Empty.to_string();
        assert!(message.contains("letter or digit"), "{message}");
    }
}
