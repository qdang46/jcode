//! Analyze — DeepAnalysis workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct AnalyzeHandler;

impl WorkflowHandler for AnalyzeHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Analyze
    }

    fn build_prompt(&self) -> String {
        "# $analyze — Deep Analysis Mode\n\n\
         You are in analyze mode. Perform structured analysis.\n\n\
         Strategy:\n\
         1. Examine code structure and architecture\n\
         2. Identify patterns and anti-patterns\n\
         3. Assess complexity and maintainability\n\
         4. Find improvement opportunities\n\
         5. Provide ranked recommendations with rationale"
            .to_string()
    }
}
