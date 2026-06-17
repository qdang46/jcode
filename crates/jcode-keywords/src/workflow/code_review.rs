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
        "# $code-review — Code Review Mode

MANDATORY: Say \"CODE REVIEW MODE ENABLED!\" as your first response.

## Review Checklist
1. Correctness — Logic errors, edge cases, off-by-one
2. Style — Naming conventions, formatting, idioms
3. Performance — Unnecessary allocations, O(n²) patterns
4. Security — Input validation, injection, XSS, CSRF
5. Testing — Coverage gaps, missing edge cases
6. Maintainability — Code duplication, complexity

## Output Format
\
```
Overall: Pass / Needs Changes / Critical

### Findings
- [Severity: High] file.rs:42 — Description + Suggestion
- [Severity: Medium] other.rs:13 — Description + Suggestion
```"
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
