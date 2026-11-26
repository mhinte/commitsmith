//! Row maths for a pane that wraps: how many rows a line occupies and where the
//! thumb of a scrollbar sits.
//!
//! Pure on purpose. The diff pane asked this of every line of the diff on every
//! frame, which is why the answer lives here where it can be tested without a
//! terminal.

use unicode_width::UnicodeWidthStr;

/// How many rows `line` needs at `width` columns, at least one.
///
/// Measured in cells, not chars: a line of emoji is wider than its `chars()`
/// count, and a tab counts as one cell here because the pane renders it as one.
pub fn line_rows(line: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    UnicodeWidthStr::width(line).div_ceil(width).max(1)
}

/// Where the thumb goes inside a track of `track` rows.
///
/// `total` is the number of rows the whole text occupies, `before` how many of
/// them lie above the first visible one, `visible` how many rows the pane shows.
/// `None` when everything fits and no bar is needed.
///
/// The thumb is sized and placed in *rows*, not lines: one long line occupies
/// several rows, and a line-based thumb drifts away from what is on screen.
pub fn thumb(total: usize, before: usize, visible: usize, track: u16) -> Option<(u16, u16)> {
    if track == 0 || total <= visible || visible == 0 {
        return None;
    }
    let span = (track as usize * visible / total).max(1) as u16;
    let room = total - visible;
    let start =
        (before.min(room) * track as usize / room).min(track.saturating_sub(span) as usize) as u16;
    Some((start, span))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_occupies_the_rows_it_is_wide() {
        assert_eq!(line_rows("", 10), 1);
        assert_eq!(line_rows("abc", 10), 1);
        assert_eq!(line_rows("abcdefghij", 10), 1, "exactly one row");
        assert_eq!(line_rows("abcdefghijk", 10), 2);
        assert_eq!(line_rows("abcdefghijklmnopqrst", 10), 2);
        assert_eq!(line_rows("x", 0), 1, "a zero-width pane still draws a row");
    }

    #[test]
    fn a_wide_character_counts_as_two_cells() {
        // 10 CJK characters are 20 cells wide, so they need two rows at 10.
        assert_eq!(line_rows("日本語日本語日本", 10), 2);
    }

    #[test]
    fn a_scrollbar_that_is_not_needed_is_not_drawn() {
        assert_eq!(thumb(10, 0, 10, 20), None, "everything fits");
        assert_eq!(thumb(10, 0, 0, 20), None, "nothing visible");
        assert_eq!(thumb(10, 0, 4, 0), None, "no track");
    }

    #[test]
    fn the_thumb_spans_and_moves_with_the_text() {
        // 100 rows of text in a 20-row track with 10 rows visible: a tenth of
        // the text is on screen, so the thumb is a tenth of the track.
        assert_eq!(thumb(100, 0, 10, 20), Some((0, 2)));
        // Halfway down: the thumb sits halfway along the remaining track.
        assert_eq!(thumb(100, 45, 10, 20), Some((10, 2)));
        // At the very bottom it ends flush with the track.
        assert_eq!(thumb(100, 90, 10, 20), Some((18, 2)));
    }

    #[test]
    fn the_thumb_stays_inside_the_track_when_it_is_a_single_row() {
        let (start, span) = thumb(1000, 999, 10, 20).expect("a bar");
        assert_eq!(span, 1, "always at least one row tall");
        assert!(start + span <= 20, "{start} + {span}");
    }

    #[test]
    fn an_offset_past_the_end_does_not_overflow() {
        // The cursor can sit beyond the last row after a reload; the bar must
        // clamp instead of wrapping around.
        assert_eq!(thumb(100, 500, 10, 20), thumb(100, 90, 10, 20));
    }
}
