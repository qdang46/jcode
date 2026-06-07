//! Hooks module — re-exports from the `jcode-hooks` crate.
//!
//! This thin wrapper allows existing `crate::hooks::` import paths to keep
//! working while the actual implementation lives in `crates/jcode-hooks`.

pub use jcode_hooks::*;
