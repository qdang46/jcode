//! Tdd — TestDrivenDev workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct TddHandler;

impl WorkflowHandler for TddHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Tdd
    }

    fn build_prompt(&self) -> String {
        "# $tdd — Test-Driven Development Mode\n\n\
         You are in TDD mode. Follow the Red → Green → Refactor cycle.\n\n\
         Strategy:\n\
         1. RED: Write a failing test that describes the desired behavior\n\
         2. GREEN: Write the minimal code to make the test pass\n\
         3. REFACTOR: Clean up the code while keeping tests green\n\
         4. Repeat for each behavior\n\
         5. Report test coverage at the end"
            .to_string()
    }
}
