//! CodeReview — workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct CodeReviewHandler;

impl WorkflowHandler for CodeReviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::CodeReview
    }

    fn build_prompt(&self) -> String {
        "# $code-review — Code Review Mode\n\n\
         You are in code review mode. Analyze code for bugs, style issues, \
         and performance problems. Provide actionable feedback.\n\n\
         Strategy:\n\
         1. Read and understand the code being reviewed\n\
         2. Check for correctness bugs\n\
         3. Check for style and convention violations\n\
         4. Check for performance issues\n\
         5. Check for security concerns\n\
         6. Provide ranked feedback with line references"
            .to_string()
    }
}
