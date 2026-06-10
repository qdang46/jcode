//! `jcode-beads-bridge` — Adapter wrapping beads_rust for jcode integration.
//!
//! # Usage
//!
//! ```rust,ignore
//! use jcode_beads_bridge::{BeadsProject, BeadsTaskManager};
//!
//! let project = BeadsProject::open(working_dir)?;
//! let manager = BeadsTaskManager::new(&project);
//! let ready = manager.ready_tasks(5)?;
//! ```

pub mod mapping;
pub mod project;
pub mod tasks;

pub use project::BeadsProject;
pub use tasks::BeadsTaskManager;

// Re-export common beads_rust types so callers only need one import.
pub use beads_rust::model::{Issue, Status, Priority, IssueType};
pub use beads_rust::storage::{self, ListFilters};
pub use beads_rust::error::BeadsError;

#[cfg(test)]
mod tests;
