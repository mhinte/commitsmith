//! Re-export of the gix reader, which now lives in `src/infrastructure/`.
//!
//! The binary keeps this module until E1-9 rewrites the call sites.

pub use commitsmith::domain::history::{PastCommit, ScopeInfo};
pub use commitsmith::infrastructure::gix_repo::*;
