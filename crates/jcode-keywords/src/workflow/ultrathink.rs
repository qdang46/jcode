//! Ultrathink — ExtendedThinking workflow handler.
//!
//! Tier 1: Prompt-only. Injects deep reasoning instructions into system prompt.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct UltrathinkHandler;

impl WorkflowHandler for UltrathinkHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultrathink
    }

    fn build_prompt(&self) -> String {
        "# $ultrathink — Extended Thinking Mode\n\n\
         You are in ultrathink mode. Reason deeply and thoroughly about the problem.\n\n\
         ## Strategy\n\
         1. **Decompose** — Break the problem into atomic components\n\
         2. **Analyze each component** — Consider edge cases, boundary conditions, failure modes\n\
         3. **Evaluate trade-offs** — Compare at least 3 approaches with pros/cons\n\
         4. **Consider alternatives** — What would a skeptical reviewer suggest?\n\
         5. **Synthesize** — Combine findings into a coherent analysis\n\
         6. **Recommend** — Provide ranked recommendations with clear rationale\n\n\
         ## Output Format\n\
         - Start with a one-sentence summary of your conclusion\n\
         - Then provide the detailed reasoning chain\n\
         - End with actionable next steps"
            .to_string()
    }

    fn execute(&self, _ctx: &WorkflowContext) -> WorkflowAction {
        // Prompt-only: the system prompt injection is sufficient
        WorkflowAction::Continue
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        // Ultrathink is single-turn, deactivate after one response
        WorkflowAction::Complete("Extended thinking complete.".to_string())
    }
}
