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
         Generate, review, and refine plans.\n\n\
         ## Cycle\n\
         1. PLAN: Generate a detailed plan\n\
         2. REVIEW: Self-review for risks and gaps\n\
         3. REVISE: Address issues found\n\
         4. APPROVE: Present for user approval\n\n\
         ## Completion Markers\n\
         Plan ready: `[PHASE:PLAN_DONE]`\n\
         Review done: `[PHASE:REVIEW_DONE]`\n\
         Revision done: `[PHASE:REVISED]`\n\
         User approved: `[PHASE:APPROVED]`\n\
         Execution done: `[PHASE:EXECUTED]`"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let phase = ctx
            .metadata
            .get("ralplan_phase")
            .map(|s| s.as_str())
            .unwrap_or("plan");

        let reminder = match phase {
            "plan" => format!(
                "## Ralplan — Phase: PLAN\n\n\
                 Generate a detailed plan for:\n{}\n\n\
                 Include: Goal, Steps, Risks, Assumptions.\n\
                 Say `[PHASE:PLAN_DONE]` when done.",
                ctx.user_input
            ),
            "review" => "## Ralplan — Phase: REVIEW\n\n\
                 Self-review the plan:\n\
                 - What could go wrong?\n\
                 - What assumptions are we making?\n\
                 - What's missing?\n\
                 Say `[PHASE:REVIEW_DONE]` when done."
                .to_string(),
            "revise" => "## Ralplan — Phase: REVISE\n\n\
                 Revise the plan addressing review issues.\n\
                 Say `[PHASE:REVISED]` when done."
                .to_string(),
            "approve" => "## Ralplan — Phase: APPROVE\n\n\
                 Present the final plan. Wait for user approval.\n\
                 Say `[PHASE:APPROVED]` when user confirms."
                .to_string(),
            "execute" => "## Ralplan — Phase: EXECUTE\n\n\
                 Execute the approved plan step by step.\n\
                 Say `[PHASE:EXECUTED]` when done."
                .to_string(),
            _ => "Continue planning.".to_string(),
        };

        // DON'T advance phase here
        let mut metadata = ctx.metadata.clone();
        if !metadata.contains_key("ralplan_phase") {
            metadata.insert("ralplan_phase".to_string(), "plan".to_string());
        }

        WorkflowAction::ContinueWithMetadata { reminder, metadata }
    }

    fn on_turn_complete(
        &self,
        response: &str,
        metadata: &HashMap<String, String>,
    ) -> WorkflowAction {
        let phase = metadata
            .get("ralplan_phase")
            .map(|s| s.as_str())
            .unwrap_or("plan");

        let next_phase = match phase {
            "plan" if response.contains("[PHASE:PLAN_DONE]") => Some("review"),
            "review" if response.contains("[PHASE:REVIEW_DONE]") => Some("revise"),
            "revise" if response.contains("[PHASE:REVISED]") => Some("approve"),
            "approve" if response.contains("[PHASE:APPROVED]") => Some("execute"),
            "execute" if response.contains("[PHASE:EXECUTED]") => {
                return WorkflowAction::Complete("Plan executed successfully.".to_string());
            }
            _ => None,
        };

        if let Some(next) = next_phase {
            let mut updated = metadata.clone();
            updated.insert("ralplan_phase".to_string(), next.to_string());
            WorkflowAction::ContinueWithMetadata {
                reminder: format!("Advancing to {} phase.", next),
                metadata: updated,
            }
        } else {
            WorkflowAction::Continue
        }
    }
}
