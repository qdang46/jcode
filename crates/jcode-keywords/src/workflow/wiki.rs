//! Wiki — DocLookup workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct WikiHandler;

impl WorkflowHandler for WikiHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Wiki
    }

    fn build_prompt(&self) -> String {
        "# $wiki — Documentation Lookup Mode\n\n\
         You are in wiki mode. Search documentation sources.\n\n\
         Strategy:\n\
         1. Search local docs (README, AGENTS.md, docs/)\n\
         2. Search code comments and docstrings\n\
         3. Search web documentation if needed\n\
         4. Summarize findings with source references\n\
         5. Provide actionable context"
            .to_string()
    }
}
