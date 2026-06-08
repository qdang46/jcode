//! Team mode: multi-agent coordination with tmux panes, a file-based mailbox,
//! and a dependency-aware task board.
//!
//! Rust port of oh-my-openagent `src/features/team-mode/`. See
//! `.claude/skills/feature-planning/plans/issue-390-tmux-team-viz.md`.

pub mod eligibility;
pub mod layout;
pub mod locks;
pub mod mailbox;
pub mod paths;
pub mod runtime;
pub mod spec;
pub mod state;
pub mod tasklist;

#[cfg(test)]
pub(crate) mod test_support;

pub use spec::*;
