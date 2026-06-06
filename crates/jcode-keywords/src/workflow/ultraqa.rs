//! Ultraqa — QACycling workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct UltraqaHandler;

impl WorkflowHandler for UltraqaHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultraqa
    }

    fn build_prompt(&self) -> String {
        "# $ultraqa — QA Cycling Mode\n\n\
         You are in ultraqa mode. Run QA cycles: implement → test → fix → repeat \
         until all tests pass. Maximum 5 iterations.\n\n\
         Strategy:\n\
         1. Implement the requested change\n\
         2. Run relevant tests\n\
         3. If tests fail, analyze failures and fix\n\
         4. Repeat until all tests pass or max 5 iterations\n\
         5. Report final status with pass/fail counts"
            .to_string()
    }
}
