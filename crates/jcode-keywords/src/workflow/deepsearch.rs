//! Deepsearch — CodebaseSearch workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct DeepsearchHandler;

impl WorkflowHandler for DeepsearchHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Deepsearch
    }

    fn build_prompt(&self) -> String {
        "# $deepsearch — Codebase Search Mode\n\n\
         You are in deepsearch mode. Use multiple search strategies \
         to find relevant code.\n\n\
         Strategy:\n\
         1. Text/regex search for keywords and patterns\n\
         2. Structural search (functions, types, modules)\n\
         3. Semantic search (related concepts, similar code)\n\
         4. Build a context map of relevant locations\n\
         5. Summarize findings with file:line references"
            .to_string()
    }
}
