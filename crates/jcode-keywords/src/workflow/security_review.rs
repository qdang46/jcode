//! SecurityReview — workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns a security auditor agent.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler, sanitize_user_input};
use crate::registry::WorkflowKind;

pub struct SecurityReviewHandler;

impl WorkflowHandler for SecurityReviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::SecurityReview
    }

    fn build_prompt(&self) -> String {
        "# $security-review — Security Review Mode\n\n\
         Perform comprehensive security audit.\n\n\
         ## OWASP Top 10\n\
         A01: Broken Access Control\n\
         A02: Cryptographic Failures\n\
         A03: Injection\n\
         A04: Insecure Design\n\
         A05: Security Misconfiguration\n\
         A06: Vulnerable Components\n\
         A07: Auth Failures\n\
         A08: Data Integrity\n\
         A09: Logging Failures\n\
         A10: SSRF\n\n\
         ## Also Check\n\
         - Hardcoded secrets/keys/tokens\n\
         - SQL injection, XSS, CSRF\n\
         - Path traversal\n\n\
         ## Output\n\
         Risk Summary: Critical/High/Medium/Low counts\n\
         Findings: Severity + OWASP Category + Location + Remediation"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let safe_input = sanitize_user_input(ctx.user_input);
        WorkflowAction::SpawnAgent {
            description: "Security auditor".to_string(),
            prompt: format!(
                "Perform a security audit on:\n\n{}\n\n\
                 Check for OWASP Top 10 vulnerabilities, hardcoded secrets, \
                 and common security issues. Provide severity ratings.",
                safe_input
            ),
            system_prompt: "You are a security auditor. Be paranoid. Check for every \
                           possible vulnerability. Rate findings by OWASP severity."
                .to_string(),
            max_turns: 10,
        }
    }
}
