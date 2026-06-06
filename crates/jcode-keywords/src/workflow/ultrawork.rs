//! Ultrawork — ParallelExecution workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns parallel sub-agents for independent subtasks.

use super::{SpawnSpec, WorkflowAction, WorkflowContext, WorkflowHandler, sanitize_user_input};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct UltraworkHandler;

impl WorkflowHandler for UltraworkHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultrawork
    }

    fn build_prompt(&self) -> String {
        "# $ultrawork — Parallel Execution Mode\n\n\
         Execute the task using parallel sub-agents.\n\n\
         ## Strategy\n\
         1. Break task into independent subtasks\n\
         2. Launch up to 4 parallel sub-agents\n\
         3. Coordinate results, handle failures\n\
         4. Aggregate into unified response"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        // Guard: don't re-spawn if already spawned this session
        if ctx.metadata.contains_key("ultrawork_spawned") {
            return WorkflowAction::Continue;
        }

        let safe_input = sanitize_user_input(ctx.user_input);
        let specs = vec![
            SpawnSpec {
                description: "Analysis subtask".to_string(),
                prompt: format!("Analyze the following task:\n{}", safe_input),
                system_prompt:
                    "You are an analysis sub-agent. Identify key components and dependencies."
                        .to_string(),
                max_turns: 5,
            },
            SpawnSpec {
                description: "Implementation subtask".to_string(),
                prompt: format!("Implement the core functionality for:\n{}", safe_input),
                system_prompt: "You are an implementation sub-agent. Write clean, working code."
                    .to_string(),
                max_turns: 10,
            },
            SpawnSpec {
                description: "Testing subtask".to_string(),
                prompt: format!("Write tests for:\n{}", safe_input),
                system_prompt: "You are a testing sub-agent. Ensure comprehensive test coverage."
                    .to_string(),
                max_turns: 5,
            },
        ];

        WorkflowAction::SpawnParallel(specs)
    }

    fn on_turn_complete(
        &self,
        _response: &str,
        metadata: &HashMap<String, String>,
    ) -> WorkflowAction {
        // If we already spawned, mark as complete
        if metadata.contains_key("ultrawork_spawned") {
            return WorkflowAction::Complete(
                "Parallel execution complete. Results aggregated.".to_string(),
            );
        }
        WorkflowAction::Continue
    }
}
