//! CodeReview — workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns a reviewer agent.

use super::SpawnSpec;
use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct CodeReviewHandler;

impl WorkflowHandler for CodeReviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::CodeReview
    }

    fn build_prompt(&self) -> String {
        "# $code-review — Code Review Mode\n\n\
         You are in code review mode. Perform thorough code review.\n\n\
         ## Review Checklist\n\
         1. **Correctness** — Logic errors, edge cases, off-by-one\n\
         2. **Style** — Naming, formatting, conventions\n\
         3. **Performance** — Unnecessary allocations, O(n²) loops\n\
         4. **Security** — Input validation, injection, secrets\n\
         5. **Maintainability** — Complexity, coupling, cohesion\n\
         6. **Testing** — Coverage, test quality, missing tests\n\n\
         ## Output Format\n\
         ### Overall Assessment\n\
         Pass / Needs Changes / Critical Issues\n\n\
         ### Findings\n\
         For each finding:\n\
         - **Severity**: Critical / High / Medium / Low / Nit\n\
         - **Location**: file:line\n\
         - **Issue**: Description\n\
         - **Suggestion**: How to fix"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let spec = SpawnSpec {
            description: "Code reviewer".to_string(),
            prompt: format!(
                "Review the following code/task thoroughly:\n\n{}\n\n\
                 Provide a structured review with severity ratings.",
                ctx.user_input
            ),
            system_prompt: "You are an expert code reviewer. Be thorough but fair. \
                           Focus on correctness, security, and maintainability. \
                           Rate each finding by severity."
                .to_string(),
            max_turns: 8,
        };

        WorkflowAction::SpawnAgent {
            description: spec.description.clone(),
            prompt: spec.prompt.clone(),
            system_prompt: spec.system_prompt.clone(),
            max_turns: spec.max_turns,
        }
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        WorkflowAction::Complete("Code review complete.".to_string())
    }
}
