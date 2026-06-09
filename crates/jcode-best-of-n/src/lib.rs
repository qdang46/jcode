//! # Best-of-N Editing Engine
//!
//! Implements parallel candidate execution with deterministic selection
//! for jcode's editing tools. Inspired by codebuff's best-of-n pipeline,
//! oh-my-pi's benchmark runner, and pi-agent-rust's `select_best_candidate`.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                 Orchestrator                        │
//! │  generate_strategies() → fan-out → collect → select │
//! └───┬─────────────────────────────────────────┬───────┘
//!     │  ProposedContentStore                    │
//!     │  (in-memory overlay per runId)           │
//!     ├──────────────────────────────────────────┤
//!     │  DeterministicSelector                   │
//!     │  (success → changes → focus → cost → idx)│
//!     └──────────────────────────────────────────┘
//! ```
//!
//! ## Phases
//!
//! Phase 1 (current): Core types, ProposedContentStore, deterministic
//! selector, strategy generation, config types, and the orchestrator's
//! public API foundation.
//!
//! Phase 2 (planned): `propose_edit` and `propose_write` tools, turn-loop
//! integration, auto-detection of edit tool calls.
//!
//! Phase 3 (planned): `show` mode UI, user picker for candidate diffs.
//!
//! Phase 4 (planned): Multi-model diversity (spawn across providers),
//! streaming selector, confidence metrics.

pub mod config;
pub mod selector;
pub mod store;
pub mod strategies;
pub mod types;

// Re-export key types at the crate root.
pub use config::{BestOfNConfig, BestOfNMode};
pub use selector::select_best_candidate;
pub use store::ProposedContentStore;
pub use types::{
    BestOfNResult, CandidateDiff, CandidateId, CandidateStatus, CandidateStrategy, FileDiff, RunId,
    SelectScore,
};

/// High-level entry point for running a best-of-N edit cycle.
///
/// This orchestrator coordinates strategy generation, candidate
/// execution (via the provided executor), and winner selection.
///
/// Phase 1 provides the collection + selection half. Phase 2 adds
/// the fan-out execution via the agent runtime.
#[derive(Debug, Clone)]
pub struct BestOfNOrchestrator {
    /// Configuration for this orchestrator instance.
    pub config: BestOfNConfig,
    /// Content store for proposed edits.
    pub store: ProposedContentStore,
}

impl BestOfNOrchestrator {
    /// Create a new orchestrator with the given configuration.
    pub fn new(config: BestOfNConfig) -> Self {
        Self {
            store: ProposedContentStore::new(),
            config,
        }
    }

    /// Create with explicit store (for sharing across sessions).
    pub fn with_store(config: BestOfNConfig, store: ProposedContentStore) -> Self {
        Self { config, store }
    }

    /// Generate strategies for this run based on config.
    pub fn generate_strategies(&self) -> Vec<types::CandidateStrategy> {
        let count = self.config.effective_count();
        strategies::generate_strategies(count, &self.config.temperatures)
    }

    /// Select the best candidate from completed results.
    pub fn select_winner(&self, candidates: &[types::CandidateDiff]) -> selector::SelectionResult {
        selector::select_best_candidate(candidates, &self.config.selector)
    }

    /// Build a `BestOfNResult` from candidates and selection.
    pub fn build_result(
        &self,
        run_id: types::RunId,
        candidates: Vec<types::CandidateDiff>,
        selection: &selector::SelectionResult,
    ) -> types::BestOfNResult {
        let winner = if selection.winner_index < candidates.len() {
            Some(candidates[selection.winner_index].clone())
        } else {
            None
        };

        types::BestOfNResult {
            run_id,
            winner_index: Some(selection.winner_index),
            winner,
            candidates,
            selection_reason: Some(selection.reason.clone()),
        }
    }

    /// Check if best-of-N should be applied for the current context.
    pub fn should_run(&self) -> bool {
        self.config.enabled()
    }
}
