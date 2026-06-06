//! Ultragoal — GoalTracking workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct UltragoalHandler;

impl WorkflowHandler for UltragoalHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultragoal
    }

    fn build_prompt(&self) -> String {
        "# $ultragoal — Goal Tracking Mode\n\n\
         You are in ultragoal mode. Maintain a durable goal across turns \
         with a token budget. Track progress, report status after each turn, \
         and adjust strategy based on results.\n\n\
         Strategy:\n\
         1. Define the goal clearly at the start\n\
         2. Allocate a token budget for the goal\n\
         3. Work toward the goal incrementally\n\
         4. Report progress after each turn\n\
         5. Adjust approach if progress stalls"
            .to_string()
    }
}
