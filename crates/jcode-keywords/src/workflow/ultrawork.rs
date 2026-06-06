//! Ultrawork — ParallelExecution workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns parallel sub-agents for independent subtasks.

use super::SpawnSpec;
use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct UltraworkHandler;

impl WorkflowHandler for UltraworkHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultrawork
    }

    fn build_prompt(&self) -> String {
        "# $ultrawork — Parallel Execution Mode\n\n\
         You are in ultrawork mode. Execute the task using parallel sub-agents.\n\n\
         ## Strategy\n\
         1. **Analyze** — Break the task into independent subtasks\n\
         2. **Spawn** — Launch up to 4 parallel sub-agents\n\
         3. **Coordinate** — Monitor progress, handle dependencies\n\
         4. **Retry** — Failed subtasks get up to 3 retries\n\
         5. **Aggregate** — Combine results into unified response\n\n\
         ## Rules\n\
         - Each subtask must be truly independent\n\
         - If a subtask depends on another, run them sequentially\n\
         - Report progress: 'Running 4 sub-agents...'\n\
         - On completion: 'All sub-agents complete (4/4)'"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        // Check if we already have subtask results from a previous turn
        if let Some(results) = ctx.metadata.get("ultrawork_results") {
            return WorkflowAction::Complete(format!(
                "Parallel execution complete.\n\n{}",
                results
            ));
        }

        // Spawn parallel sub-agents for the task
        let task = &ctx.user_input;
        let specs = vec![
            SpawnSpec {
                description: "Analysis subtask".to_string(),
                prompt: format!("Analyze the following task and identify key components:\n{}", task),
                system_prompt: "You are an analysis sub-agent. Focus on understanding the task structure and identifying independent components.".to_string(),
                max_turns: 5,
            },
            SpawnSpec {
                description: "Implementation subtask".to_string(),
                prompt: format!("Implement the core functionality for:\n{}", task),
                system_prompt: "You are an implementation sub-agent. Focus on writing clean, working code.".to_string(),
                max_turns: 10,
            },
            SpawnSpec {
                description: "Testing subtask".to_string(),
                prompt: format!("Write tests for the following task:\n{}", task),
                system_prompt: "You are a testing sub-agent. Focus on comprehensive test coverage.".to_string(),
                max_turns: 5,
            },
            SpawnSpec {
                description: "Documentation subtask".to_string(),
                prompt: format!("Write documentation for:\n{}", task),
                system_prompt: "You are a documentation sub-agent. Focus on clear, concise docs.".to_string(),
                max_turns: 5,
            },
        ];

        WorkflowAction::SpawnParallel(specs)
    }

    fn on_turn_complete(&self, response: &str, metadata: &HashMap<String, String>) -> WorkflowAction {
        // If we got sub-agent results, aggregate and complete
        if metadata.contains_key("ultrawork_results") || response.contains("sub-agent") {
            WorkflowAction::Complete("Parallel execution complete. Results aggregated.".to_string())
        } else {
            // First turn: let the LLM work, then we'll spawn sub-agents
            WorkflowAction::Continue
        }
    }
}
