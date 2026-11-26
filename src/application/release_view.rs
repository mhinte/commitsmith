//! Tab 3: the log's own state and the release toolbox beside it.

use super::*;

/// Which commits the plan counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Basis {
    /// This branch's own work: everything since its merge base with the main
    /// branch. The default, because it needs no tags and answers the question
    /// "what would go into *this* release".
    #[default]
    Branch,
    /// Everything since the last version tag, for a repository that tags.
    Tag,
    /// The last N commits, for a repository that has no tags at all.
    Recent(usize),
}

impl Basis {
    /// `b` walks through these.
    pub fn next(self) -> Self {
        match self {
            Self::Tag => Self::Branch,
            Self::Branch => Self::Recent(20),
            Self::Recent(20) => Self::Recent(50),
            Self::Recent(_) => Self::Tag,
        }
    }

    /// A label for the cases that are not "since the tag"; the branch case
    /// fills in the base it found, so `label_for` takes it.
    pub fn label_for(self, base: Option<&str>) -> Option<String> {
        match self {
            Self::Tag => None,
            Self::Branch => Some(match base {
                Some(base) => format!("on this branch, not on {base}"),
                None => "on this branch".to_string(),
            }),
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
