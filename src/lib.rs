//! Library root.
//!
//! The crate is split into layers that may only depend downwards:
//!
//! - `domain`: value objects and aggregates. No UI, no git, no file system.
//! - `infrastructure`: adapters (gix, the git CLI, TOML files).
//! - `application`: orchestration and state, no rendering.
//! - `ui`: `ratatui` views.
//!
//! The binary keeps its own module tree until the migration in E1-9 is done;
//! new code lands in the layers below.

pub mod domain;
