//! Lightweight, dependency-minimal secret redaction.
//!
//! This crate holds the single canonical `redact_secrets` sanitizer so that
//! callers which must not pull in the heavier `jcode-secrets` stack (`age`,
//! `keyring`, ...) — notably `jcode-logging` — can still scrub secrets from
//! output. `jcode-secrets` re-exports [`redact_secrets`] for backward
//! compatibility.
//!
//! **Best-effort:** this is high-precision pattern matching for *secret-shaped*
//! tokens, not a guarantee. It cannot know the actual values stored in the
//! secrets manager, and novel or low-entropy token formats may slip through.
//! Treat it as defense-in-depth, not a substitute for not logging secrets.
//!
//! Patterns aggregated from codex `codex-rs/secrets/src/sanitizer.rs`,
//! oh-my-codex `src/auth/redact.ts`, and jcode's own export / session-history
//! redactors.

use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

/// Known environment variable names that carry API keys or secrets.
/// Matched case-insensitively in `KEY=value` assignments.
const KNOWN_SECRET_ENV_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "OPENAI_COMPAT_API_KEY",
    "OPENROUTER_API_KEY",
    "ZHIPU_API_KEY",
    "ZAI_API_KEY",
    "COHERE_API_KEY",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "MINIMAX_API_KEY",
    "XAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "FIREWORKS_API_KEY",
    "GROQ_API_KEY",
    "MISTRAL_API_KEY",
    "OPENCODE_API_KEY",
    "OPENCODE_GO_API_KEY",
    "TOGETHER_API_KEY",
    "PERPLEXITY_API_KEY",
    "CEREBRAS_API_KEY",
    "NVIDIA_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_AUTH_TOKEN",
];

/// Ordered (regex, replacement) table, compiled once.
///
/// Order matters: specific high-precision token formats run before the generic
/// `key=value` / env-var assignment patterns. The assignment patterns exclude a
/// leading `[` (`[^\s"'\[]`) so they never re-match an already-inserted
/// `[REDACTED:*]` placeholder.
static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    let env_names = KNOWN_SECRET_ENV_VARS.join("|");
    vec![
        // OpenAI / Anthropic style keys: `sk-...`, `sk-ant-...`
        (Regex::new(r"\bsk-[A-Za-z0-9_\-\.]{20,}").unwrap(), "[REDACTED:sk]"),
        // GitHub classic tokens: ghp_/gho_/ghs_/ghr_/ghu_
        (Regex::new(r"\bgh[opsru]_[A-Za-z0-9]{20,}").unwrap(), "[REDACTED:github]"),
        // GitHub fine-grained PAT
        (Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}").unwrap(), "[REDACTED:github]"),
        // Stripe secret/restricted/publishable live/test keys (underscore form)
        (Regex::new(r"\b[rsp]k_(?:live|test)_[A-Za-z0-9]{16,}").unwrap(), "[REDACTED:stripe]"),
        // Google API keys
        (Regex::new(r"\bAIza[0-9A-Za-z_\-]{35}").unwrap(), "[REDACTED:google]"),
        // Google OAuth access tokens
        (Regex::new(r"\bya29\.[A-Za-z0-9._\-]{20,}").unwrap(), "[REDACTED:google]"),
        // Slack tokens: xoxb-/xoxp-/xoxa-/xoxr-/xoxs-
        (Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{10,}").unwrap(), "[REDACTED:slack]"),
        // AWS access key IDs
        (Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(), "[REDACTED:aws]"),
        // z.ai / ZHIPU style tokens: {32 hex}.{12+ alphanum}
        (Regex::new(r"\b[a-f0-9]{32}\.[A-Za-z0-9]{12,}").unwrap(), "[REDACTED:token]"),
        // JSON Web Tokens: base64url header.payload.signature (header+payload start eyJ)
        (
            Regex::new(r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}").unwrap(),
            "[REDACTED:jwt]",
        ),
        // HTTP Bearer tokens (keep the "Bearer " prefix)
        (Regex::new(r"(?i)\b(Bearer\s+)[A-Za-z0-9._\-]{16,}").unwrap(), "${1}[REDACTED]"),
        // Generic api_key/token/secret/password assignments
        (
            Regex::new(r#"(?i)\b(api[_-]?key|token|secret|password)\b(\s*[:=]\s*)(["']?)[^\s"'\[]{8,}"#).unwrap(),
            "${1}${2}[REDACTED]",
        ),
        // Known provider env-var assignments: KEY=value. The value's first char
        // must not be `[`, so an already-inserted `[REDACTED...]` placeholder is
        // never re-redacted (e.g. when layered after another sanitizer).
        (
            Regex::new(&format!(r#"(?i)\b({})(\s*=\s*)([^\r\n,'"\s\[][^\r\n,'"\s]*)"#, env_names)).unwrap(),
            "${1}${2}[REDACTED:env]",
        ),
    ]
});

/// Redact known secret patterns from `input`, returning a sanitized string.
///
/// The original string is not modified. Allocation is avoided unless a pattern
/// actually matches: each step keeps the current value borrowed until a regex
/// produces a replacement, so secret-free input incurs at most one final copy.
///
/// # Example
///
/// ```
/// use jcode_redact::redact_secrets;
///
/// let safe = redact_secrets("My token is sk-abc123DEF456ghi789jkl012");
/// assert!(safe.contains("[REDACTED:sk]"));
/// ```
pub fn redact_secrets(input: &str) -> String {
    let mut out: Cow<'_, str> = Cow::Borrowed(input);
    for (re, replacement) in PATTERNS.iter() {
        // Extract an owned result only when a replacement actually happened, so
        // the borrow of `out` does not span the reassignment below.
        let replaced: Option<String> = match re.replace_all(&out, *replacement) {
            Cow::Owned(s) => Some(s),
            Cow::Borrowed(_) => None,
        };
        if let Some(s) = replaced {
            out = Cow::Owned(s);
        }
    }
    out.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_openai_key() {
        let result = redact_secrets("sk-abc123DEF456ghi789jkl012");
        assert!(result.contains("[REDACTED:sk]"));
        assert!(!result.contains("sk-abc123DEF456ghi789jkl012"));
    }

    #[test]
    fn redacts_anthropic_key() {
        let result = redact_secrets("sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456");
        assert!(result.contains("[REDACTED:sk]"));
    }

    #[test]
    fn redacts_github_token() {
        let result = redact_secrets("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
        assert!(result.contains("[REDACTED:github]"));
    }

    #[test]
    fn redacts_github_fine_grained_pat() {
        let result = redact_secrets("github_pat_11ABCDEFG0abcdefghijklmnop");
        assert!(result.contains("[REDACTED:github]"));
    }

    #[test]
    fn redacts_stripe_key() {
        // Assembled at runtime so the literal isn't flagged by secret scanners.
        let result = redact_secrets(concat!("sk_", "live_", "abcdefghijklmnop1234567890"));
        assert!(result.contains("[REDACTED:stripe]"), "got: {result}");
        assert!(!result.contains("abcdefghijklmnop1234567890"));
    }

    #[test]
    fn redacts_google_api_key() {
        let result = redact_secrets("AIzaSyA1234567890abcdefghijklmnopqrstuvw");
        assert!(result.contains("[REDACTED:google]"), "got: {result}");
    }

    #[test]
    fn redacts_google_oauth_token() {
        let result = redact_secrets("ya29.a0AfB_byC1234567890abcdefghij");
        assert!(result.contains("[REDACTED:google]"), "got: {result}");
    }

    #[test]
    fn redacts_slack_token() {
        // Assembled at runtime so the literal isn't flagged by secret scanners.
        let result = redact_secrets(concat!("xox", "b-123456789012-abcdefghijklmnop"));
        assert!(result.contains("[REDACTED:slack]"), "got: {result}");
    }

    #[test]
    fn redacts_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4";
        let result = redact_secrets(jwt);
        assert!(result.contains("[REDACTED:jwt]"), "got: {result}");
        assert!(!result.contains("SflKxwRJSMeKKF2QT4"));
    }

    #[test]
    fn redacts_bearer_token() {
        let result =
            redact_secrets("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...");
        assert!(result.contains("Bearer [REDACTED]"));
        assert!(!result.contains("eyJhbGci"));
    }

    #[test]
    fn redacts_env_var_assignment() {
        let result = redact_secrets("OPENAI_API_KEY=sk-abc123def456");
        assert!(result.contains("[REDACTED:env]"));
        assert!(result.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn redacts_aws_key() {
        let result = redact_secrets("AKIAIOSFODNN7EXAMPLE");
        assert!(result.contains("[REDACTED:aws]"));
    }

    #[test]
    fn preserves_safe_text() {
        let input = "Hello, this is a normal message with no secrets.";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn redacts_multiple_secrets() {
        let input = concat!(
            "OPENAI_API_KEY=sk-abc123DEF456ghi789jkl012, ",
            "GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ123456, ",
            "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"
        );
        let result = redact_secrets(input);
        assert!(!result.contains("sk-abc123DEF456ghi789jkl012"));
        assert!(!result.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"));
        assert!(!result.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
    }

    #[test]
    fn does_not_re_redact_existing_placeholder() {
        // A value already replaced by another sanitizer must be left intact.
        let input = "GROQ_API_KEY=[REDACTED_SECRET]";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn redacts_zai_token() {
        let result =
            redact_secrets("token=b8c37e33defde51cf91e1e03e51657da.AqkKii0K0VqLpQRnP");
        assert!(result.contains("[REDACTED:token]"));
    }

    #[test]
    fn redacts_gho_token() {
        let result = redact_secrets("gho_ABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
        assert!(result.contains("[REDACTED:github]"));
    }

    #[test]
    fn redacts_ghs_token() {
        let result = redact_secrets("ghs_ABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
        assert!(result.contains("[REDACTED:github]"));
    }
}
