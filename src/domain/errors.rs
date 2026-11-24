//! What the ports hand back when something goes wrong.
//!
//! A port never answers with a `String`: the application has to tell a missing
//! branch from a failed push without parsing prose, and a surface may want to
//! render the same failure differently. Each variant therefore names the thing
//! it is about — a reference, a path, an operation — and `Display` only turns
//! that into a sentence.
//!
//! No `From<std::io::Error>` is derived for the io variants. A blank conversion
//! would throw away the one piece of information that makes the error useful,
//! namely *where* the read or write happened; use [`GitError::io`] and
//! [`ConfigError::io`] and pass a context with it.

use std::fmt;
use std::io;

/// Which config file a lookup was about.
///
/// The user file is the one written on first start, the project file sits in the
/// repository and wins over the user file. The scope is part of the error so a
/// caller can say which of the two is missing without guessing from a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigScope {
    /// The user-wide file, e.g. `~/.config/commitsmith/config.toml`.
    User,
    /// The file in the repository root, e.g. `.commitsmith.toml`.
    Project,
}

impl ConfigScope {
    /// Lower-case name, as used in messages and log lines.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

impl fmt::Display for ConfigScope {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.as_str())
    }
}

/// Everything that can go wrong in the repository.
///
/// Variants carry context rather than a pre-formatted message: a reference, the
/// operation that failed, the paths that conflict. Callers that only want to
/// print the failure use `Display`, callers that have to react match on it.
#[derive(Debug)]
pub enum GitError {
    /// No such branch, tag, commit or remote.
    NotFound { reference: String },
    /// The repository cannot do this right now, e.g. a detached head, no
    /// commits yet, or a bare repository where a work tree is needed.
    InvalidState { context: String },
    /// A merge left these paths unresolved.
    Conflict { paths: Vec<String> },
    /// Talking to the remote failed; `operation` is `fetch`, `push`, `pull`, …
    Remote {
        operation: String,
        message: String,
    },
    /// Reading or writing inside the repository failed; `context` says where.
    Io {
        context: String,
        source: io::Error,
    },
}

impl GitError {
    /// Nothing in the repository is named `reference`.
    pub fn not_found(reference: impl Into<String>) -> Self {
        Self::NotFound {
            reference: reference.into(),
        }
    }

    /// The repository is not in a state that allows the requested operation.
    pub fn invalid_state(context: impl Into<String>) -> Self {
        Self::InvalidState {
            context: context.into(),
        }
    }

    /// These paths did not merge cleanly.
    pub fn conflict<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<String>,
    {
        Self::Conflict {
            paths: paths.into_iter().map(Into::into).collect(),
        }
    }

    /// A remote operation failed with `message`.
    pub fn remote(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Remote {
            operation: operation.into(),
            message: message.into(),
        }
    }

    /// An io failure in `context`, e.g. `"reading .git/index"`.
    ///
    /// This is the only way to build the variant, so the context can never go
    /// missing the way it would with a blanket `From<std::io::Error>`.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { reference } => {
                write!(out, "`{reference}` not found in this repository")
            }
            Self::InvalidState { context } => {
                write!(out, "the repository cannot do this right now: {context}")
            }
            Self::Conflict { paths } if paths.is_empty() => {
                write!(out, "the merge left unresolved conflicts")
            }
            Self::Conflict { paths } => {
                write!(out, "unresolved conflicts in {}", paths.join(", "))
            }
            Self::Remote { operation, message } => {
                write!(out, "`{operation}` on the remote failed: {message}")
            }
            Self::Io { context, source } => write!(out, "{context}: {source}"),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Everything that can go wrong while loading or saving configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// The file for this scope does not exist.
    NotFound { scope: ConfigScope },
    /// The file exists but does not parse; `context` is the location or key,
    /// `message` says what was expected.
    Parse {
        context: String,
        message: String,
    },
    /// Reading or writing the file failed; `context` says where.
    Io {
        context: String,
        source: io::Error,
    },
}

impl ConfigError {
    /// No config file for `scope`.
    pub fn not_found(scope: ConfigScope) -> Self {
        Self::NotFound { scope }
    }

    /// The file did not parse; `context` names the file or key.
    pub fn parse(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Parse {
            context: context.into(),
            message: message.into(),
        }
    }

    /// An io failure in `context`, e.g. `"reading ~/.config/commitsmith/config.toml"`.
    ///
    /// Kept as a constructor instead of a `From` impl for the same reason as
    /// [`GitError::io`]: the context is not optional.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { scope } => write!(out, "no {scope} config found"),
            Self::Parse { context, message } => write!(out, "cannot parse {context}: {message}"),
            Self::Io { context, source } => write!(out, "{context}: {source}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_missing_reference() {
        let error = GitError::NotFound {
            reference: "release/1.0".into(),
        };
        assert_eq!(
            error.to_string(),
            "`release/1.0` not found in this repository"
        );
    }

    #[test]
    fn display_lists_conflicting_paths() {
        let error = GitError::conflict(["src/app.rs", "src/ui.rs"]);
        assert_eq!(
            error.to_string(),
            "unresolved conflicts in src/app.rs, src/ui.rs"
        );
    }

    #[test]
    fn display_of_a_conflict_without_paths_still_reads() {
        let error = GitError::conflict(Vec::<String>::new());
        assert_eq!(error.to_string(), "the merge left unresolved conflicts");
    }

    #[test]
    fn io_variants_carry_context_and_hand_the_source_through() {
        let git = GitError::io(
            "reading .git/index",
            io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
        );
        assert_eq!(
            git.to_string(),
            "reading .git/index: permission denied",
            "the context is part of the message"
        );
        let source = std::error::Error::source(&git).expect("an io error has a source");
        assert!(source.to_string().contains("permission denied"));

        let config = ConfigError::io(
            "writing ~/.config/commitsmith/config.toml",
            io::Error::new(io::ErrorKind::NotFound, "no such directory"),
        );
        assert!(config.to_string().contains("config.toml"));
        assert!(std::error::Error::source(&config).is_some());
    }

    #[test]
    fn non_io_variants_have_no_source() {
        assert!(std::error::Error::source(&GitError::not_found("main")).is_none());
        assert!(
            std::error::Error::source(&ConfigError::not_found(ConfigScope::User)).is_none()
        );
    }

    #[test]
    fn config_errors_name_their_scope_and_location() {
        assert_eq!(
            ConfigError::NotFound {
                scope: ConfigScope::Project
            }
            .to_string(),
            "no project config found"
        );
        assert_eq!(
            ConfigError::parse(".commitsmith.toml, key `profile`", "expected a string").to_string(),
            "cannot parse .commitsmith.toml, key `profile`: expected a string"
        );
        assert_eq!(ConfigScope::User.to_string(), "user");
    }

    #[test]
    fn the_ports_can_share_the_errors_across_threads() {
        fn assert_send_sync<T: Send + Sync + 'static>() {}
        assert_send_sync::<GitError>();
        assert_send_sync::<ConfigError>();
        assert_send_sync::<ConfigScope>();
    }
}
