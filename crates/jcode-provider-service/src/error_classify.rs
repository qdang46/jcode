//! Error classification for failover decisions.
//!
//! Plan Phase 6 detail:
//!
//!   > 1. Classify error (rate-limit / quota / server-error / auth)
//!   > 2. If retryable -> Catalog.provider.available().next()
//!
//! When a provider returns an error, the runtime needs to know:
//!  - Is this a transient error (rate-limit, server) that should
//!    trigger failover to the next provider?
//!  - Or is this a hard error (auth, bad request) that won't be
//!    fixed by switching providers?
//!
//! This module exposes [`ProviderError`] and [`classify`] so the
//! failover chain can make that decision.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// HTTP 429 — too many requests. Transient, trigger failover.
    RateLimit,
    /// HTTP 402 / quota exceeded. May be transient (after reset)
    /// or permanent (plan limit). Trigger failover with backoff.
    Quota,
    /// HTTP 5xx or network error. Transient.
    ServerError,
    /// HTTP 401 / 403. Not transient — credentials are bad. Don't
    /// failover; surface to the user.
    Auth,
    /// HTTP 4xx other than 401/402/403/429. Bad request; not
    /// transient. Don't failover.
    BadRequest,
    /// Network-level error (DNS, TCP, TLS). Transient.
    Network,
    /// Unknown / unparseable. Treated as non-transient to avoid
    /// masking real bugs.
    Unknown,
}

impl ErrorCategory {
    /// Should the failover chain walk to the next provider after
    /// this error?
    pub fn should_failover(self) -> bool {
        matches!(
            self,
            Self::RateLimit | Self::Quota | Self::ServerError | Self::Network
        )
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::RateLimit => "rate limit (HTTP 429)",
            Self::Quota => "quota exceeded (HTTP 402)",
            Self::ServerError => "server error (HTTP 5xx)",
            Self::Auth => "auth error (HTTP 401/403)",
            Self::BadRequest => "bad request (HTTP 4xx)",
            Self::Network => "network error",
            Self::Unknown => "unknown error",
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProviderError {
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("network: {0}")]
    Network(String),
    #[error("protocol: {0}")]
    Protocol(String),
}

/// Classify an error into a category.
pub fn classify(err: &ProviderError) -> ErrorCategory {
    match err {
        ProviderError::Http { status, .. } => classify_status(*status),
        ProviderError::Network(_) => ErrorCategory::Network,
        ProviderError::Protocol(_) => ErrorCategory::Unknown,
    }
}

/// Classify an HTTP status code.
pub fn classify_status(status: u16) -> ErrorCategory {
    match status {
        401 | 403 => ErrorCategory::Auth,
        402 => ErrorCategory::Quota,
        408 | 425 | 429 => ErrorCategory::RateLimit,
        400 | 405 | 406 | 407 | 409 | 410 | 411 | 412 | 413 | 414 | 415 | 416 | 417 | 418 => {
            ErrorCategory::BadRequest
        }
        500..=599 => ErrorCategory::ServerError,
        _ => ErrorCategory::Unknown,
    }
}

/// Try to parse a status code from an error body string (for
/// cases where the body says e.g. `{"type":"rate_limit_error"}`).
pub fn classify_body(body: &str) -> Option<ErrorCategory> {
    let lower = body.to_ascii_lowercase();
    if lower.contains("rate_limit") || lower.contains("too many requests") {
        Some(ErrorCategory::RateLimit)
    } else if lower.contains("quota") || lower.contains("insufficient_quota") {
        Some(ErrorCategory::Quota)
    } else if lower.contains("unauthorized") || lower.contains("invalid_api_key") {
        Some(ErrorCategory::Auth)
    } else if lower.contains("internal_server_error")
        || lower.contains("service_unavailable")
        || lower.contains("server_error")
    {
        Some(ErrorCategory::ServerError)
    } else {
        None
    }
}

/// Best-effort classifier that combines status code + body.
pub fn classify_with_body(status: u16, body: &str) -> ErrorCategory {
    classify_body(body).unwrap_or_else(|| classify_status(status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_triggers_failover() {
        let err = ProviderError::Http {
            status: 429,
            body: "{}".into(),
        };
        assert_eq!(classify(&err), ErrorCategory::RateLimit);
        assert!(classify(&err).should_failover());
    }

    #[test]
    fn auth_does_not_trigger_failover() {
        let err = ProviderError::Http {
            status: 401,
            body: "{}".into(),
        };
        assert_eq!(classify(&err), ErrorCategory::Auth);
        assert!(!classify(&err).should_failover());
    }

    #[test]
    fn server_error_triggers_failover() {
        let err = ProviderError::Http {
            status: 502,
            body: "{}".into(),
        };
        assert_eq!(classify(&err), ErrorCategory::ServerError);
        assert!(classify(&err).should_failover());
    }

    #[test]
    fn quota_triggers_failover() {
        let err = ProviderError::Http {
            status: 402,
            body: "payment_required".into(),
        };
        assert_eq!(classify(&err), ErrorCategory::Quota);
    }

    #[test]
    fn network_triggers_failover() {
        let err = ProviderError::Network("connection reset".into());
        assert_eq!(classify(&err), ErrorCategory::Network);
        assert!(classify(&err).should_failover());
    }

    #[test]
    fn body_classifier_recognizes_anthropic_rate_limit() {
        let body = r#"{"type":"rate_limit_error","message":"too many requests"}"#;
        assert_eq!(classify_body(body), Some(ErrorCategory::RateLimit));
    }

    #[test]
    fn body_classifier_recognizes_openai_quota() {
        let body = r#"{"error":{"type":"insufficient_quota","message":"..."}}"#;
        assert_eq!(classify_body(body), Some(ErrorCategory::Quota));
    }

    #[test]
    fn body_classifier_recognizes_auth() {
        let body = r#"{"error":"invalid_api_key"}"#;
        assert_eq!(classify_body(body), Some(ErrorCategory::Auth));
    }

    #[test]
    fn body_classifier_returns_none_for_unknown() {
        assert!(classify_body("unexpected response").is_none());
    }

    #[test]
    fn with_body_prefers_body_when_both_present() {
        // Status 401 but body says rate_limit.
        let cat = classify_with_body(401, "rate_limit_error");
        assert_eq!(cat, ErrorCategory::RateLimit);
    }

    #[test]
    fn with_body_falls_back_to_status() {
        // Status 503 and an unrecognized body.
        let cat = classify_with_body(503, "blah");
        assert_eq!(cat, ErrorCategory::ServerError);
    }
}
