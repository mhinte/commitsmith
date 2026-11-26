//! Tab 3: the log's own state and the release toolbox beside it.

use super::*;

/// Which commits the plan counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Basis {
    /// Everything since the last version tag — the usual answer.
    #[default]
    Tag,
    /// The last N commits, for a repository that has no tags at all.
    Recent(usize),
}

impl Basis {
    /// `b` walks through these.
    pub fn next(self) -> Self {
        match self {
            Self::Tag => Self::Recent(20),
            Self::Recent(20) => Self::Recent(50),
            Self::Recent(_) => Self::Tag,
        }
    }

    pub fn label(self) -> Option<String> {
        match self {
            Self::Tag => None,
            Self::Recent(count) => Some(format!("last {count} commits")),
        }
    }
}

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
    /// Which commits the plan counts.
    pub basis: Basis,
}
