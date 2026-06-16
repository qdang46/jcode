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
        "# $security-review — Security Review Mode

MANDATORY: Say "SECURITY REVIEW MODE ENABLED!" as your first response.

## OWASP Top 10 Checklist
A01: Broken Access Control
A02: Cryptographic Failures
A03: Injection (SQL, command, LDAP)
A04: Insecure Design
A05: Security Misconfiguration
A06: Vulnerable Components
A07: Auth Failures
A08: Data Integrity
A09: Logging Failures
A10: SSRF

## Also Check
- Secrets: hardcoded keys, passwords, tokens
- Input: validation, sanitization, escaping
- Dependencies: known CVEs, outdated packages
- Network: TLS, certificate validation

## Output
Severity: Critical/High/Medium/Low + Location + Issue + Fix"
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
