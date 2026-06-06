//! Analyze — DeepAnalysis workflow handler.
//!
//! Tier 1: Prompt-only. Injects structured analysis instructions.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct AnalyzeHandler;

impl WorkflowHandler for AnalyzeHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Analyze
    }

    fn build_prompt(&self) -> String {
        "# $analyze — Deep Analysis Mode\n\n\
         You are in analyze mode. Perform structured, thorough analysis.\n\n\
         ## Strategy\n\
         1. **Scope** — Identify what to analyze (file, module, system, concept)\n\
         2. **Structure** — Map the architecture, dependencies, data flow\n\
         3. **Patterns** — Identify design patterns, anti-patterns, conventions\n\
         4. **Complexity** — Assess cognitive complexity, cyclomatic complexity\n\
         5. **Quality** — Check error handling, testing, documentation\n\
         6. **Improvements** — Generate ranked recommendations with rationale\n\n\
         ## Output Format\n\
         ### Summary\n\
         One-paragraph overview of findings.\n\n\
         ### Detailed Findings\n\
         For each finding:\n\
         - **Finding**: Description\n\
         - **Impact**: Low/Medium/High/Critical\n\
         - **Location**: file:line references\n\
         - **Recommendation**: Specific action to take\n\n\
         ### Priority Actions\n\
         Top 3 things to address first."
            .to_string()
    }

    fn execute(&self, _ctx: &WorkflowContext) -> WorkflowAction {
        WorkflowAction::Continue
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        WorkflowAction::Complete("Analysis complete.".to_string())
    }
}
