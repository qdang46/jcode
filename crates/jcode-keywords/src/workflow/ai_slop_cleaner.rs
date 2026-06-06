//! AiSlopCleaner — SlopCleanup workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct AiSlopCleanerHandler;

impl WorkflowHandler for AiSlopCleanerHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::AiSlopCleaner
    }

    fn build_prompt(&self) -> String {
        "# ai-slop-cleaner — AI Slop Cleanup Mode\n\n\
         You are in AI slop cleanup mode. Detect and fix low-quality \
         AI-generated code.\n\n\
         Strategy:\n\
         1. Scan for redundant/obvious comments\n\
         2. Find over-abstraction and unnecessary wrappers\n\
         3. Detect dead code and unused variables\n\
         4. Identify verbose patterns that could be simplified\n\
         5. Fix with minimal, clean replacements"
            .to_string()
    }
}
