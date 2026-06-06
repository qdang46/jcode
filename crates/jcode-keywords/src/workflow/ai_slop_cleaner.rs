//! AiSlopCleaner — SlopCleanup workflow handler.
//!
//! Tier 1: Prompt-only. Injects AI code quality improvement instructions.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct AiSlopCleanerHandler;

impl WorkflowHandler for AiSlopCleanerHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::AiSlopCleaner
    }

    fn build_prompt(&self) -> String {
        "# ai-slop-cleaner — AI Slop Cleanup Mode\n\n\
         You are in AI slop cleanup mode. Detect and fix low-quality AI-generated code.\n\n\
         ## What to Look For\n\
         1. **Redundant comments** — Comments that restate the code\n\
         2. **Over-abstraction** — Unnecessary wrappers, factories, builders\n\
         3. **Dead code** — Unused imports, variables, functions, modules\n\
         4. **Verbose patterns** — Could be simplified (e.g., match → if let)\n\
         5. **Generic names** — `data`, `result`, `temp`, `helper`, `utils`\n\
         6. **Copy-paste patterns** — Duplicated logic that should be extracted\n\
         7. **Unnecessary clones** — `.clone()` where borrow would work\n\
         8. **Excessive error handling** — `.unwrap()` chains, verbose match arms\n\n\
         ## For Each Issue\n\
         - **Location**: file:line\n\
         - **Problem**: What's wrong\n\
         - **Fix**: Clean replacement code\n\
         - **Why**: Why the fix is better\n\n\
         ## Rules\n\
         - Don't change behavior, only improve quality\n\
         - Preserve all public API contracts\n\
         - Keep fixes minimal and focused"
            .to_string()
    }

    fn execute(&self, _ctx: &WorkflowContext) -> WorkflowAction {
        WorkflowAction::Continue
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        WorkflowAction::Complete("AI slop cleanup complete.".to_string())
    }
}
