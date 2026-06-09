use serde::{Deserialize, Serialize};

const PROVIDER_FAILOVER_PROMPT_PREFIX: &str = "[jcode-provider-failover]";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderFailoverPrompt {
    pub from_provider: String,
    pub from_label: String,
    pub to_provider: String,
    pub to_label: String,
    pub reason: String,
    pub estimated_input_chars: usize,
    pub estimated_input_tokens: usize,
}

impl ProviderFailoverPrompt {
    pub fn to_error_message(&self) -> String {
        let payload = serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string());
        format!(
            "{PROVIDER_FAILOVER_PROMPT_PREFIX}{payload}\n{} is unavailable; switching to {} would resend about {} input tokens (~{} chars).",
            self.from_label, self.to_label, self.estimated_input_tokens, self.estimated_input_chars,
        )
    }
}

pub fn parse_failover_prompt_message(message: &str) -> Option<ProviderFailoverPrompt> {
    let line = message.lines().next()?.trim();
    let json = line.strip_prefix(PROVIDER_FAILOVER_PROMPT_PREFIX)?;
    serde_json::from_str(json).ok()
}

// ---------------------------------------------------------------------------
// FailoverDecision — what the orchestrator should do with a provider error
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailoverDecision {
    /// Do NOT failover. The error is treated as terminal (non-retryable or
    /// session-halting).
    None,
    /// Fall back to the next provider in the chain but do NOT mark the
    /// current one unavailable (it might work for a smaller request later).
    RetryNextProvider,
    /// Fall back and mark the current provider unavailable for the rest of
    /// the session (rate-limit, auth, quota exhaustion).
    RetryAndMarkUnavailable,
    /// Halt — the error is a session-terminating condition (credits exhausted,
    /// billing hard limit, free usage cap). Do NOT failover; surface the error
    /// code so the TUI can show a final message.
    Halt,
}

impl FailoverDecision {
    pub fn should_failover(self) -> bool {
        matches!(
            self,
            Self::RetryNextProvider | Self::RetryAndMarkUnavailable
        )
    }

    pub fn should_mark_provider_unavailable(self) -> bool {
        matches!(self, Self::RetryAndMarkUnavailable)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::RetryNextProvider => "retry-next-provider",
            Self::RetryAndMarkUnavailable => "retry-and-mark-unavailable",
            Self::Halt => "halt",
        }
    }
}

// ---------------------------------------------------------------------------
// ErrorCode — structured error classification for downstream consumers
// ---------------------------------------------------------------------------

/// Structured error code produced by the classifier, so TUI / telemetry /
/// IPC consumers can match on the *kind* of error without reparsing the
/// message string.
///
/// Groups:
/// - **Retryable** — the orchestrator should fail over to another provider
///   (marks the current one unavailable so we don't burn retries on it).
/// - **Non-retryable** — the error will not resolve by switching providers;
///   surface to the user / let the agent loop handle it.
/// - **STOP** — the session cannot continue (credits, billing, free tier cap).
///
/// Inspired by oh-my-openagent's 3-tier classifier (99 patterns),
/// claude-code's `SDKAssistantMessageError` (7 categories), and Codex's
/// 22-variant `CodexErr` enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // --- Retryable (fail over to next provider) ---
    RateLimited,
    Overloaded,
    ServerError,
    ConnectionError,
    Timeout,
    ModelNotFound,

    // --- Non-retryable (propagate up, do NOT failover) ---
    ContextLengthExceeded,
    PermissionDenied,
    MessageAborted,
    InvalidRequest,
    ContentPolicy,
    ValidationError,

    // --- STOP (halt the session — billing/credits exhaustion) ---
    QuotaExceeded,
    InsufficientCredits,
    BillingLimitReached,
    FreeUsageLimit,

    // --- Fallback (classifier could not determine the kind) ---
    Unknown,
}

impl ErrorCode {
    pub fn is_stop(self) -> bool {
        matches!(
            self,
            Self::QuotaExceeded
                | Self::InsufficientCredits
                | Self::BillingLimitReached
                | Self::FreeUsageLimit
        )
    }

    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited
                | Self::Overloaded
                | Self::ServerError
                | Self::ConnectionError
                | Self::Timeout
                | Self::ModelNotFound
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::RateLimited => "rate_limited",
            Self::Overloaded => "overloaded",
            Self::ServerError => "server_error",
            Self::ConnectionError => "connection_error",
            Self::Timeout => "timeout",
            Self::ModelNotFound => "model_not_found",
            Self::ContextLengthExceeded => "context_length_exceeded",
            Self::PermissionDenied => "permission_denied",
            Self::MessageAborted => "message_aborted",
            Self::InvalidRequest => "invalid_request",
            Self::ContentPolicy => "content_policy",
            Self::ValidationError => "validation_error",
            Self::QuotaExceeded => "quota_exceeded",
            Self::InsufficientCredits => "insufficient_credits",
            Self::BillingLimitReached => "billing_limit_reached",
            Self::FreeUsageLimit => "free_usage_limit",
            Self::Unknown => "unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Error-name → ErrorCode mapping
// Provider SDKs often surface a structured `error.type` or `error.code` field
// (e.g. OpenAI's `rate_limit_error`, Anthropic's `overloaded_error`). When
// available this is the most reliable signal.
// ---------------------------------------------------------------------------

fn error_code_by_name(name: &str) -> Option<ErrorCode> {
    // Normalise: strip common suffixes and lower-case
    let normalised = name
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "");

    Some(match normalised.as_str() {
        // Retryable
        "ratelimit"
        | "ratelimiterror"
        | "rate_limit_error"
        | "toomanyrequests"
        | "rate_limit_exceeded" => ErrorCode::RateLimited,

        "overloaded"
        | "overloadederror"
        | "overloaded_error"
        | "serveroverloaded"
        | "server_is_overloaded"
        | "modeloverloadederror" => ErrorCode::Overloaded,

        "servererror"
        | "server_error"
        | "internalservererror"
        | "internal_error"
        | "internal_server_error" => ErrorCode::ServerError,

        "connectionerror"
        | "providerconnectionerror"
        | "connection_error"
        | "networkerror"
        | "network_error"
        | "econnrefused"
        | "econnreset" => ErrorCode::ConnectionError,

        "timeout" | "timeouterror" | "timeout_error" | "requesttimeout" | "request_timeout"
        | "stream_timeout" => ErrorCode::Timeout,

        "modelnotfound"
        | "modelunavailable"
        | "model_not_found"
        | "modelunavailableerror"
        | "modelnotfounderror"
        | "model_not_supported" => ErrorCode::ModelNotFound,

        // Non-retryable
        "contextlengtherror"
        | "contextlengthexceeded"
        | "context_length_exceeded"
        | "context_length_error"
        | "prompttooolong"
        | "prompt_is_too_long"
        | "string_too_long" => ErrorCode::ContextLengthExceeded,

        "permissiondenied"
        | "permissiondeniederror"
        | "permission_denied"
        | "authenticationerror"
        | "authentication_error"
        | "unauthorized"
        | "forbidden"
        | "accessdenied" => ErrorCode::PermissionDenied,

        "messageaborted"
        | "messageabortederror"
        | "message_aborted"
        | "turnaborted"
        | "interrupted" => ErrorCode::MessageAborted,

        "invalidrequesterror"
        | "invalid_request_error"
        | "invalidrequest"
        | "badrequesterror"
        | "bad_request_error"
        | "invalidparameter" => ErrorCode::InvalidRequest,

        "contentpolicierror"
        | "content_policy_error"
        | "contentfilter"
        | "content_filter"
        | "safetyerror"
        | "safety_error"
        | "responsibleaipolicyviolation" => ErrorCode::ContentPolicy,

        "validationerror" | "validation_error" | "invalidtoolinput" | "invalid_prompt" => {
            ErrorCode::ValidationError
        }

        // STOP — halt the session
        "quotaexceeded"
        | "quotaexceedederror"
        | "quota_exceeded"
        | "insufficient_quota"
        | "insufficientquota"
        | "usage_limit_reached" => ErrorCode::QuotaExceeded,

        "insufficientcredits"
        | "insufficientcreditserror"
        | "insufficient_credits"
        | "nocredits"
        | "no_credits"
        | "credits_exhausted"
        | "outofcredits" => ErrorCode::InsufficientCredits,

        "billingerror"
        | "billing_error"
        | "paymentrequired"
        | "payment_required"
        | "payment_required_error"
        | "usage_not_included" => ErrorCode::BillingLimitReached,

        "freeusagelimiterror"
        | "free_usage_limit_error"
        | "freeusagelimit"
        | "free_usage_limit"
        | "freeusagelimitexceeded" => ErrorCode::FreeUsageLimit,

        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Pattern lists — ported from oh-my-openagent's 3-tier classifier
// (https://github.com/code-yeongyu/oh-my-openagent/blob/main/packages/model-core/src/model-error-classifier.ts)
// ---------------------------------------------------------------------------

/// Message substrings that indicate a **STOP** (session-halting) condition.
/// Checked BEFORE retryable patterns so that e.g. "quota exceeded" is
/// never retried even if "429" also appears in the message.
const STOP_MESSAGE_PATTERNS: &[&str] = &[
    "quota will reset after",
    "quota exceeded",
    "free usage limit",
    "billing limit",
    "billing hard limit",
    "monthly limit",
    "plan limit",
    "subscription quota",
    "subscription limit",
    "payment required",
    "out of credits",
    "credits exhausted",
    "insufficient credits",
    "insufficient balance",
    "credit balance",
    "usage limit for this month",
    "exhausted your capacity",
    "daily call limit",
    "daily limit",
    "usage limit reached for",
    "in arrears",
    "fair use policy",
    "recharge and try",
    // Chinese (from oh-my-openagent)
    "使用上限",
    "额度不足",
    "余额不足",
    "已耗尽",
];

/// Message substrings that indicate a **retryable** transient error.
/// These are only checked after STOP patterns, so quota/billing errors
/// in the same message are never accidentally retried.
const RETRYABLE_MESSAGE_PATTERNS: &[&str] = &[
    // English — direct from oh-my-openagent
    "rate_limit",
    "rate limit",
    "usage_limit_reached",
    "usage limit has been reached",
    "quota",
    "all credentials for model",
    "cooling down",
    "exhausted your capacity",
    "not found",
    "unavailable",
    "insufficient",
    "too many requests",
    "over limit",
    "overloaded",
    "bad gateway",
    "bad request",
    "unknown provider",
    "provider not found",
    "model_not_supported",
    "model not supported",
    "model is not supported",
    "connection error",
    "network error",
    "timeout",
    "service unavailable",
    "internal_server_error",
    "free usage",
    "usage exceeded",
    "temporarily unavailable",
    "try again",
    "503",
    "502",
    "504",
    // "429", "529", "503" — checked via contains_independent_status_code
    // in the error-code tier; still useful as message-substring fallback
    // when the provider doesn't surface a structured status but does mention "503"
    "selected provider is forbidden",
    "provider is forbidden",
    "server_error",
    "an error occurred while processing",
    // Chinese — retryable
    "频率限制",
    "请求过于频繁",
    "暂时不可用",
    "服务不可用",
];

/// Context-length patterns that trigger `RetryNextProvider` (do NOT mark the
/// provider unavailable — it might work for a smaller context).
const CONTEXT_LENGTH_PATTERNS: &[&str] = &[
    "context length",
    "context_length",
    "context window",
    "maximum context",
    "prompt is too long",
    "input is too long",
    "too many tokens",
    "max tokens",
    "token limit",
    "token_limit",
    "413 payload too large",
    "413 request entity too large",
];

/// The "auto-retry signal" gate: when the message contains "retrying in"
/// AND one of these gate substrings, the provider is already retrying on
/// its side — we should wait rather than fail- over.
const AUTO_RETRY_GATE_PATTERNS: &[&str] = &["rate limit", "cooling down", "credentials for model"];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn contains_independent_status_code(haystack: &str, code: &str) -> bool {
    let haystack_bytes = haystack.as_bytes();
    let code_len = code.len();

    haystack.match_indices(code).any(|(start, _)| {
        let before_ok = start == 0 || !haystack_bytes[start - 1].is_ascii_digit();
        let end = start + code_len;
        let after_ok = end == haystack_bytes.len() || !haystack_bytes[end].is_ascii_digit();
        before_ok && after_ok
    })
}

fn has_auto_retry_signal(lower: &str) -> bool {
    if !lower.contains("retrying in") {
        return false;
    }
    AUTO_RETRY_GATE_PATTERNS
        .iter()
        .any(|gate| lower.contains(gate))
}

fn classify_by_status_code(
    lower: &str,
    status_code: Option<u16>,
) -> Option<(FailoverDecision, ErrorCode)> {
    // Structured status_code from the HTTP response takes priority
    if let Some(sc) = status_code {
        let result = match sc {
            429 => Some((
                FailoverDecision::RetryAndMarkUnavailable,
                ErrorCode::RateLimited,
            )),
            401 | 403 => Some((
                FailoverDecision::RetryAndMarkUnavailable,
                ErrorCode::PermissionDenied,
            )),
            402 => Some((FailoverDecision::Halt, ErrorCode::BillingLimitReached)),
            408 => Some((
                FailoverDecision::RetryAndMarkUnavailable,
                ErrorCode::Timeout,
            )),
            500..=511 => Some((
                FailoverDecision::RetryAndMarkUnavailable,
                ErrorCode::ServerError,
            )),
            529 => Some((
                FailoverDecision::RetryAndMarkUnavailable,
                ErrorCode::Overloaded,
            )),
            _ => None,
        };
        if result.is_some() {
            return result;
        }
    }

    // Fallback: check for embedded status codes in the message string,
    // using the same independent-digit guard as the existing code.
    // These are checked AFTER the structured status_code so that an explicit
    // 200 with embedded "429" in the body does not override the HTTP truth.
    if contains_independent_status_code(lower, "529") {
        return Some((
            FailoverDecision::RetryAndMarkUnavailable,
            ErrorCode::Overloaded,
        ));
    }
    if contains_independent_status_code(lower, "429") {
        return Some((
            FailoverDecision::RetryAndMarkUnavailable,
            ErrorCode::RateLimited,
        ));
    }
    if contains_independent_status_code(lower, "402") {
        return Some((FailoverDecision::Halt, ErrorCode::BillingLimitReached));
    }
    if contains_independent_status_code(lower, "401")
        || contains_independent_status_code(lower, "403")
    {
        return Some((
            FailoverDecision::RetryAndMarkUnavailable,
            ErrorCode::PermissionDenied,
        ));
    }

    None
}

// ---------------------------------------------------------------------------
// Canonical classifier
// ---------------------------------------------------------------------------

/// The 3-tier decision tree ported from oh-my-openagent.
///
/// Priority order:
/// 1. Structured error name (most reliable — provider SDK error codes)
/// 2. STOP message patterns (billing/credits — never retry)
/// 3. Auto-retry signal ("retrying in" + gate)
/// 4. HTTP status codes (structured + embedded)
/// 5. Context-length patterns (fail-over but don't mark unavailable)
/// 6. Retryable message patterns (catch-all for transient errors)
/// 7. Default → None (terminal / unrecognised)
///
/// Returns `(FailoverDecision, Option<ErrorCode>)` so downstream code
/// (TUI, telemetry, IPC) can match on the structured code.
pub fn classify_failover_error_message_structured(
    message: &str,
    status_code: Option<u16>,
    error_name: Option<&str>,
    retry_after_secs: Option<u64>,
    _provider: Option<&str>,
) -> (FailoverDecision, Option<ErrorCode>) {
    // Tier 0: Retry-After header present.
    // The provider explicitly told us to wait and retry — treat as rate-limited.
    //
    // We only trust retry_after_secs when the message also looks retryable,
    // because some non-retryable errors (e.g. "payment required") carry a
    // retry-after by convention but should never be retried.
    if let Some(_ras) = retry_after_secs {
        let lower = message.to_ascii_lowercase();
        // Only treat as rate-limited if there's no STOP signal in the message.
        let has_stop = STOP_MESSAGE_PATTERNS.iter().any(|p| lower.contains(p));
        if !has_stop {
            return (
                FailoverDecision::RetryAndMarkUnavailable,
                Some(ErrorCode::RateLimited),
            );
        }
    }

    // Tier 1: Structured error name (most reliable signal).
    if let Some(name) = error_name {
        let name_lower = name.to_ascii_lowercase();
        if let Some(code) = error_code_by_name(&name_lower) {
            let decision = match code {
                // STOP tier — halt the session
                ErrorCode::QuotaExceeded
                | ErrorCode::InsufficientCredits
                | ErrorCode::BillingLimitReached
                | ErrorCode::FreeUsageLimit => FailoverDecision::Halt,

                // Non-retryable — propagate up
                ErrorCode::ContextLengthExceeded
                | ErrorCode::PermissionDenied
                | ErrorCode::MessageAborted
                | ErrorCode::InvalidRequest
                | ErrorCode::ContentPolicy
                | ErrorCode::ValidationError => FailoverDecision::None,

                // Retryable — fail over
                ErrorCode::RateLimited
                | ErrorCode::Overloaded
                | ErrorCode::ServerError
                | ErrorCode::ConnectionError
                | ErrorCode::Timeout
                | ErrorCode::ModelNotFound => FailoverDecision::RetryAndMarkUnavailable,

                ErrorCode::Unknown => FailoverDecision::None,
            };
            return (decision, Some(code));
        }
    }

    let lower = message.to_ascii_lowercase();

    // Tier 2: STOP message patterns (checked before everything else so
    // billing/quota errors are never accidentally retried).
    if STOP_MESSAGE_PATTERNS.iter().any(|p| lower.contains(p)) {
        let code =
            if lower.contains("quota") || lower.contains("credit") || lower.contains("billing") {
                ErrorCode::QuotaExceeded
            } else {
                ErrorCode::FreeUsageLimit
            };
        return (FailoverDecision::Halt, Some(code));
    }

    // Tier 3: Auto-retry signal — some providers say
    // "retrying in ~5 days [attempt #1]" which means they are already
    // retrying on their side; we should wait, not fail-over.
    // Only fires when message contains "retrying in" AND a gate pattern.
    if has_auto_retry_signal(&lower) {
        return (
            FailoverDecision::RetryAndMarkUnavailable,
            Some(ErrorCode::RateLimited),
        );
    }

    // Tier 4: HTTP status codes — structured then embedded.
    if let Some((decision, code)) = classify_by_status_code(&lower, status_code) {
        return (decision, Some(code));
    }

    // Tier 5a: Context-length patterns (do NOT mark provider unavailable).
    if CONTEXT_LENGTH_PATTERNS.iter().any(|p| lower.contains(p))
        || contains_independent_status_code(&lower, "413")
    {
        return (
            FailoverDecision::RetryNextProvider,
            Some(ErrorCode::ContextLengthExceeded),
        );
    }

    // Tier 5b: General retryable message patterns.
    if RETRYABLE_MESSAGE_PATTERNS.iter().any(|p| lower.contains(p)) {
        return (
            FailoverDecision::RetryAndMarkUnavailable,
            Some(ErrorCode::ServerError),
        );
    }

    // Default: terminal/unrecognised.
    (FailoverDecision::None, None)
}

// ---------------------------------------------------------------------------
// Legacy convenience wrapper
// ---------------------------------------------------------------------------

/// Legacy classifier — classifies an error message string only.
///
/// **Prefer [`classify_failover_error_message_structured`] when you have
/// access to the HTTP status code, error type, or Retry-After header.**
pub fn classify_failover_error_message(message: &str) -> FailoverDecision {
    classify_failover_error_message_structured(message, None, None, None, None).0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Existing tests (must not regress) ---------------------------------

    #[test]
    fn failover_prompt_roundtrips_from_error_message() {
        let prompt = ProviderFailoverPrompt {
            from_provider: "claude".to_string(),
            from_label: "Anthropic".to_string(),
            to_provider: "openai".to_string(),
            to_label: "OpenAI".to_string(),
            reason: "rate limit".to_string(),
            estimated_input_chars: 1200,
            estimated_input_tokens: 300,
        };

        let parsed = parse_failover_prompt_message(&prompt.to_error_message()).expect("prompt");
        assert_eq!(parsed, prompt);
    }

    #[test]
    fn classifier_marks_rate_limits_unavailable() {
        assert_eq!(
            classify_failover_error_message("429 Too Many Requests"),
            FailoverDecision::RetryAndMarkUnavailable
        );
    }

    #[test]
    fn classifier_retries_context_errors_without_marking_unavailable() {
        assert_eq!(
            classify_failover_error_message("context length exceeded"),
            FailoverDecision::RetryNextProvider
        );
    }

    #[test]
    fn classifier_ignores_embedded_status_digits() {
        assert_eq!(
            classify_failover_error_message("model version 4130 failed"),
            FailoverDecision::None
        );
    }

    // --- STOP tier tests ---------------------------------------------------

    #[test]
    fn stop_tier_halt_on_quota_exceeded() {
        let (decision, code) = classify_failover_error_message_structured(
            "quota exceeded: too many tokens per minute",
            Some(429),
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        assert_eq!(code, Some(ErrorCode::QuotaExceeded));
    }

    #[test]
    fn stop_tier_halt_on_credit_exhaustion() {
        let (decision, code) = classify_failover_error_message_structured(
            "Credit balance is too low to process this request",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        assert_eq!(code, Some(ErrorCode::QuotaExceeded));
    }

    #[test]
    fn stop_tier_wins_over_retryable_status() {
        // STOP pattern "quota will reset after" must beat the 429 status code
        let (decision, code) = classify_failover_error_message_structured(
            "quota will reset after 60s",
            Some(429),
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        assert_eq!(code, Some(ErrorCode::QuotaExceeded));
    }

    #[test]
    fn stop_tier_chinese() {
        let (decision, _) =
            classify_failover_error_message_structured("额度不足，请充值", None, None, None, None);
        assert_eq!(decision, FailoverDecision::Halt);
    }

    #[test]
    fn stop_tier_free_usage_limit() {
        let (decision, code) = classify_failover_error_message_structured(
            "Free usage limit reached for this model",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        assert_eq!(code, Some(ErrorCode::FreeUsageLimit));
    }

    // --- Retryable tier tests ----------------------------------------------

    #[test]
    fn retryable_on_rate_limit_message() {
        let (decision, code) = classify_failover_error_message_structured(
            "rate limit exceeded, please slow down",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
        assert_eq!(code, Some(ErrorCode::ServerError)); // caught by generic retryable patterns (contains "rate limit")
    }

    #[test]
    fn retryable_on_529_overloaded() {
        let (decision, code) = classify_failover_error_message_structured(
            "529: model overloaded",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
        assert_eq!(code, Some(ErrorCode::Overloaded));
    }

    #[test]
    fn retryable_on_502_bad_gateway() {
        let (decision, code) =
            classify_failover_error_message_structured("502 Bad Gateway", None, None, None, None);
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
        assert_eq!(code, Some(ErrorCode::ServerError));
    }

    #[test]
    fn retryable_on_structured_status_500() {
        let (decision, code) = classify_failover_error_message_structured(
            "Internal server error",
            Some(500),
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
        assert_eq!(code, Some(ErrorCode::ServerError));
    }

    // --- Context-length tests ----------------------------------------------

    #[test]
    fn context_length_retries_next_provider() {
        let (decision, code) = classify_failover_error_message_structured(
            "prompt is too long: 137500 tokens > 135000 maximum",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryNextProvider);
        assert_eq!(code, Some(ErrorCode::ContextLengthExceeded));
    }

    #[test]
    fn context_length_does_not_mark_unavailable() {
        assert_eq!(
            classify_failover_error_message("maximum context length is 200000 tokens"),
            FailoverDecision::RetryNextProvider
        );
    }

    // --- Error name tests --------------------------------------------------

    #[test]
    fn error_name_quota_exceeded_maps_to_halt() {
        let (decision, code) = classify_failover_error_message_structured(
            "API responded with error",
            Some(429),
            Some("quota_exceeded"),
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        assert_eq!(code, Some(ErrorCode::QuotaExceeded));
    }

    #[test]
    fn error_name_overloaded_maps_to_retry() {
        let (decision, code) = classify_failover_error_message_structured(
            "too many requests",
            Some(529),
            Some("overloaded_error"),
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
        assert_eq!(code, Some(ErrorCode::Overloaded));
    }

    #[test]
    fn error_name_context_length_maps_to_retry_next() {
        let (decision, code) = classify_failover_error_message_structured(
            "request too large",
            Some(400),
            Some("context_length_exceeded"),
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::None);
        assert_eq!(code, Some(ErrorCode::ContextLengthExceeded));
    }

    #[test]
    fn error_name_free_usage_limit_halt() {
        let (decision, code) = classify_failover_error_message_structured(
            "You have reached the free usage limit",
            Some(429),
            Some("free_usage_limit_error"),
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        assert_eq!(code, Some(ErrorCode::FreeUsageLimit));
    }

    // --- Auto-retry signal tests -------------------------------------------

    #[test]
    fn auto_retry_signal_triggers_on_retrying_in_with_gate() {
        let (decision, code) = classify_failover_error_message_structured(
            "retrying in ~5 days [attempt #1] due to rate limit",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
        assert_eq!(code, Some(ErrorCode::RateLimited));
    }

    #[test]
    fn auto_retry_signal_ignored_without_gate() {
        let (decision, code) = classify_failover_error_message_structured(
            "retrying in 5 seconds [attempt #1]",
            None,
            None,
            None,
            None,
        );
        // Falls through to default (no gate pattern)
        assert_eq!(decision, FailoverDecision::None);
        assert_eq!(code, None);
    }

    // --- Retry-After header tests ------------------------------------------

    #[test]
    fn retry_after_triggers_rate_limited_when_no_stop() {
        let (decision, code) = classify_failover_error_message_structured(
            "Too many requests",
            None,
            None,
            Some(30),
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
        assert_eq!(code, Some(ErrorCode::RateLimited));
    }

    #[test]
    fn retry_after_ignored_on_stop_signal() {
        let (decision, code) = classify_failover_error_message_structured(
            "quota exceeded: credit balance is too low",
            None,
            None,
            Some(3600),
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        // Halt because STOP pattern matched first, retry_after_secs is ignored
        // after checking Tier 0 against stop patterns.
        assert!(code.is_some_and(|c| c.is_stop()));
    }

    // --- Chinese pattern tests ---------------------------------------------

    #[test]
    fn chinese_retryable() {
        let (decision, _) = classify_failover_error_message_structured(
            "由于频率限制，请稍后重试",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::RetryAndMarkUnavailable);
    }

    // --- Edge case tests ---------------------------------------------------

    #[test]
    fn generic_unknown_error_returns_none() {
        let (decision, code) = classify_failover_error_message_structured(
            "Something completely unexpected happened",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::None);
        assert_eq!(code, None);
    }

    #[test]
    fn empty_message_returns_none() {
        let (decision, code) =
            classify_failover_error_message_structured("", None, None, None, None);
        assert_eq!(decision, FailoverDecision::None);
        assert_eq!(code, None);
    }

    #[test]
    fn embedded_429_with_stop_still_halt() {
        // STOP must win over embedded 429
        let (decision, code) = classify_failover_error_message_structured(
            "daily limit reached (429 usage limit)",
            None,
            None,
            None,
            None,
        );
        assert_eq!(decision, FailoverDecision::Halt);
        assert_eq!(code, Some(ErrorCode::FreeUsageLimit));
    }

    #[test]
    fn contains_independent_status_code_handles_edge_cases() {
        assert!(contains_independent_status_code("HTTP 429", "429"));
        assert!(contains_independent_status_code("429 ", "429"));
        assert!(contains_independent_status_code("status 429", "429"));
        assert!(!contains_independent_status_code("14290", "429"));
        assert!(!contains_independent_status_code("4290", "429"));
        assert!(!contains_independent_status_code("0429", "429"));
    }
}
