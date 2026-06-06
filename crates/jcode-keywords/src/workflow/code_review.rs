//! CodeReview — workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns a reviewer agent.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler, sanitize_user_input};
use crate::registry::WorkflowKind;

pub struct CodeReviewHandler;

impl WorkflowHandler for CodeReviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::CodeReview
    }

    fn build_prompt(&self) -> String {
        "# $code-review — Code Review Mode\n\n\
         Perform thorough code review.\n\n\
         ## Checklist\n\
         - Correctness: logic errors, edge cases\n\
         - Style: naming, conventions\n\
         - Performance: unnecessary allocations\n\
         - Security: input validation, injection\n\
         - Testing: coverage, missing tests\n\n\
         ## Output\n\
         Overall: Pass / Needs Changes / Critical\n\
         Findings: Severity + Location + Issue + Suggestion"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let safe_input = sanitize_user_input(ctx.user_input);
        WorkflowAction::SpawnAgent {
            description: "Code reviewer".to_string(),
            prompt: format!(
                "Review the following code/task thoroughly:\n\n{}\n\n\
                 Provide a structured review with severity ratings.",
                safe_input
            ),
            system_prompt: "You are an expert code reviewer. Be thorough but fair. \
                           Focus on correctness, security, and maintainability. \
                           Rate each finding by severity."
                .to_string(),
            max_turns: 8,
        }
    }
}
