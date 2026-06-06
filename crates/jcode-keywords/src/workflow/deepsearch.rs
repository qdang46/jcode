//! Deepsearch — CodebaseSearch workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns parallel search agents with different strategies.

use super::SpawnSpec;
use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct DeepsearchHandler;

impl WorkflowHandler for DeepsearchHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Deepsearch
    }

    fn build_prompt(&self) -> String {
        "# $deepsearch — Codebase Search Mode\n\n\
         You are in deepsearch mode. Use multiple search strategies.\n\n\
         ## Search Strategies\n\
         1. **Text/Regex** — Grep for keywords, patterns, strings\n\
         2. **Structural** — Find functions, types, modules by name\n\
         3. **Semantic** — Find related concepts, similar code patterns\n\
         4. **Dependency** — Trace imports, usages, call chains\n\n\
         ## Output Format\n\
         ### Context Map\n\
         ```\n\
         file:line — Description\n\
         file:line — Description\n\
         ```\n\n\
         ### Summary\n\
         How the found code relates to the search query.\n\n\
         ### Related Locations\n\
         Other files that might be relevant."
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let query = &ctx.user_input;
        let specs = vec![
            SpawnSpec {
                description: "Text/regex search".to_string(),
                prompt: format!(
                    "Search the codebase for text patterns related to:\n{}\n\n\
                     Use grep, ripgrep, or similar tools. Report file:line matches.",
                    query
                ),
                system_prompt: "You are a text search agent. Find all textual matches. \
                               Use file_grep tool extensively. Report results as file:line:content."
                    .to_string(),
                max_turns: 5,
            },
            SpawnSpec {
                description: "Structural search".to_string(),
                prompt: format!(
                    "Search the codebase for structural elements (functions, types, modules) \
                     related to:\n{}\n\n\
                     Look for definitions, implementations, and usages.",
                    query
                ),
                system_prompt: "You are a structural search agent. Find code structures. \
                               Look at function signatures, type definitions, module structure."
                    .to_string(),
                max_turns: 5,
            },
            SpawnSpec {
                description: "Semantic search".to_string(),
                prompt: format!(
                    "Search the codebase for semantically related code to:\n{}\n\n\
                     Look for similar patterns, related concepts, analogous implementations.",
                    query
                ),
                system_prompt: "You are a semantic search agent. Find code by meaning, \
                               not just keywords. Look for similar patterns and related concepts."
                    .to_string(),
                max_turns: 5,
            },
        ];

        WorkflowAction::SpawnParallel(specs)
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        WorkflowAction::Complete("Codebase search complete. Context map generated.".to_string())
    }
}
