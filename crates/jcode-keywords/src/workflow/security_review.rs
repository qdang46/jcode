//! SecurityReview — workflow handler.
//!
//! Tier 2: Sub-agent spawning. Spawns a security auditor agent.

use super::SpawnSpec;
use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct SecurityReviewHandler;

impl WorkflowHandler for SecurityReviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::SecurityReview
    }

    fn build_prompt(&self) -> String {
        "# $security-review — Security Review Mode\n\n\
         You are in security review mode. Perform comprehensive security audit.\n\n\
         ## OWASP Top 10 Checklist\n\
         1. **A01: Broken Access Control** — Authorization bypass, IDOR\n\
         2. **A02: Cryptographic Failures** — Weak crypto, plaintext secrets\n\
         3. **A03: Injection** — SQL, XSS, command injection\n\
         4. **A04: Insecure Design** — Missing threat modeling\n\
         5. **A05: Security Misconfiguration** — Default creds, debug mode\n\
         6. **A06: Vulnerable Components** — Outdated dependencies\n\
         7. **A07: Auth Failures** — Weak passwords, missing MFA\n\
         8. **A08: Data Integrity** — Deserialization, CI/CD pipeline\n\
         9. **A09: Logging Failures** — Missing audit logs\n\
         10. **A10: SSRF** — Server-side request forgery\n\n\
         ## Additional Checks\n\
         - Hardcoded secrets, API keys, tokens\n\
         - SQL injection in queries\n\
         - XSS in user-facing output\n\
         - CSRF in state-changing operations\n\
         - Path traversal in file operations\n\n\
         ## Output Format\n\
         ### Risk Summary\n\
         Critical / High / Medium / Low findings count\n\n\
         ### Findings\n\
         - **Severity**: Critical / High / Medium / Low\n\
         - **Category**: OWASP category\n\
         - **Location**: file:line\n\
         - **Description**: What's wrong\n\
         - **Remediation**: How to fix"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let spec = SpawnSpec {
            description: "Security auditor".to_string(),
            prompt: format!(
                "Perform a security audit on the following:\n\n{}\n\n\
                 Check for OWASP Top 10 vulnerabilities, hardcoded secrets, \
                 and common security issues. Provide severity ratings.",
                ctx.user_input
            ),
            system_prompt: "You are a security auditor. Be paranoid. Check for every \
                           possible vulnerability. Rate findings by OWASP severity."
                .to_string(),
            max_turns: 10,
        };

        WorkflowAction::SpawnAgent {
            description: spec.description.clone(),
            prompt: spec.prompt.clone(),
            system_prompt: spec.system_prompt.clone(),
            max_turns: spec.max_turns,
        }
    }

    fn on_turn_complete(&self, _response: &str, _metadata: &HashMap<String, String>) -> WorkflowAction {
        WorkflowAction::Complete("Security review complete.".to_string())
    }
}
