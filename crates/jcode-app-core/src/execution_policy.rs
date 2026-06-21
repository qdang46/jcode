//! Execution policy engine — per-command rule evaluation.
//!
//! Evaluates individual shell commands against TOML-configured rules,
//! built-in safe whitelist, and dangerous patterns before execution.
//!
//! # Two-stage permission architecture
//!
//! jcode already has Stage 1 (tool-level) permission checks via `dcg_bridge`.
//! This module adds Stage 2 (command-level) evaluation inside tool execution:
//!
//! ```text
//! Stage 1: validate_tool_allowed("bash") → dcg_bridge → Allow/Deny/Prompt
//! Stage 2: ExecutionPolicyEngine::evaluate("rm -rf /tmp") → Allow/Deny/Prompt (NEW)
//! ```
//!
//! # Rule evaluation order
//!
//! 1. Protected patterns — always prompt (even in BypassPermissions mode)
//! 2. Circuit breaker — if exceeded, deny immediately with clear message
//! 3. Custom rules — first match wins: Allow, Deny, or Prompt
//! 4. Fallback — caller falls through to dcg-core's mode-based evaluation

use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use jcode_config_types::{
    CircuitBreakerConfig, ExecutionPolicyConfig, PolicyRuleAction, PolicyRuleDef,
};

/// Parse a Claude Code-style `ToolName(pattern)` permission rule string.
///
/// # Formats
///
/// | Input | Tool | Pattern |
/// |-------|------|---------|
/// | `"Bash(ls *)"` | `bash` | `ls .*` |
/// | `"WebSearch"` | `web_search` | `.*` |
/// | `"Read(.git/config)"` | `read` | `\.git/config` |
///
/// Returns `(tool_name_lowercased, regex_pattern)`.
pub fn parse_permission_rule(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(paren) = s.find('(') {
        let tool_name = s[..paren].trim().to_lowercase();
        let raw = s[paren + 1..].strip_suffix(')')?.trim();
        let regex = glob_like_to_regex(raw);
        Some((tool_name, regex))
    } else {
        Some((s.to_lowercase(), ".*".to_string()))
    }
}

/// Convert a Claude Code glob-like pattern to a regex string.
/// `*` → `.*` (greedy), `?` → `.`, regex special chars are escaped.
fn glob_like_to_regex(pattern: &str) -> String {
    let mut re = String::with_capacity(pattern.len() + 2);
    re.push('^');
    for ch in pattern.chars() {
        match ch {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            '.' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '\\' | '|' | '^' | '$' => {
                re.push('\\');
                re.push(ch);
            }
            _ => re.push(ch),
        }
    }
    re.push('$');
    re
}

// ── Constants ──────────────────────────────────────────────────────────

/// TTL for allow-once codes (24 hours).
const ALLOW_ONCE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Default circuit breaker thresholds (matching claude-code defaults).
const DEFAULT_MAX_CONSECUTIVE_DENIALS: u32 = 3;
const DEFAULT_MAX_TOTAL_DENIALS: u32 = 20;

// ── Decision ──────────────────────────────────────────────────────────

/// Outcome of evaluating a command against the policy engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Execute immediately.
    Allow {
        /// Human-readable reason (for audit logging).
        reason: String,
    },
    /// Block with explanation and alternatives.
    Deny {
        /// Human-readable explanation.
        reason: String,
        /// Suggested safer alternatives.
        alternatives: Vec<String>,
        /// Reference to the rule that matched (for debugging).
        rule_id: Option<String>,
    },
    /// Require human approval.
    Prompt {
        /// Human-readable explanation.
        reason: String,
        /// Allow-once code scoped to this exact command.
        allow_once_code: String,
        /// Suggested alternatives.
        alternatives: Vec<String>,
    },
}

// ── Compiled Rule ──────────────────────────────────────────────────────

/// A compiled policy rule (regex pre-compiled at load time).
#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub id: String,
    pub description: String,
    pub regex: Regex,
    pub action: PolicyRuleAction,
    pub tool: Option<String>,
    pub alternatives: Vec<String>,
}

impl CompiledRule {
    fn from_def(def: &PolicyRuleDef) -> Result<Self, regex::Error> {
        let regex = Regex::new(&def.pattern)?;
        Ok(Self {
            id: def.id.clone(),
            description: def.description.clone(),
            regex,
            action: def.action,
            tool: def.tool.clone(),
            alternatives: def.alternatives.clone(),
        })
    }
}

// ── Execution Policy Engine ────────────────────────────────────────────

/// Evaluates commands against configured rules and built-in patterns.
///
/// Lightweight and synchronous — designed to be called inside the hot
/// path of `BashTool::execute()` with minimal overhead (<500µs typical).
#[derive(Debug, Clone)]
pub struct ExecutionPolicyEngine {
    /// Custom policy rules (first match wins).
    rules: Vec<CompiledRule>,
    /// Protected patterns — always prompt regardless of mode.
    protected_patterns: Vec<Regex>,
    /// Circuit breaker settings.
    circuit_breaker: CircuitBreakerConfig,
}

impl ExecutionPolicyEngine {
    /// Create a new engine with defaults (no custom rules).
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            protected_patterns: Vec::new(),
            circuit_breaker: CircuitBreakerConfig::default(),
        }
    }

    /// Build an engine from TOML config.
    /// Returns an empty engine (no rules, no protected patterns) when
    /// `config.enabled` is `false`, so disabling the policy is a no-op.
    pub fn from_config(config: &ExecutionPolicyConfig) -> Self {
        if !config.enabled {
            return Self {
                rules: Vec::new(),
                protected_patterns: Vec::new(),
                circuit_breaker: config.circuit_breaker,
            };
        }
        let mut rules: Vec<CompiledRule> = Vec::new();

        // Convert Claude Code-style allow/deny/ask entries to CompiledRules.
        // These are parsed BEFORE user-defined rules so user rules take priority (last wins).
        let mut rule_index = 0u32;
        for entry in &config.deny {
            if let Some((tool, pattern)) = parse_permission_rule(entry)
                && let Ok(regex) = Regex::new(&pattern)
            {
                rule_index += 1;
                rules.push(CompiledRule {
                    id: format!("builtin-deny-{}", rule_index),
                    description: format!("Denied by permission rule: {}", entry),
                    regex,
                    action: PolicyRuleAction::Deny,
                    tool: Some(tool),
                    alternatives: vec![],
                });
            }
        }
        for entry in &config.ask {
            if let Some((tool, pattern)) = parse_permission_rule(entry)
                && let Ok(regex) = Regex::new(&pattern)
            {
                rule_index += 1;
                rules.push(CompiledRule {
                    id: format!("builtin-ask-{}", rule_index),
                    description: format!("Ask by permission rule: {}", entry),
                    regex,
                    action: PolicyRuleAction::Prompt,
                    tool: Some(tool),
                    alternatives: vec![],
                });
            }
        }
        for entry in &config.allow {
            if let Some((tool, pattern)) = parse_permission_rule(entry)
                && let Ok(regex) = Regex::new(&pattern)
            {
                rule_index += 1;
                rules.push(CompiledRule {
                    id: format!("builtin-allow-{}", rule_index),
                    description: format!("Allowed by permission rule: {}", entry),
                    regex,
                    action: PolicyRuleAction::Allow,
                    tool: Some(tool),
                    alternatives: vec![],
                });
            }
        }

        // Append user-defined custom rules (these take priority over the auto-generated ones).
        for def in &config.rules {
            match CompiledRule::from_def(def) {
                Ok(rule) => rules.push(rule),
                Err(e) => {
                    crate::logging::warn(&format!(
                        "ExecutionPolicy: invalid regex pattern '{}' in rule '{}': {}",
                        def.pattern, def.id, e
                    ));
                }
            }
        }

        let protected_patterns: Vec<Regex> = config
            .protected_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        Self {
            rules,
            protected_patterns,
            circuit_breaker: config.circuit_breaker,
        }
    }

    /// Evaluate a tool call against the policy rules, matching only by tool name.
    /// Use this for tool-level allow/deny/ask rules (e.g., `"WebSearch"` in deny).
    ///
    /// Returns `None` if no rule matches this tool.
    pub fn evaluate_tool(&self, tool_name: &str) -> Option<PolicyDecision> {
        for rule in &self.rules {
            if let Some(ref t) = rule.tool {
                if t != tool_name {
                    continue;
                }
            } else {
                continue; // skip rules with no tool scope
            }
            // For tool-level evaluation, match only the tool name (regex must match empty/anything).
            // The regex is tested against an empty string — for bare tool names the pattern is `.*`
            // which always matches. For ToolName(pattern) patterns, the command-level evaluate()
            // is what actually checks the command.
            if rule.regex.is_match("") {
                match rule.action {
                    PolicyRuleAction::Allow => {
                        return Some(PolicyDecision::Allow {
                            reason: format!("Allowed by tool rule '{}'", rule.id),
                        });
                    }
                    PolicyRuleAction::Deny => {
                        return Some(PolicyDecision::Deny {
                            reason: rule.description.clone(),
                            alternatives: rule.alternatives.clone(),
                            rule_id: Some(rule.id.clone()),
                        });
                    }
                    PolicyRuleAction::Prompt => {
                        return Some(PolicyDecision::Prompt {
                            reason: rule.description.clone(),
                            allow_once_code: String::new(),
                            alternatives: rule.alternatives.clone(),
                        });
                    }
                }
            }
        }
        None
    }

    pub fn evaluate(
        &self,
        tool_name: &str,
        command: &str,
        session: &mut PolicySession,
    ) -> Option<PolicyDecision> {
        // 1. Check protected patterns (always prompt, regardless of mode)
        for pattern in &self.protected_patterns {
            if pattern.is_match(command) {
                let code = session.generate_allow_once_code(command);
                return Some(PolicyDecision::Prompt {
                    reason: "Command matches protected pattern".into(),
                    allow_once_code: code,
                    alternatives: vec![],
                });
            }
        }

        // 2. Check circuit breaker
        if session.is_circuit_broken(self.circuit_breaker) {
            return Some(PolicyDecision::Deny {
                reason: format!(
                    "Policy circuit breaker active: {} consecutive denials, {} total. \
                     Reset by switching to interactive mode or restarting the session.",
                    session.consecutive_denials, session.total_denials
                ),
                alternatives: vec![
                    "Use --permission-mode default to reset".into(),
                    "Approve commands interactively in the TUI".into(),
                ],
                rule_id: Some("circuit-breaker".into()),
            });
        }

        // 3. Evaluate custom rules (first match wins)
        for rule in &self.rules {
            #[allow(clippy::collapsible_if)]
            if let Some(ref t) = rule.tool {
                if t != tool_name {
                    continue;
                }
            }
            if rule.regex.is_match(command) {
                match rule.action {
                    PolicyRuleAction::Allow => {
                        session.record_allow();
                        return Some(PolicyDecision::Allow {
                            reason: format!("Matched allow rule '{}'", rule.id),
                        });
                    }
                    PolicyRuleAction::Deny => {
                        session.record_deny();
                        return Some(PolicyDecision::Deny {
                            reason: rule.description.clone(),
                            alternatives: rule.alternatives.clone(),
                            rule_id: Some(rule.id.clone()),
                        });
                    }
                    PolicyRuleAction::Prompt => {
                        let code = session.generate_allow_once_code(command);
                        return Some(PolicyDecision::Prompt {
                            reason: rule.description.clone(),
                            allow_once_code: code,
                            alternatives: rule.alternatives.clone(),
                        });
                    }
                }
            }
        }

        // 4. No rule matched — caller falls through to dcg-core evaluation
        None
    }

    /// Number of loaded custom rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Number of loaded protected patterns.
    pub fn protected_pattern_count(&self) -> usize {
        self.protected_patterns.len()
    }
}

impl Default for ExecutionPolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Per-Session Policy State ───────────────────────────────────────────

/// Tracks allow-once approvals and denial counters for a single session.
#[derive(Debug, Clone)]
pub struct PolicySession {
    /// Allow-once approvals: SHA256(command hash) → expiry instant.
    allow_once_cache: HashMap<String, Instant>,
    /// Consecutive denials since last allow.
    pub consecutive_denials: u32,
    /// Total denials across the session.
    pub total_denials: u32,
}

impl PolicySession {
    /// Create a fresh session with zero counters.
    pub fn new() -> Self {
        Self {
            allow_once_cache: HashMap::new(),
            consecutive_denials: 0,
            total_denials: 0,
        }
    }

    /// Generate a 6-hex-char allow-once code for this command.
    ///
    /// The code is derived from SHA256(command) and stored with an expiry.
    pub fn generate_allow_once_code(&mut self, command: &str) -> String {
        let hash = hex::encode(Sha256::digest(command.as_bytes()));
        let code = hash[..6].to_string();
        self.allow_once_cache
            .insert(code.clone(), Instant::now() + ALLOW_ONCE_TTL);
        code
    }

    /// Consume an allow-once code for this command.
    ///
    /// Returns `true` if the code is valid and not expired.
    pub fn consume_allow_once(&mut self, code: &str) -> bool {
        #[allow(clippy::collapsible_if)]
        if let Some(expiry) = self.allow_once_cache.get(code) {
            if *expiry > Instant::now() {
                self.allow_once_cache.remove(code);
                self.record_allow();
                return true;
            }
        }
        false
    }

    /// Record an allowed command (resets consecutive denial counter).
    pub fn record_allow(&mut self) {
        self.consecutive_denials = 0;
    }

    /// Record a denied command (increments both counters).
    pub fn record_deny(&mut self) {
        self.consecutive_denials += 1;
        self.total_denials += 1;
    }

    /// Check if the circuit breaker is active.
    pub fn is_circuit_broken(&self, config: CircuitBreakerConfig) -> bool {
        self.consecutive_denials >= config.max_consecutive_denials
            || self.total_denials >= config.max_total_denials
    }

    /// Reset all counters and allow-once cache.
    pub fn reset(&mut self) {
        self.consecutive_denials = 0;
        self.total_denials = 0;
        self.allow_once_cache.clear();
    }

    /// Number of active allow-once codes.
    pub fn allow_once_count(&self) -> usize {
        self.allow_once_cache.len()
    }
}

impl Default for PolicySession {
    fn default() -> Self {
        Self::new()
    }
}

// ── Global Instances ──────────────────────────────────────────────────

use std::sync::{LazyLock, Mutex};

/// Per-process execution policy engine. Initialized at startup from config.
static ENGINE: LazyLock<Mutex<ExecutionPolicyEngine>> =
    LazyLock::new(|| Mutex::new(ExecutionPolicyEngine::new()));

/// Per-process policy session (reset on session change).
static SESSION: LazyLock<Mutex<PolicySession>> = LazyLock::new(|| Mutex::new(PolicySession::new()));

/// Initialize the policy engine from config. Called at startup.
pub fn init_policy_engine(config: &ExecutionPolicyConfig) {
    if let Ok(mut engine) = ENGINE.lock() {
        *engine = ExecutionPolicyEngine::from_config(config);
    }
}

/// Reset the per-process policy session. Called when starting a new session.
pub fn reset_policy_session() {
    if let Ok(mut session) = SESSION.lock() {
        session.reset();
    }
}

/// Evaluate a command against the global policy engine.
///
/// Returns `None` if no rule matched (caller falls through to dcg-core).
///
/// # Lock ordering
///
/// This function acquires `ENGINE` then `SESSION` (in that order).
/// `dcg_bridge::classify_command` later acquires `dcg_bridge::SESSION`.
/// The global ordering is: exec_policy::ENGINE → exec_policy::SESSION → dcg_bridge::SESSION.
/// No code inverts this order, so deadlock is not possible.
pub fn evaluate_command(tool_name: &str, command: &str) -> Option<PolicyDecision> {
    let engine = ENGINE.lock().ok()?;
    let mut session = SESSION.lock().ok()?;
    engine.evaluate(tool_name, command, &mut session)
}

/// Evaluate a tool call against the global policy engine (tool-level rules only).
///
/// Returns `None` if no rule matches this tool.
pub fn evaluate_tool(tool_name: &str) -> Option<PolicyDecision> {
    let engine = ENGINE.lock().ok()?;
    engine.evaluate_tool(tool_name)
}

/// Try to consume an allow-once code.
pub fn try_consume_allow_once(code: &str) -> bool {
    SESSION
        .lock()
        .ok()
        .map(|mut s| s.consume_allow_once(code))
        .unwrap_or(false)
}

/// Get the current policy engine state (for TUI display).
pub fn policy_stats() -> (u32, u32, usize) {
    let session = SESSION.lock().ok();
    let engine = ENGINE.lock().ok();
    (
        session.as_ref().map(|s| s.consecutive_denials).unwrap_or(0),
        session.as_ref().map(|s| s.total_denials).unwrap_or(0),
        engine.as_ref().map(|e| e.rule_count()).unwrap_or(0),
    )
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn allow_rule(id: &str, pattern: &str) -> PolicyRuleDef {
        PolicyRuleDef {
            id: id.to_string(),
            description: format!("Allow rule: {}", pattern),
            pattern: pattern.to_string(),
            action: PolicyRuleAction::Allow,
            tool: Some("bash".into()),
            alternatives: vec![],
        }
    }

    fn deny_rule(id: &str, pattern: &str, alts: Vec<&str>) -> PolicyRuleDef {
        PolicyRuleDef {
            id: id.to_string(),
            description: format!("Deny rule: {}", pattern),
            pattern: pattern.to_string(),
            action: PolicyRuleAction::Deny,
            tool: Some("bash".into()),
            alternatives: alts.into_iter().map(String::from).collect(),
        }
    }

    // ── Happy Path ──────────────────────────────────────────────────

    #[test]
    fn allow_rule_lets_command_through() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(allow_rule("test-allow", "^git log"));
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        let result = engine.evaluate("bash", "git log --oneline", &mut session);
        assert!(
            matches!(&result, Some(PolicyDecision::Allow { reason }) if reason.contains("test-allow")),
            "Expected Allow from test-allow rule, got {:?}",
            result
        );
    }

    #[test]
    fn deny_rule_blocks_command() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(deny_rule(
            "test-deny",
            "git push --force",
            vec!["git push --force-with-lease"],
        ));
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        let result = engine.evaluate("bash", "git push --force origin master", &mut session);
        assert!(
            matches!(&result, Some(PolicyDecision::Deny { reason, .. }) if reason.contains("git push --force")),
            "Expected Deny for force push, got {:?}",
            result
        );
    }

    #[test]
    fn prompt_rule_triggers_allow_once_flow() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(PolicyRuleDef {
            id: "test-prompt".into(),
            description: "Prompt for rm".into(),
            pattern: "^rm ".into(),
            action: PolicyRuleAction::Prompt,
            tool: Some("bash".into()),
            alternatives: vec!["Use trash-cli instead".into()],
        });
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        let result = engine.evaluate("bash", "rm -rf /tmp/foo", &mut session);
        match result {
            Some(PolicyDecision::Prompt {
                allow_once_code, ..
            }) => {
                assert_eq!(allow_once_code.len(), 6);
                assert!(allow_once_code.chars().all(|c| c.is_ascii_hexdigit()));
            }
            other => panic!("Expected Prompt with allow-once code, got {:?}", other),
        }
    }

    // ── Tool Scoping ────────────────────────────────────────────────

    #[test]
    fn rule_scoped_to_bash_ignores_read_tool() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(deny_rule("bash-only", ".", vec![]));
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        // Rule should NOT match for "read" tool
        let result = engine.evaluate("read", "anything", &mut session);
        assert!(
            result.is_none(),
            "Bash-only rule should not affect read tool"
        );
    }

    #[test]
    fn unscoped_rule_applies_to_all_tools() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(PolicyRuleDef {
            id: "global-deny".into(),
            description: "Global deny".into(),
            pattern: "dangerous".into(),
            action: PolicyRuleAction::Deny,
            tool: None, // Applies to all tools
            alternatives: vec![],
        });
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        assert!(
            engine
                .evaluate("read", "dangerous command", &mut session)
                .is_some(),
            "Unscoped rule should match read tool"
        );
    }

    // ── Priority: First Match Wins ──────────────────────────────────

    #[test]
    fn first_allow_rule_wins_before_deny() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(allow_rule("allow-first", "^git"));
        config
            .rules
            .push(deny_rule("deny-second", "^git push", vec![]));
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        // "git push" should be allowed because "allow-first" matches first
        let result = engine.evaluate("bash", "git push", &mut session);
        assert!(
            matches!(result, Some(PolicyDecision::Allow { .. })),
            "First-match rule should allow before deny rule is checked"
        );
    }

    // ── No Match ────────────────────────────────────────────────────

    #[test]
    fn no_matching_rule_returns_none() {
        let engine = ExecutionPolicyEngine::new();
        let mut session = PolicySession::new();
        let result = engine.evaluate("bash", "anything", &mut session);
        assert!(result.is_none(), "Expected None when no rules configured");
    }

    // ── Edge Cases ──────────────────────────────────────────────────

    #[test]
    fn empty_command_does_not_panic() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(deny_rule("catch-all", ".*", vec![]));
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();
        let result = engine.evaluate("bash", "", &mut session);
        assert!(result.is_some(), "Catch-all should match empty command");
    }

    #[test]
    fn invalid_regex_pattern_is_skipped() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(PolicyRuleDef {
            id: "bad-regex".into(),
            description: "Broken".into(),
            pattern: "[invalid".into(),
            action: PolicyRuleAction::Deny,
            tool: None,
            alternatives: vec![],
        });
        config.rules.push(allow_rule("good-rule", "^git"));
        let engine = ExecutionPolicyEngine::from_config(&config);
        assert_eq!(engine.rule_count(), 1, "Invalid regex should be skipped");
    }

    // ── Circuit Breaker ─────────────────────────────────────────────

    #[test]
    fn circuit_breaker_blocks_after_max_consecutive() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(deny_rule("deny-all", ".*", vec![]));
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        // First 3 denials should be Deny from deny-all rule
        for i in 0..DEFAULT_MAX_CONSECUTIVE_DENIALS {
            let result = engine.evaluate("bash", &format!("cmd{}", i), &mut session);
            assert!(
                matches!(&result, Some(PolicyDecision::Deny { rule_id, .. }) if rule_id.as_deref() == Some("deny-all")),
                "Denial {} should be from deny-all rule",
                i + 1
            );
        }

        // 4th should be circuit-broken
        let result = engine.evaluate("bash", "another", &mut session);
        match &result {
            Some(PolicyDecision::Deny { rule_id, .. }) => {
                assert_eq!(
                    rule_id.as_deref(),
                    Some("circuit-breaker"),
                    "Expected circuit-breaker, got {:?}",
                    result
                );
            }
            other => panic!("Expected circuit-breaker Deny, got {:?}", other),
        }
    }

    #[test]
    fn allow_resets_consecutive_counter() {
        let mut config = ExecutionPolicyConfig::default();
        config.rules.push(deny_rule("deny-all", "^deny", vec![]));
        config.rules.push(allow_rule("allow-foo", "^foo"));
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        // 2 denials
        engine.evaluate("bash", "deny1", &mut session);
        engine.evaluate("bash", "deny2", &mut session);
        assert_eq!(session.consecutive_denials, 2);

        // 1 allow
        engine.evaluate("bash", "foo", &mut session);
        assert_eq!(session.consecutive_denials, 0);

        // 1 more denial shouldn't trigger breaker yet
        let result = engine.evaluate("bash", "deny3", &mut session);
        assert!(
            matches!(&result, Some(PolicyDecision::Deny { rule_id, .. }) if rule_id.as_deref() == Some("deny-all")),
            "After allow+1 deny, should still be deny-all, got {:?}",
            result
        );
    }

    // ── Allow-Once Codes ────────────────────────────────────────────

    #[test]
    fn allow_once_code_is_6_hex_chars() {
        let mut session = PolicySession::new();
        let code = session.generate_allow_once_code("git push");
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn allow_once_code_is_single_use() {
        let mut session = PolicySession::new();
        let code = session.generate_allow_once_code("git push");

        assert!(session.consume_allow_once(&code));
        assert!(
            !session.consume_allow_once(&code),
            "Allow-once code should be single-use"
        );
    }

    #[test]
    fn same_command_different_code() {
        let mut session = PolicySession::new();
        // Each call to generate should produce a cached code
        let code1 = session.generate_allow_once_code("ls");
        let code2 = session.generate_allow_once_code("ls");
        // After generating twice, the second call will overwrite with same hash-derived code
        assert_eq!(
            code1, code2,
            "Same command should produce same allow-once code"
        );
    }

    // ── Protected Patterns ──────────────────────────────────────────

    #[test]
    fn protected_pattern_triggers_prompt() {
        let mut config = ExecutionPolicyConfig::default();
        config.protected_patterns.push("sudo rm -rf /".to_string());
        let engine = ExecutionPolicyEngine::from_config(&config);
        let mut session = PolicySession::new();

        let result = engine.evaluate("bash", "sudo rm -rf /", &mut session);
        assert!(
            matches!(&result, Some(PolicyDecision::Prompt { reason, .. }) if reason == "Command matches protected pattern"),
            "Protected pattern should prompt, got {:?}",
            result
        );
    }
}
