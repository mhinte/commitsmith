//! The repository the tool is looking at: what is checked out and where it stands.
//!
//! Second step out of `App`. Everything here answers "which repository, which
//! branch, what does git say about it" — nothing about how it is drawn.

use super::*;

#[derive(Default)]
pub struct RepoState {
    /// The repository behind the actions of tab 2, when there is one.
    pub git: Option<Box<dyn GitRepository>>,
    /// Short name of the checked-out branch, for the context bridge.
    pub branch_name: Option<String>,
    /// Branch and staged file summary, as the header shows it.
    pub line: String,
    /// `(ahead, behind)` against the upstream; `None` when there is no upstream.
    pub sync: Option<(usize, usize)>,
    /// False when the working directory is not inside a repository.
    pub in_repository: bool,
}
