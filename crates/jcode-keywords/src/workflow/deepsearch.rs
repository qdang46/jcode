//! Deepsearch — CodebaseSearch workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns parallel search agents with different strategies.

use super::{SpawnSpec, WorkflowAction, WorkflowContext, WorkflowHandler, sanitize_user_input};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct DeepsearchHandler;

impl WorkflowHandler for DeepsearchHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Deepsearch
    }

    fn build_prompt(&self) -> String {
        "# $deepsearch — Codebase Search Mode\n\n\
         Use multiple search strategies.\n\n\
         ## Strategies\n\
         1. Text/Regex: grep for keywords, patterns\n\
         2. Structural: find functions, types, modules\n\
         3. Semantic: find related concepts, similar code\n\n\
         ## Output\n\
         Context Map: file:line — Description\n\
         Summary: How found code relates to query"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        // Guard: don't re-spawn if already spawned
        if ctx.metadata.contains_key("deepsearch_spawned") {
            return WorkflowAction::Continue;
        }

        let safe_input = sanitize_user_input(ctx.user_input);
        let specs = vec![
            SpawnSpec {
                description: "Text/regex search".to_string(),
                prompt: format!("Search the codebase for text patterns related to:\n{}\n\nReport file:line matches.", safe_input),
                system_prompt: "You are a text search agent. Use file_grep tool extensively. Report results as file:line:content.".to_string(),
                max_turns: 5,
            },
            SpawnSpec {
                description: "Structural search".to_string(),
                prompt: format!("Search for structural elements (functions, types, modules) related to:\n{}", safe_input),
                system_prompt: "You are a structural search agent. Find code structures — function signatures, type definitions, module structure.".to_string(),
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
        if metadata.contains_key("deepsearch_spawned") {
            return WorkflowAction::Complete(
                "Codebase search complete. Context map generated.".to_string(),
            );
        }
        WorkflowAction::Continue
    }
}
