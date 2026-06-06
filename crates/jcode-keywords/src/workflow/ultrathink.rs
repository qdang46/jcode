//! Ultrathink — ExtendedThinking workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct UltrathinkHandler;

impl WorkflowHandler for UltrathinkHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultrathink
    }

    fn build_prompt(&self) -> String {
        "# $ultrathink — Extended Thinking Mode\n\n\
         You are in ultrathink mode. Reason deeply about the problem.\n\n\
         Strategy:\n\
         1. Consider the problem from multiple angles\n\
         2. Identify edge cases and boundary conditions\n\
         3. Evaluate trade-offs between approaches\n\
         4. Consider alternatives and their implications\n\
         5. Provide thorough analysis with reasoning chain"
            .to_string()
    }
}
