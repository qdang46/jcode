//! # JCode Edit Benchmark
//!
//! Mutation-based edit benchmark harness for measuring edit-tool quality
//! across different models. Generates benchmark tasks by applying AST-level
//! mutations to real Rust source files using tree-sitter, runs agents against
//! each task, and verifies results via rustfmt-normalized comparison.
//!
//! Architecture (based on oh-my-pi's typescript-edit-benchmark):
//!
//! ```text
//! Source Files → tree-sitter parse → find candidates → apply mutation
//!   → validate single-hunk change → score difficulty
//!   → package fixtures (input/expected/prompt/metadata)
//!   → run jcode agent (parallel, best-of-N)
//!   → verify with rustfmt normalization → report
//! ```

#![forbid(unsafe_code)]

pub mod types;
pub mod mutation;
pub mod difficulty;
pub mod formatter;
pub mod fixtures;
pub mod generate;
pub mod verify;
pub mod report;
pub mod runner;

pub use types::*;
use crate::types::SourceEdit;
pub use mutation::{all_mutations, Mutation, apply_source_edits};
pub use difficulty::score_difficulty;
pub use generate::generate_tasks;
pub use verify::verify_files;
pub use runner::run_benchmark;
