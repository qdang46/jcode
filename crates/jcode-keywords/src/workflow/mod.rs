//! Workflow handlers — trait definition, execution context, and dispatch for keyword-triggered workflows.

use crate::registry::WorkflowKind;
use crate::state::ModeState;
use std::collections::HashMap;

pub mod ai_slop_cleaner;
pub mod analyze;
pub mod cancel;
pub mod code_review;
pub mod deep_interview;
pub mod deepsearch;
pub mod executor;
pub mod ralplan;
pub mod security_review;
pub mod spawn;
pub mod tdd;
pub mod ultraqa;
pub mod ultragoal;
pub mod ultrathink;
pub mod ultrawork;
pub mod wiki;

/// Execution context passed to workflow handlers.
pub struct WorkflowContext<'a> {
    /// The user's original input (with keyword stripped).
    pub user_input: &'a str,
    /// Working directory.
    pub working_dir: Option<&'a std::path::Path>,
    /// Session ID.
    pub session_id: &'a str,
    /// Current mode state (borrowed, not cloned).
    pub mode_state: &'a ModeState,
    /// Metadata from the current active mode.
    pub metadata: &'a HashMap<String, String>,
}

/// Action a workflow handler wants the turn loop to take.
#[derive(Debug, Clone)]
pub enum WorkflowAction {
    /// Inject a system reminder into the current turn's dynamic prompt.
    InjectReminder(String),
    /// Spawn a single sub-agent and wait for result.
    SpawnAgent {
        description: String,
        prompt: String,
        system_prompt: String,
        max_turns: u32,
    },
    /// Spawn multiple sub-agents in parallel, aggregate results.
    SpawnParallel(Vec<SpawnSpec>),
    /// Ask the user a question (pauses workflow, resumes next turn).
    AskUser(String),
    /// Continue with normal LLM turn (prompt-only mode).
    Continue,
    /// Workflow complete, deactivate mode. Contains summary message.
    Complete(String),
    /// Workflow needs more turns, continue with updated metadata.
    ContinueWithMetadata {
        reminder: String,
        metadata: HashMap<String, String>,
    },
    /// Workflow encountered an error.
    Error(String),
}

/// Specification for spawning a sub-agent.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub description: String,
    pub prompt: String,
    pub system_prompt: String,
    pub max_turns: u32,
}

/// Result of a spawned sub-agent.
#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub description: String,
    pub output: String,
    pub success: bool,
}

/// Enhanced workflow handler trait.
pub trait WorkflowHandler: Send + Sync {
    /// The workflow kind this handler implements.
    fn kind(&self) -> WorkflowKind;

    /// Build the prompt injection for this workflow (shown in system prompt).
    fn build_prompt(&self) -> String;

    /// Execute the workflow. Called at the start of each turn while mode is active.
    /// Default: prompt-only mode (just inject instructions).
    fn execute(&self, _ctx: &WorkflowContext) -> WorkflowAction {
        WorkflowAction::Continue
    }

    /// Called after each turn to process the LLM's response and decide next action.
    /// Default: no-op, workflow continues.
    fn on_turn_complete(
        &self,
        _response: &str,
        _metadata: &HashMap<String, String>,
    ) -> WorkflowAction {
        WorkflowAction::Continue
    }

    /// Whether this workflow should suppress its heavy behavior for simple tasks.
    fn should_suppress_for_task_size(&self, task_size: crate::task_size::TaskSize) -> bool {
        crate::task_size::should_suppress(self.kind(), task_size)
    }
}

/// Get a handler reference for a workflow kind (zero-allocation dispatch).
pub fn get_handler(kind: WorkflowKind) -> Option<&'static dyn WorkflowHandler> {
    Some(match kind {
        WorkflowKind::Ultrawork => &ultrawork::UltraworkHandler,
        WorkflowKind::Ultragoal => &ultragoal::UltragoalHandler,
        WorkflowKind::Ultraqa => &ultraqa::UltraqaHandler,
        WorkflowKind::Ralplan => &ralplan::RalplanHandler,
        WorkflowKind::DeepInterview => &deep_interview::DeepInterviewHandler,
        WorkflowKind::Tdd => &tdd::TddHandler,
        WorkflowKind::CodeReview => &code_review::CodeReviewHandler,
        WorkflowKind::SecurityReview => &security_review::SecurityReviewHandler,
        WorkflowKind::Ultrathink => &ultrathink::UltrathinkHandler,
        WorkflowKind::Deepsearch => &deepsearch::DeepsearchHandler,
        WorkflowKind::Analyze => &analyze::AnalyzeHandler,
        WorkflowKind::Wiki => &wiki::WikiHandler,
        WorkflowKind::AiSlopCleaner => &ai_slop_cleaner::AiSlopCleanerHandler,
        WorkflowKind::Cancel => &cancel::CancelHandler,
    })
}

/// Wrap user input in delimiters to prevent prompt injection in sub-agent prompts.
/// Escapes the closing delimiter within the input to prevent breakout attacks.
pub fn sanitize_user_input(input: &str) -> String {
    let escaped = input.replace("</user_request>", "<\\/user_request>");
    format!("<user_request>\n{}\n</user_request>", escaped)
}
