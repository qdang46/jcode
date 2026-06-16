//! Tdd — TestDrivenDev workflow handler.
//!
//! Tier 3: Loop orchestration. Runs red → green → refactor cycles.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct TddHandler;

impl WorkflowHandler for TddHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Tdd
    }

    fn build_prompt(&self) -> String {
        "# $tdd — Test-Driven Development Mode

MANDATORY: Say "TDD MODE ENABLED!" as your first response.

## Red → Green → Refactor Cycle
1. RED — Write a failing test FIRST
Verify the test fails with actual output
2. GREEN — Write minimal code to pass
Verify all tests pass before continuing
3. REFACTOR — Clean up while keeping tests green

## Rules
- NEVER write code without a failing test
- Write the simplest code that works
- Refactor only when tests are green
- Never skip the RED phase

## Completion Markers
Red done: [PHASE:RED_DONE]
Green done: [PHASE:GREEN_DONE]
Refactored: [PHASE:REFACTORED]
TDD Complete: [MODE:TDD_COMPLETE]"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let phase = ctx
            .metadata
            .get("tdd_phase")
            .map(|s| s.as_str())
            .unwrap_or("red");

        let reminder = match phase {
            "red" => format!(
                "## TDD — Phase: RED\n\n\
                 Write a FAILING test for:\n{}\n\n\
                 The test must fail. Say `[PHASE:RED_DONE]` when done.",
                ctx.user_input
            ),
            "green" => "## TDD — Phase: GREEN\n\n\
                 Write MINIMAL code to make the failing test pass.\n\
                 Say `[PHASE:GREEN_DONE]` when done."
                .to_string(),
            "refactor" => "## TDD — Phase: REFACTOR\n\n\
                 Clean up the code. Keep all tests green.\n\
                 Say `[PHASE:REFACTORED]` when done."
                .to_string(),
            _ => "Continue TDD cycle.".to_string(),
        };

        // DON'T advance phase here — let on_turn_complete do it
        let mut metadata = ctx.metadata.clone();
        if !metadata.contains_key("tdd_phase") {
            metadata.insert("tdd_phase".to_string(), "red".to_string());
        }

        WorkflowAction::ContinueWithMetadata { reminder, metadata }
    }

    fn on_turn_complete(
        &self,
        response: &str,
        metadata: &HashMap<String, String>,
    ) -> WorkflowAction {
        let phase = metadata
            .get("tdd_phase")
            .map(|s| s.as_str())
            .unwrap_or("red");

        // Use structured markers instead of fragile string matching
        let next_phase = match phase {
            "red" if response.contains("[PHASE:RED_DONE]") => Some("green"),
            "green" if response.contains("[PHASE:GREEN_DONE]") => Some("refactor"),
            "refactor" if response.contains("[PHASE:REFACTORED]") => {
                return WorkflowAction::Complete(
                    "TDD cycle complete. Code is tested and refactored.".to_string(),
                );
            }
            _ => None,
        };

        if let Some(next) = next_phase {
            let mut updated = metadata.clone();
            updated.insert("tdd_phase".to_string(), next.to_string());
            WorkflowAction::ContinueWithMetadata {
                reminder: format!("Advancing to {} phase.", next),
                metadata: updated,
            }
        } else {
            WorkflowAction::Continue
        }
    }
}
