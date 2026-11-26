//! What the user is typing: the commit, where the cursor sits, what is folded.
//!
//! Split out of `App`, which had grown into one struct for the form, the
//! repository, the views and the commands at once. The rules that only need the
//! form live next to it.

use super::*;

#[derive(Debug, Default, Clone)]
pub struct Form {
    /// The commit being written.
    pub commit: Commit,
    /// Index into `FIELDS`.
    pub field: usize,
    /// Cursor position per field, counted in characters, not bytes.
    pub(crate) cursors: [usize; FIELDS.len()],
    /// True once the user chose a type; the branch must not override it.
    pub(crate) type_touched: bool,
    /// What `seed_footer` last wrote, so a later suggestion may replace its own
    /// text but never what the user typed.
    pub(crate) footer_seed: Option<String>,
    /// Body, breaking and footer folded away.
    pub compact: bool,
}

impl Form {
    /// A form that starts with the given type.
    pub fn with_kind(kind: String) -> Self {
        Self {
            commit: Commit {
                kind,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
