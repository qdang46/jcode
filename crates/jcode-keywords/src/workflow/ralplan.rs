//! Ralplan — ConsensusPlanning workflow handler.
//!
//! Tier 3: Loop orchestration. Runs plan → review → revise → approve cycles.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct RalplanHandler;

impl WorkflowHandler for RalplanHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ralplan
    }

    fn build_prompt(&self) -> String {
        "# $ralplan — Consensus Planning Mode\n\n\
         You are in ralplan mode. Generate, review, and refine plans.\n\n\
         ## Cycle\n\
         1. **Plan** — Generate an initial plan with clear steps\n\
         2. **Review** — Self-review: identify risks, gaps, assumptions\n\
         3. **Revise** — Address issues found in review\n\
         4. **Approve** — Present final plan for user approval\n\
         5. **Execute** — Only after explicit approval\n\n\
         ## Plan Format\n\
         ### Goal\n\
         What we're trying to achieve.\n\n\
         ### Steps\n\
         1. [ ] Step 1 — Description\n\
         2. [ ] Step 2 — Description\n\n\
         ### Risks\n\
         - Risk 1: Mitigation\n\
         - Risk 2: Mitigation\n\n\
         ### Assumptions\n\
         - Assumption 1\n\
         - Assumption 2"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let phase = ctx
            .metadata
            .get("ralplan_phase")
            .map(|s| s.as_str())
            .unwrap_or("plan");

        let reminder = match phase {
            "plan" => {
                format!(
                    "## Ralplan — Phase: PLAN\n\n\
                     Generate a detailed plan for:\n{}\n\n\
                     Include: Goal, Steps, Risks, Assumptions.",
                    ctx.user_input
                )
            }
            "review" => {
                "## Ralplan — Phase: REVIEW\n\n\
                 Self-review the plan:\n\
                 - What could go wrong?\n\
                 - What assumptions are we making?\n\
                 - What's missing?\n\
                 - What are the dependencies?"
                    .to_string()
            }
            "revise" => {
                "## Ralplan — Phase: REVISE\n\n\
                 Revise the plan addressing the issues found in review.\n\
                 Present the updated plan."
                    .to_string()
            }
            "approve" => {
                "## Ralplan — Phase: APPROVE\n\n\
                 Present the final plan for approval.\n\
                 Wait for user confirmation before executing."
                    .to_string()
            }
            "execute" => {
                "## Ralplan — Phase: EXECUTE\n\n\
                 The plan is approved. Execute each step in order.\n\
                 Report progress after each step."
                    .to_string()
            }
            _ => "Continue planning.".to_string(),
        };

        let mut metadata = HashMap::new();
        metadata.insert(
            "ralplan_phase".to_string(),
            match phase {
                "plan" => "review",
                "review" => "revise",
                "revise" => "approve",
                "approve" => "execute",
                "execute" => "execute",
                _ => "plan",
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
            .get("ralplan_phase")
            .map(|s| s.as_str())
            .unwrap_or("plan");

        match phase {
            "approve" if response.contains("approved") || response.contains("yes") => {
                // User approved, move to execute
                WorkflowAction::Continue
            }
            "execute" if response.contains("complete") || response.contains("done") => {
                // Execution complete
                WorkflowAction::Complete(
                    "Plan executed successfully.".to_string(),
                )
            }
            _ => WorkflowAction::Continue,
        }
    }
}
