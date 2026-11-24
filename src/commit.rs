//! Re-export of the commit model, which now lives in the domain layer.
//!
//! The binary keeps this module until E1-9 rewrites the call sites; the code and
//! its tests live in `src/domain/commit.rs`.

pub use commitsmith::domain::commit::*;
