//! The domain layer.
//!
//! Rules live here, not in the UI and not in an adapter: a ticket id knows how it
//! is spelled, an author slug knows which characters it may contain. Nothing in
//! this module touches the terminal, the file system or a repository.

pub mod author;
pub mod branch;
pub mod commit;
pub mod errors;
pub mod hints;
pub mod history;
pub mod keymap;
pub mod log;
pub mod ports;
pub mod prefill;
pub mod release;
pub mod repair;
pub mod scroll;
pub mod setup;
pub mod slug;
pub mod text;
pub mod ticket;
pub mod update;
pub mod validate;

pub use author::{AuthorSlug, AuthorSlugError};
pub use branch::{BranchError, BranchKind, BranchTemplate, ConventionalBranchName};
pub use log::Filter;
pub use release::{Bump, ReleasePlan, SemVer};
pub use setup::{Reference, SetupAnswers, SetupError};
pub use slug::{BranchSlug, MAX_SLUG_LEN, SlugError};
pub use ticket::{TicketId, TicketIdError};
pub use validate::{Issue, Rules, Severity};

/// Everything that can go wrong when a user typed a value object.
///
/// Kept as one type per object rather than a string so callers can render the
/// message in the language of their surface and still test for the case.
pub trait DomainError: std::error::Error + Send + Sync + 'static {}
