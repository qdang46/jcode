//! Workflow handlers — trait definition, execution context, and dispatch for keyword-triggered workflows.

use crate::registry::WorkflowKind;
use crate::state::ModeState;
use std::collections::HashMap;
use std::path::PathBuf;

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
pub struct WorkflowContext {
    /// The user's original input (with keyword stripped).
    pub user_input: String,
    /// Working directory.
    pub working_dir: Option<PathBuf>,
    /// Session ID.
    pub session_id: String,
    /// Current mode state.
    pub mode_state: ModeState,
    /// Metadata from previous turns (iteration counts, scores, etc.).
    pub metadata: HashMap<String, String>,
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
