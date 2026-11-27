//! Deciding whether an update is worth it, and which file to fetch.
//!
//! The rules live here so they can be tested without a network: what a version
//! string means, which release asset belongs to this machine, and when an update
//! must be refused. Anything that talks to GitHub belongs to the adapter.

use crate::domain::release::SemVer;

/// The three platforms this project ships binaries for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    LinuxGnu,
    LinuxMusl,
    Windows,
}

impl Platform {
    /// The running platform, as far as this program can tell.
    ///
    /// Linux always takes the static musl build: it is the one that runs on any
    /// distribution, and offering a choice would only add a way to pick wrong.
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "linux" => Some(Self::LinuxMusl),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }

    pub fn asset_name(self, version: &str) -> String {
        match self {
            Self::LinuxGnu => format!("commitsmith-{version}-x86_64-unknown-linux-gnu.tar.gz"),
            Self::LinuxMusl => format!("commitsmith-{version}-x86_64-unknown-linux-musl.tar.gz"),
            Self::Windows => format!("commitsmith-{version}-x86_64-pc-windows-msvc.zip"),
        }
    }

    /// Where the binary sits inside the unpacked archive.
    pub fn binary_in_archive(self, version: &str) -> Option<String> {
        match self {
            Self::LinuxGnu | Self::LinuxMusl => {
                let triple = match self {
                    Self::LinuxGnu => "x86_64-unknown-linux-gnu",
                    _ => "x86_64-unknown-linux-musl",
                };
                Some(format!("commitsmith-{version}-{triple}/commitsmith"))
            }
            // The zip holds the exe at the top level.
            Self::Windows => Some(format!(
                "commitsmith-{version}-x86_64-pc-windows-msvc/commitsmith.exe"
            )),
        }
    }
}

/// Our own version, parsed. A development build that cannot be parsed is treated
/// as "older than everything", so `update` still works on it.
pub fn current() -> Option<SemVer> {
    SemVer::parse(env!("CARGO_PKG_VERSION"))
}

/// What to do about a release tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    UpToDate {
        current: String,
    },
    Newer {
        latest: String,
    },
    /// The release tag is not a version this build understands.
    Unreadable(String),
}

/// Compare the release tag with the running version.
pub fn decide(current: Option<SemVer>, tag: &str) -> Decision {
    let Some(latest) = SemVer::parse(tag) else {
        return Decision::Unreadable(tag.to_string());
    };
    match current {
        Some(ours) if ours >= latest => Decision::UpToDate {
            current: format!("{}.{}.{}", ours.major, ours.minor, ours.patch),
        },
        _ => Decision::Newer {
            latest: format!("{}.{}.{}", latest.major, latest.minor, latest.patch),
        },
    }
}

/// The line for our file out of a `SHA256SUMS` file.
pub fn checksum_for<'a>(sums: &'a str, asset: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let sum = parts.next()?;
        let name = parts.next()?;
        // Some tools prefix the name with `*` for binary mode.
        let name = name.trim_start_matches('*');
        (name == asset && sum.chars().all(|c| c.is_ascii_hexdigit()) && sum.len() == 64)
            .then_some(sum)
    })
}

#[cfg(test)]
mod tests {
    use super::{Decision, Platform, checksum_for, decide};
    use crate::domain::release::SemVer;

    #[test]
    fn a_newer_tag_is_an_update() {
        assert_eq!(
            decide(SemVer::parse("0.26.0"), "v0.27.0"),
            Decision::Newer {
                latest: "0.27.0".into()
            }
        );
        assert_eq!(
            decide(SemVer::parse("0.26.0"), "v0.26.1"),
            Decision::Newer {
                latest: "0.26.1".into()
            }
        );
    }

    #[test]
    fn the_same_or_an_older_tag_is_not() {
        assert_eq!(
            decide(SemVer::parse("0.26.0"), "v0.26.0"),
            Decision::UpToDate {
                current: "0.26.0".into()
            }
        );
        assert_eq!(
            decide(SemVer::parse("0.26.0"), "v0.25.9"),
            Decision::UpToDate {
                current: "0.26.0".into()
            }
        );
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_named_not_guessed() {
        assert_eq!(
            decide(Some(SemVer::START), "nightly"),
            Decision::Unreadable("nightly".into())
        );
    }

    #[test]
    fn the_asset_names_match_what_the_release_publishes() {
        assert_eq!(
            Platform::LinuxMusl.asset_name("0.26.0"),
            "commitsmith-0.26.0-x86_64-unknown-linux-musl.tar.gz"
        );
        assert_eq!(
            Platform::Windows.asset_name("0.26.0"),
            "commitsmith-0.26.0-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            Platform::LinuxMusl.binary_in_archive("0.26.0").as_deref(),
            Some("commitsmith-0.26.0-x86_64-unknown-linux-musl/commitsmith")
        );
    }

    #[test]
    fn only_the_line_for_our_asset_counts() {
        // Real SHA256SUMS lines: 64 hex characters, two spaces, the file name.
        let gnu = "1".repeat(64);
        let musl = "2".repeat(64);
        let sums = format!(
            "{gnu}  commitsmith-0.26.0-x86_64-unknown-linux-gnu.tar.gz\n\
             {musl}  commitsmith-0.26.0-x86_64-unknown-linux-musl.tar.gz\n"
        );
        assert_eq!(
            checksum_for(&sums, "commitsmith-0.26.0-x86_64-unknown-linux-musl.tar.gz"),
            Some(musl.as_str())
        );
        assert_eq!(
            checksum_for(&sums, "commitsmith-0.26.0-x86_64-unknown-linux-gnu.tar.gz"),
            Some(gnu.as_str())
        );
        // A truncated or malformed line is not a checksum.
        assert_eq!(
            checksum_for(&sums, "commitsmith-0.26.0-x86_64-pc-windows-msvc.zip"),
            None
        );
        assert_eq!(checksum_for("abc  file.tar.gz\n", "file.tar.gz"), None);
    }
}
