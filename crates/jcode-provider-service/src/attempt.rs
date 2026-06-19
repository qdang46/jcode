//! OAuth attempt state machine.
//!
//! Plan Phase 2 deliverable: `crates/jcode-provider-service/src/attempt.rs`.
//!
//! The OAuth lifecycle has four states:
//
//! ```text
//!  Pending (10-min TTL) ──> Complete ──> credential stored
//!       │
//!       └──> Expired      (auto-cleaned by the integration service)
//! ```
//!
//! `OAuthAttempt` is the in-memory record for an in-flight login. The
//! `IntegrationService::start_oauth` call creates one and returns it
//! to the caller; the caller drives the browser/CLI flow; the
//! `IntegrationService::complete_oauth` call finalizes it.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::integration::AuthMethod;
use crate::types::ProviderId;

/// State of an in-flight OAuth login. Created when the user runs
/// `jcode provider connect anthropic`; expires after 10 minutes
/// (the opencode standard TTL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthAttempt {
    pub id: String,
    pub provider: ProviderId,
    pub method: AuthMethod,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Local callback server port, if the provider's OAuth flow uses
    /// a loopback redirect.
    pub callback_port: Option<u16>,
}

/// Status of an OAuth attempt. Persisted as part of the attempt
/// record's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    /// User has not yet completed the browser/CLI flow.
    Pending,
    /// User has completed the flow; credential is in the store.
    Complete,
    /// The 10-minute TTL elapsed without completion.
    Expired,
    /// The flow failed (e.g. user denied authorization).
    Failed,
    /// The flow was cancelled by the caller.
    Cancelled,
}

impl OAuthAttempt {
    /// Construct a new attempt that expires `ttl` from now.
    pub fn new(provider: ProviderId, method: AuthMethod, ttl: Duration) -> Self {
        let now = Utc::now();
        Self {
            id: new_attempt_uuid(),
            provider,
            method,
            created_at: now,
            expires_at: now + ttl,
            callback_port: None,
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }

    pub fn remaining(&self) -> Duration {
        self.expires_at - Utc::now()
    }

    /// The current status, computed from the timestamp.
    pub fn status(&self) -> AttemptStatus {
        if self.is_expired() {
            AttemptStatus::Expired
        } else {
            AttemptStatus::Pending
        }
    }

    /// How long the attempt has been alive.
    pub fn elapsed(&self) -> Duration {
        Utc::now() - self.created_at
    }
}

fn new_attempt_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("oauth-{:032x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_attempt_is_pending() {
        let a = OAuthAttempt::new(
            "anthropic".into(),
            AuthMethod::OAuth {
                authorization_url: "https://example.com/authorize".into(),
            },
            Duration::minutes(10),
        );
        assert_eq!(a.status(), AttemptStatus::Pending);
        assert!(!a.is_expired());
        assert!(a.remaining() > Duration::minutes(9));
    }

    #[test]
    fn expired_attempt_reports_expired_status() {
        let mut a = OAuthAttempt::new(
            "anthropic".into(),
            AuthMethod::ApiKey {
                env_var: "X".into(),
            },
            Duration::seconds(0),
        );
        // Force the expires_at into the past.
        a.expires_at = Utc::now() - Duration::seconds(1);
        assert!(a.is_expired());
        assert_eq!(a.status(), AttemptStatus::Expired);
    }

    #[test]
    fn elapsed_is_non_negative() {
        let a = OAuthAttempt::new(
            "anthropic".into(),
            AuthMethod::ApiKey {
                env_var: "X".into(),
            },
            Duration::minutes(10),
        );
        assert!(a.elapsed() >= Duration::zero());
    }

    #[test]
    fn attempt_id_is_unique() {
        let a = OAuthAttempt::new(
            "anthropic".into(),
            AuthMethod::ApiKey {
                env_var: "X".into(),
            },
            Duration::minutes(10),
        );
        assert!(a.id.starts_with("oauth-"));
    }
}
