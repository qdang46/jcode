//! Workflow handlers — trait definition and dispatch for keyword-triggered workflows.

use crate::registry::WorkflowKind;

pub mod ai_slop_cleaner;
pub mod analyze;
pub mod cancel;
pub mod code_review;
pub mod deep_interview;
pub mod deepsearch;
pub mod ralplan;
pub mod security_review;
pub mod tdd;
pub mod ultraqa;
pub mod ultragoal;
pub mod ultrathink;
pub mod ultrawork;
pub mod wiki;

/// Result of executing a workflow handler.
#[derive(Debug, Clone)]
pub struct WorkflowResult {
    /// Whether the workflow completed successfully.
    pub success: bool,
    /// Human-readable summary of what was done.
    pub summary: String,
    /// Optional prompt text to inject into the conversation.
    pub prompt_injection: Option<String>,
}

/// Trait for workflow handlers.
pub trait WorkflowHandler: Send + Sync {
    /// The workflow kind this handler implements.
    fn kind(&self) -> WorkflowKind;

    /// Build the prompt injection for this workflow.
    ///
    /// This is called at the start of each turn while the workflow is active.
    fn build_prompt(&self) -> String;

    /// Check if this workflow should suppress its heavy behavior
    /// given the task size.
    fn should_suppress_for_task_size(&self, task_size: crate::task_size::TaskSize) -> bool {
        crate::task_size::should_suppress(self.kind(), task_size)
    }
}

/// Get all workflow handlers.
pub fn all_handlers() -> Vec<Box<dyn WorkflowHandler>> {
    vec![
        Box::new(ultrawork::UltraworkHandler),
        Box::new(ultragoal::UltragoalHandler),
        Box::new(ultraqa::UltraqaHandler),
        Box::new(ralplan::RalplanHandler),
        Box::new(deep_interview::DeepInterviewHandler),
        Box::new(tdd::TddHandler),
        Box::new(code_review::CodeReviewHandler),
        Box::new(security_review::SecurityReviewHandler),
        Box::new(ultrathink::UltrathinkHandler),
        Box::new(deepsearch::DeepsearchHandler),
        Box::new(analyze::AnalyzeHandler),
        Box::new(wiki::WikiHandler),
        Box::new(ai_slop_cleaner::AiSlopCleanerHandler),
        Box::new(cancel::CancelHandler),
    ]
}

/// Dispatch to the appropriate handler for a workflow kind.
pub fn get_handler(kind: WorkflowKind) -> Option<Box<dyn WorkflowHandler>> {
    all_handlers().into_iter().find(|h| h.kind() == kind)
}
