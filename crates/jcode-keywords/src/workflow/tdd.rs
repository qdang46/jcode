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
        "# $tdd — Test-Driven Development Mode\n\n\
         You are in TDD mode. Follow the Red → Green → Refactor cycle.\n\n\
         ## Cycle\n\
         1. **RED** — Write a failing test that describes the desired behavior\n\
         2. **GREEN** — Write the minimal code to make the test pass\n\
         3. **REFACTOR** — Clean up the code while keeping tests green\n\
         4. **Repeat** — For each new behavior\n\n\
         ## Rules\n\
         - Never write production code without a failing test\n\
         - Write the simplest code that works\n\
         - Refactor only when tests are green\n\
         - One behavior per cycle\n\n\
         ## Output\n\
         After each cycle:\n\
         - Test written: [test name]\n\
         - Status: RED → GREEN → REFACTORED\n\
         - Coverage: X%"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let phase = ctx
            .metadata
            .get("tdd_phase")
            .map(|s| s.as_str())
            .unwrap_or("red");

        let reminder = match phase {
            "red" => {
                format!(
                    "## TDD — Phase: RED\n\n\
                     Write a FAILING test for the following behavior:\n{}\n\n\
                     The test must fail when run. Report: 'Test [name] written — RED'",
                    ctx.user_input
                )
            }
            "green" => {
                "## TDD — Phase: GREEN\n\n\
                 Write the MINIMAL code to make the failing test pass.\n\
                 Don't over-engineer. Report: 'Implementation done — GREEN'"
                    .to_string()
            }
            "refactor" => {
                "## TDD — Phase: REFACTOR\n\n\
                 Clean up the code while keeping all tests green.\n\
                 - Remove duplication\n\
                 - Improve naming\n\
                 - Simplify logic\n\
                 Report: 'Refactoring done — all tests still GREEN'"
                    .to_string()
            }
            _ => "Continue TDD cycle.".to_string(),
        };

        let mut metadata = HashMap::new();
        metadata.insert(
            "tdd_phase".to_string(),
            match phase {
                "red" => "green",
                "green" => "refactor",
                "refactor" => "red",
                _ => "red",
            }
            .to_string(),
        );

        WorkflowAction::ContinueWithMetadata {
            reminder,
            metadata,
        }
    }

    fn on_turn_complete(&self, response: &str, metadata: &HashMap<String, String>) -> WorkflowAction {
        let phase = metadata
            .get("tdd_phase")
            .map(|s| s.as_str())
            .unwrap_or("red");

        // Check for phase completion signals
        match phase {
            "red" if response.contains("RED") || response.contains("failing") => {
                // Test is failing as expected, move to green
                WorkflowAction::Continue
            }
            "green" if response.contains("GREEN") || response.contains("passing") => {
                // Implementation works, move to refactor
                WorkflowAction::Continue
            }
            "refactor" if response.contains("REFACTORED") || response.contains("green") => {
                // Cycle complete
                WorkflowAction::Complete(
                    "TDD cycle complete. Code is tested and refactored.".to_string(),
                )
            }
            _ => WorkflowAction::Continue,
        }
    }
}
