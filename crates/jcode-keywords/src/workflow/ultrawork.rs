//! Ultrawork — ParallelExecution workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct UltraworkHandler;

impl WorkflowHandler for UltraworkHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultrawork
    }

    fn build_prompt(&self) -> String {
        "# $ultrawork — Parallel Execution Mode\n\n\
         You are in ultrawork mode. Break the task into independent subtasks \
         and execute them in parallel using sub-agents. Coordinate results, \
         handle failures with retries (max 3), and aggregate into a unified response.\n\n\
         Strategy:\n\
         1. Analyze the task and identify independent subtasks\n\
         2. Spawn sub-agents for each subtask (up to 4 concurrent)\n\
         3. Collect results as they complete\n\
         4. Retry failed subtasks up to 3 times\n\
         5. Aggregate all results into a coherent response\n\
         6. Report completion status with summary"
            .to_string()
    }
}
