//! Ultraqa — QACycling workflow handler.
//!
//! Tier 3: Loop orchestration. Runs implement → test → fix cycles.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct UltraqaHandler;

const MAX_ITERATIONS: u32 = 5;

impl WorkflowHandler for UltraqaHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultraqa
    }

    fn build_prompt(&self) -> String {
        "# $ultraqa — QA Cycling Mode

MANDATORY: Say "QA MODE ENABLED!" as your first response.

## QA Protocol (max 5 iterations)
1. IMPLEMENT — Write or modify code
2. TEST — Run tests, report ALL results
3. FIX — Fix ALL failures found
4. RE-TEST — Verify all fixes pass

## Rules
- Never skip the test phase
- Report actual test output, not assumptions
- If tests fail, fix ALL failures before moving on
- Max 5 cycles, then report what remains

## Completion Markers
Implement done: [PHASE:IMPL_DONE]
Tests pass: [PHASE:TESTS_PASS]
Fix done: [PHASE:FIX_DONE]
QA Complete: [MODE:QA_COMPLETE]"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let iteration: u32 = ctx
            .metadata
            .get("qa_iteration")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if iteration >= MAX_ITERATIONS {
            return WorkflowAction::Complete(format!(
                "QA cycling complete after {} iterations.",
                iteration
            ));
        }

        let phase = ctx
            .metadata
            .get("qa_phase")
            .map(|s| s.as_str())
            .unwrap_or("implement");

        let reminder = match phase {
            "implement" => format!(
                "## QA Cycle — Iteration {}/{}\n\n\
                 **Phase: IMPLEMENT**\n\
                 Implement:\n{}\n\n\
                 Say `[PHASE:IMPL_DONE]` when done.",
                iteration + 1,
                MAX_ITERATIONS,
                ctx.user_input
            ),
            "test" => "## QA Cycle — Phase: TEST\n\n\
                 Run all tests. Report results.\n\
                 If all pass, say `[PHASE:TESTS_PASS]`."
                .to_string(),
            "fix" => "## QA Cycle — Phase: FIX\n\n\
                 Fix test failures. Re-run tests.\n\
                 Say `[PHASE:FIX_DONE]` when done."
                .to_string(),
            _ => "Continue QA cycle.".to_string(),
        };

        // DON'T advance phase here
        let mut metadata = ctx.metadata.clone();
        if !metadata.contains_key("qa_phase") {
            metadata.insert("qa_phase".to_string(), "implement".to_string());
        }
        if !metadata.contains_key("qa_iteration") {
            metadata.insert("qa_iteration".to_string(), "0".to_string());
        }

        WorkflowAction::ContinueWithMetadata { reminder, metadata }
    }

    fn on_turn_complete(
        &self,
        response: &str,
        metadata: &HashMap<String, String>,
    ) -> WorkflowAction {
        let iteration: u32 = metadata
            .get("qa_iteration")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let phase = metadata
            .get("qa_phase")
            .map(|s| s.as_str())
            .unwrap_or("implement");

        let mut updated = metadata.clone();

        let next_phase = match phase {
            "implement" if response.contains("[PHASE:IMPL_DONE]") => "test",
            "test" if response.contains("[PHASE:TESTS_PASS]") => {
                return WorkflowAction::Complete(format!(
                    "All tests passing after {} iterations.",
                    iteration
                ));
            }
            "test" => "fix", // Tests failed, move to fix
            "fix" if response.contains("[PHASE:FIX_DONE]") => "test",
            _ => return WorkflowAction::Continue,
        };

        // Increment iteration when cycling back to test from fix
        if phase == "fix" && next_phase == "test" {
            updated.insert("qa_iteration".to_string(), (iteration + 1).to_string());
        }

        updated.insert("qa_phase".to_string(), next_phase.to_string());

        WorkflowAction::ContinueWithMetadata {
            reminder: format!("Advancing to {} phase.", next_phase),
            metadata: updated,
        }
    }
}
