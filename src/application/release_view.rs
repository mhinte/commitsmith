//! Tab 3: the log's own state and the release toolbox beside it.

use super::*;

#[derive(Default)]
pub struct Release {
    /// Log state of tab 3.
    pub log: LogState,
    /// What the next release would be, from the last tag to `HEAD`.
    pub plan: Option<commitsmith::domain::release::ReleasePlan>,
    /// True while the write waits for a second keypress.
    pub confirm: bool,
    /// Where `CHANGELOG.md` lives; `None` outside a repository.
    pub changelog_root: Option<std::path::PathBuf>,
}
