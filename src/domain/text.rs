//! Cutting text to a column budget.
//!
//! Widths are terminal cells, not characters: an emoji takes two, a combining
//! mark none. Everything that shortens a label, a path or a status line asks
//! here, so the four near-identical helpers that used to exist behave the same.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Cut from the right, with an ellipsis when something was dropped.
pub fn fit(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cell = UnicodeWidthChar::width(c).unwrap_or(1);
        if used + cell + 1 > width {
            break;
        }
        used += cell;
        out.push(c);
        if used + 1 >= width {
            break;
        }
    }
    out.push('…');
    out
}

/// Cut from the left: the end of a path is the part that identifies it.
pub fn fit_left(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut keep: Vec<char> = Vec::new();
    let mut used = 0;
    for c in text.chars().rev() {
        let cell = UnicodeWidthChar::width(c).unwrap_or(1);
        if used + cell + 1 > width {
            break;
        }
        used += cell;
        keep.push(c);
    }
    let mut out = String::from("…");
    out.extend(keep.into_iter().rev());
    out
}

/// `"1 error"`, `"2 errors"` — the counts sit where the user reads them.
pub fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", plural_suffix(count))
}

/// Just the trailing `s`, for sentences that already carry the number.
pub fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_returned_unchanged() {
        assert_eq!(fit("abc", 10), "abc");
        assert_eq!(fit_left("abc", 10), "abc");
        assert_eq!(fit("abc", 3), "abc");
    }

    #[test]
    fn cutting_right_keeps_the_beginning() {
        assert_eq!(fit("abcdefghij", 5), "abcd…");
        assert_eq!(fit("abcdefghij", 1), "…");
    }

    #[test]
    fn cutting_left_keeps_the_end() {
        assert_eq!(fit_left("src/deep/file.rs", 8), "…file.rs");
    }

    #[test]
    fn a_wide_character_counts_two_cells() {
        // Each emoji is two cells wide; four cells hold one and the ellipsis.
        let text = "✨✨✨";
        let cut = fit(text, 4);
        assert!(UnicodeWidthStr::width(cut.as_str()) <= 4, "{cut}");
        assert_eq!(cut, "✨…");
    }

    #[test]
    fn plurals_agree_on_the_rule() {
        assert_eq!(plural(1, "error"), "1 error");
        assert_eq!(plural(2, "error"), "2 errors");
        assert_eq!(plural(0, "warning"), "0 warnings");
        assert_eq!(plural_suffix(1), "");
        assert_eq!(plural_suffix(5), "s");
    }

    #[test]
    fn umlauts_keep_their_width() {
        let cut = fit("ääääää", 4);
        assert_eq!(cut, "äää…");
    }
}
