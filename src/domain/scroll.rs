//! Row maths for a pane that wraps: how many rows a line occupies and where the
//! thumb of a scrollbar sits.
//!
//! Pure on purpose. The diff pane asked this of every line of the diff on every
//! frame, which is why the answer lives here where it can be tested without a
//! terminal.

use unicode_width::UnicodeWidthStr;

/// How many rows `line` needs at `width` columns, at least one.
///
/// This follows the way the pane actually wraps: at spaces, and a word that is
/// wider than a whole row is broken. Counting `cells / width` was wrong for real
/// diffs — a line of `+ some words here` needs more rows than its cell count
/// divided by the width, because the wrap leaves the rest of the row empty — and
/// the scrollbar therefore reached the bottom of the track while lines were still
/// missing below. That was Marc's finding on 26.11.
pub fn line_rows(line: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut rows = 1;
    let mut used = 0;
    for part in line.split_inclusive(' ') {
        let cells = UnicodeWidthStr::width(part);
        if used + cells <= width {
            used += cells;
            continue;
        }
        if used > 0 {
            rows += 1;
        }
        // The part alone is wider than one row: break it hard.
        let mut rest = cells;
        while rest > width {
            rows += 1;
            rest -= width;
        }
        used = rest;
    }
    rows
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
        // One word, wider than the row: broken where the row ends.
        assert_eq!(line_rows("abcdefghijk", 10), 2);
        assert_eq!(line_rows("abcdefghijklmnopqrst", 10), 2);
        assert_eq!(line_rows("x", 0), 1, "a zero-width pane still draws a row");
    }

    #[test]
    fn wrapped_text_costs_a_row_per_wrap_not_per_width() {
        // Three six-cell words at ten columns: cell counting says two rows
        // (18 / 10), the wrap needs three because no two words fit on one row.
        assert_eq!(line_rows("abcde fghij klmno", 10), 3);
        assert_eq!(line_rows("abcde fghij klmno", 12), 2, "two words fit");
        assert_eq!(line_rows("abcde fghij klmno", 6), 3, "one word per row");
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
