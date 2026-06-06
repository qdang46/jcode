//! SecurityReview — workflow handler.

use super::WorkflowHandler;
use crate::registry::WorkflowKind;

pub struct SecurityReviewHandler;

impl WorkflowHandler for SecurityReviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::SecurityReview
    }

    fn build_prompt(&self) -> String {
        "# $security-review — Security Review Mode\n\n\
         You are in security review mode. Perform a thorough security audit.\n\n\
         Strategy:\n\
         1. OWASP Top 10 scan\n\
         2. Check for hardcoded secrets/credentials\n\
         3. Verify input validation and sanitization\n\
         4. Check for SQL injection, XSS, CSRF vulnerabilities\n\
         5. Review authentication and authorization\n\
         6. Report findings ranked by severity (Critical/High/Medium/Low)"
            .to_string()
    }
}
