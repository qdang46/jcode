//! Ultrathink — ExtendedThinking workflow handler.
//!
//! Tier 1: Prompt-only. Injects deep reasoning instructions into system prompt.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct UltrathinkHandler;

impl WorkflowHandler for UltrathinkHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultrathink
    }

    fn build_prompt(&self) -> String {
        "# $ultrathink — Extended Thinking Mode

MANDATORY: Say "ULTRATHINK MODE ENABLED!" as your first response.

## Deep Reasoning Protocol
1. Problem decomposition — Break into sub-problems
2. Assumptions — List and validate each assumption
3. Alternatives — Evaluate 2-3 approaches minimum
4. Trade-offs — Compare: complexity vs flexibility vs performance
5. Edge cases — Boundary conditions, error states
6. Conclusion — Clear recommendation with reasoning chain

## Rules
- No implementation during thinking
- Present reasoning before any code
- If uncertain, state confidence level"
            .to_string()
    }

    // Use trait default: Continue (no-op execute, no-op on_turn_complete)
    // Defer to turn-limit expiration in state::update_modes
}
