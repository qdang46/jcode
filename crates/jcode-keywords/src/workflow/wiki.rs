//! Wiki — DocLookup workflow handler.
//!
//! Tier 1: Prompt-only. Injects documentation search instructions.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct WikiHandler;

impl WorkflowHandler for WikiHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Wiki
    }

    fn build_prompt(&self) -> String {
        "# $wiki — Documentation Lookup Mode\n\n\
         You are in wiki mode. Search and synthesize documentation.\n\n\
         ## Search Strategy\n\
         1. **Local docs** — README.md, AGENTS.md, docs/, .jcode/\n\
         2. **Code docs** — Docstrings, comments, rustdoc\n\
         3. **Config files** — Cargo.toml, package.json, config files\n\
         4. **Web docs** — Official documentation, API references\n\
         5. **Cross-reference** — Verify information across multiple sources\n\n\
         ## Output Format\n\
         ### Answer\n\
         Direct answer to the question.\n\n\
         ### Sources\n\
         - file:line references for local sources\n\
         - URLs for web sources\n\n\
         ### Related\n\
         - Links to related documentation\n\
         - Common pitfalls or gotchas"
            .to_string()
    }

    fn execute(&self, _ctx: &WorkflowContext) -> WorkflowAction {
        WorkflowAction::Continue
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        WorkflowAction::Complete("Documentation lookup complete.".to_string())
    }
}
