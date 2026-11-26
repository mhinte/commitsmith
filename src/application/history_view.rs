//! The history the tool knows about: the commits, their file lists, the cache.
//!
//! Third step out of `App`. The commit picker on tab 1 and the log on tab 3 read
//! the same list, so it is one object, not a field of each pane.

use super::*;

#[derive(Default)]
pub struct History {
    /// Recent commits, offered by the picker and shown in the log.
    pub commits: Vec<PastCommit>,
    /// Reads a commit's file list when the preview needs it. `None` outside a repo.
    pub loader: Option<FilesLoader>,
    /// One entry per commit whose files have been read.
    pub cache: HashMap<String, Vec<String>>,
}
