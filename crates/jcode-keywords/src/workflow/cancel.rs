//! Cancel — CancelAll workflow handler.
//!
//! Tier 6: System action. Clears all active modes and cancels tasks.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct CancelHandler;

impl WorkflowHandler for CancelHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Cancel
    }

    fn build_prompt(&self) -> String {
        "# canceljcode — All Modes Cancelled\n\n\
         All keyword modes have been deactivated.\n\
         Returning to normal operation."
            .to_string()
    }

    fn execute(&self, _ctx: &WorkflowContext) -> WorkflowAction {
        // Cancel is handled by state::update_modes() which clears all modes.
        // This handler just provides the completion message.
        WorkflowAction::Complete(
            "✅ All modes cancelled. Returning to normal operation.".to_string(),
        )
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        // Cancel should never need multiple turns
        WorkflowAction::Complete("All modes cancelled.".to_string())
    }
}
