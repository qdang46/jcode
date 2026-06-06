//! Ralplan — ConsensusPlanning workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct RalplanHandler;

impl WorkflowHandler for RalplanHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ralplan
    }

    fn build_prompt(&self) -> String {
        "# $ralplan — Consensus Planning Mode\n\n\
         You are in ralplan mode. Generate a plan, run adversarial review, \
         revise based on feedback, and get approval before executing.\n\n\
         Strategy:\n\
         1. Generate an initial plan with clear steps\n\
         2. Self-review: identify risks, gaps, assumptions\n\
         3. Revise the plan addressing found issues\n\
         4. Present the revised plan for user approval\n\
         5. Only execute after explicit approval"
            .to_string()
    }
}
