//! Cancel — CancelAll workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct CancelHandler;

impl WorkflowHandler for CancelHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Cancel
    }

    fn build_prompt(&self) -> String {
        "# canceljcode — All Modes Cancelled\n\n\
         All keyword modes have been deactivated. \
         Returning to normal operation."
            .to_string()
    }
}
