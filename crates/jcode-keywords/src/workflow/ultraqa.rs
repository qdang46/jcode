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
        "# $ultraqa — QA Cycling Mode\n\n\
         You are in ultraqa mode. Run QA cycles until all tests pass.\n\n\
         ## Cycle\n\
         1. **Implement** — Write/modify the code\n\
         2. **Test** — Run relevant tests\n\
         3. **Fix** — If failures, analyze and fix\n\
         4. **Repeat** — Until all tests pass (max 5 iterations)\n\n\
         ## Rules\n\
         - After each fix, re-run ALL tests (not just failed ones)\n\
         - If stuck after 3 attempts on same error, ask for help\n\
         - Report: 'Iteration N/5: X tests passing, Y failing'"
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
            "implement" => {
                format!(
                    "## QA Cycle — Iteration {}/{}\n\n\
                     **Phase: IMPLEMENT**\n\
                     Implement the requested change:\n{}\n\n\
                     After implementing, run tests and report results.",
                    iteration + 1,
                    MAX_ITERATIONS,
                    ctx.user_input
                )
            }
            "test" => {
                "## QA Cycle — Phase: TEST\n\n\
                 Run all relevant tests. Report:\n\
                 - Total tests: N\n\
                 - Passing: N\n\
                 - Failing: N (with error messages)"
                    .to_string()
            }
            "fix" => {
                "## QA Cycle — Phase: FIX\n\n\
                 Analyze test failures and fix them.\n\
                 After fixing, re-run all tests."
                    .to_string()
            }
            _ => "Continue QA cycle.".to_string(),
        };

        let mut metadata = HashMap::new();
        metadata.insert("qa_iteration".to_string(), (iteration + 1).to_string());
        metadata.insert(
            "qa_phase".to_string(),
            match phase {
                "implement" => "test",
                "test" => "fix",
                "fix" => "test",
                _ => "implement",
            }
            .to_string(),
        );

        WorkflowAction::ContinueWithMetadata {
            reminder,
            metadata,
        }
    }

    fn on_turn_complete(&self, response: &str, metadata: &HashMap<String, String>) -> WorkflowAction {
        let iteration: u32 = metadata
            .get("qa_iteration")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Check if tests are passing
        if response.contains("all tests pass")
            || response.contains("0 failing")
            || response.contains("All tests passed")
        {
            return WorkflowAction::Complete(format!(
                "QA cycling complete after {} iterations. All tests passing.",
                iteration
            ));
        }

        if iteration >= MAX_ITERATIONS {
            return WorkflowAction::Complete(format!(
                "QA cycling reached max iterations ({}). Some tests may still be failing.",
                MAX_ITERATIONS
            ));
        }

        WorkflowAction::Continue
    }
}
