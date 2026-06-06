//! DeepInterview — RequirementsGathering workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct DeepInterviewHandler;

impl WorkflowHandler for DeepInterviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::DeepInterview
    }

    fn build_prompt(&self) -> String {
        "# $deep-interview — Requirements Gathering Mode\n\n\
         You are in deep-interview mode. Ask clarifying questions to gather \
         requirements. Score ambiguity on a 1-10 scale. Continue until \
         ambiguity < 3.\n\n\
         Strategy:\n\
         1. Analyze the request for ambiguity\n\
         2. Ask targeted clarifying questions (max 3 per round)\n\
         3. Score remaining ambiguity 1-10\n\
         4. If ambiguity >= 3, ask another round\n\
         5. Once ambiguity < 3, summarize requirements and proceed"
            .to_string()
    }
}
