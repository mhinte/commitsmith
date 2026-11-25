//! Infrastructure adapters.
//!
//! Everything that touches the outside world lives here: `gix` for reading,
//! the `git` CLI for writing, TOML files for the config. The application only
//! knows the traits from [`crate::domain::ports`].

pub mod config_toml;
pub mod git_cli;
pub mod gix_repo;
