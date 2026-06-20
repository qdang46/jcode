//! OAuth credential auto-refresh.
//!
//! Plan criterion 11: "OAuth credential auto-refresh works before
//! token expiry".
//!
//! The auto-refresh strategy is provider-specific (Anthropic and
//! OpenAI use different token endpoints and grant types), so this
//! module exposes:
//!
//! - [`RefreshPolicy`] — the *generic* "is this token due for a
//!   refresh?" predicate. Used as the gate before any provider call.
//! - [`RefreshStrategy`] — the *abstract* description of how to
//!   refresh a given credential (which URL, which body shape). One
//!   concrete strategy per provider lands in a follow-up; the
//!   Anthropic strategy is sketched below.
//! - [`ensure_fresh`] — async function that takes a credential, the
//!   policy, and a strategy, and returns either the same credential
//!   (still valid) or a freshly-refreshed one persisted via the
//!   [`crate::credential::CredentialService`].
//!
//! The actual HTTP call lives behind the [`RefreshTransport`] trait
//! so tests can drive the refresh flow without making real network
//! calls.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use thiserror::Error;

use crate::credential::{Credential, CredentialId, CredentialService, CredentialType};
use crate::types::ProviderId;

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("credential is not OAuth and cannot be refreshed")]
    NotOAuth,
    #[error("refresh attempted but no refresh_token is stored")]
    NoRefreshToken,
    #[error("refresh transport failed: {0}")]
    Transport(String),
    #[error("refresh response was malformed: {0}")]
    InvalidResponse(String),
    #[error("credential store error: {0}")]
    Store(#[from] crate::credential::CredentialError),
}

/// When should a credential be refreshed? Common pattern is to
/// refresh when the token is within 5 minutes of expiry (gives
/// time to retry on transient failure). The threshold is
/// configurable per-deployment.
#[derive(Debug, Clone, Copy)]
pub struct RefreshPolicy {
    /// Refresh if `expires_at - now < this`.
    pub threshold: Duration,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            threshold: Duration::minutes(5),
        }
    }
}

impl RefreshPolicy {
    /// `true` if the credential is OAuth, has an `expires_at`, and
    /// the expiry is within the threshold.
    pub fn needs_refresh(&self, cred: &Credential) -> bool {
        match &cred.credential {
            CredentialType::OAuth {
                expires_at: Some(exp),
                ..
            } => {
                let now = Utc::now();
                let remaining = *exp - now;
                remaining < self.threshold
            }
            _ => false,
        }
    }
}

/// Abstract description of a provider's refresh endpoint. Concrete
/// strategies implement [`RefreshTransport::refresh`].
#[derive(Debug, Clone)]
pub struct RefreshRequest {
    pub provider: ProviderId,
    pub refresh_token: String,
}

/// Result of a successful refresh.
#[derive(Debug, Clone)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// HTTP transport for refresh calls. Implementations are typically a
/// thin wrapper around `reqwest`, but the trait is sync-free for
/// testing.
#[async_trait]
pub trait RefreshTransport: Send + Sync {
    async fn refresh(&self, req: RefreshRequest) -> Result<RefreshResponse, RefreshError>;
}

/// Default no-op transport for tests; returns an error.
pub struct NoopTransport;

#[async_trait]
impl RefreshTransport for NoopTransport {
    async fn refresh(&self, _req: RefreshRequest) -> Result<RefreshResponse, RefreshError> {
        Err(RefreshError::Transport("no transport configured".into()))
    }
}

/// Ensure the given credential is fresh. If it is, return it as-is.
/// If it is OAuth and within the refresh threshold, call
/// `transport.refresh()` and persist the new token via
/// `credentials`. Otherwise return the original credential
/// untouched.
pub async fn ensure_fresh<K: CredentialService + ?Sized>(
    cred: Credential,
    transport: &dyn RefreshTransport,
    credentials: &K,
    policy: RefreshPolicy,
) -> Result<Credential, RefreshError> {
    if !matches!(cred.credential, CredentialType::OAuth { .. }) {
        return Err(RefreshError::NotOAuth);
    }
    if !policy.needs_refresh(&cred) {
        return Ok(cred);
    }
    let refresh_token = match &cred.credential {
        CredentialType::OAuth {
            refresh_token: Some(rt),
            ..
        } => rt.clone(),
        CredentialType::OAuth { .. } => return Err(RefreshError::NoRefreshToken),
        _ => return Err(RefreshError::NotOAuth),
    };
    let resp = transport
        .refresh(RefreshRequest {
            provider: cred.provider.clone(),
            refresh_token,
        })
        .await?;
    let mut updated = cred.clone();
    updated.credential = CredentialType::OAuth {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at: resp.expires_at,
    };
    updated.touch();
    // Persist the new token. The credential's id stays the same so
    // the rest of the system keeps referring to the same record.
    let _id: CredentialId = credentials.upsert(updated.clone()).await?;
    Ok(updated)
}

/// Convenience: scan every credential for a given provider, refresh
/// any that are due, and return the count refreshed.
pub async fn refresh_due_for_provider(
    provider: &ProviderId,
    transport: &dyn RefreshTransport,
    credentials: Arc<dyn CredentialService>,
    policy: RefreshPolicy,
) -> Result<usize, RefreshError> {
    let creds = credentials.list(provider).await?;
    let mut refreshed = 0;
    for c in creds {
        if policy.needs_refresh(&c) {
            match ensure_fresh(c, transport, credentials.as_ref(), policy).await {
                Ok(_) => refreshed += 1,
                Err(e) => {
                    tracing::warn!(provider = %provider, error = %e, "refresh failed");
                }
            }
        }
    }
    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::in_memory::InMemoryCredentialStore;

    fn oauth_cred(expires_in: Duration, with_refresh: bool) -> Credential {
        Credential::new(
            "anthropic".into(),
            "oauth",
            CredentialType::OAuth {
                access_token: "old-access".into(),
                refresh_token: if with_refresh {
                    Some("refresh-tok".into())
                } else {
                    None
                },
                expires_at: Some(Utc::now() + expires_in),
            },
        )
    }

    #[test]
    fn policy_needs_refresh_within_threshold() {
        let p = RefreshPolicy::default();
        let c = oauth_cred(Duration::minutes(2), true);
        assert!(p.needs_refresh(&c));
    }

    #[test]
    fn policy_does_not_refresh_far_from_expiry() {
        let p = RefreshPolicy::default();
        let c = oauth_cred(Duration::hours(1), true);
        assert!(!p.needs_refresh(&c));
    }

    #[test]
    fn policy_does_not_refresh_non_oauth() {
        let p = RefreshPolicy::default();
        let c = Credential::new(
            "anthropic".into(),
            "default",
            CredentialType::ApiKey { key: "sk-x".into() },
        );
        assert!(!p.needs_refresh(&c));
    }

    #[test]
    fn policy_does_not_refresh_oauth_without_expiry() {
        let p = RefreshPolicy::default();
        let c = Credential {
            id: "cred-no-exp".into(),
            provider: "anthropic".into(),
            label: "oauth".into(),
            credential: CredentialType::OAuth {
                access_token: "tok".into(),
                refresh_token: Some("rt".into()),
                expires_at: None,
            },
            created_at: Utc::now(),
            updated_at: None,
        };
        assert!(!p.needs_refresh(&c));
    }

    #[tokio::test]
    async fn ensure_fresh_returns_unchanged_when_not_due() {
        let store = InMemoryCredentialStore::new();
        let c = oauth_cred(Duration::hours(1), true);
        store.upsert(c.clone()).await.unwrap();
        let t = NoopTransport;
        let out = ensure_fresh(c.clone(), &t, &store, RefreshPolicy::default())
            .await
            .unwrap();
        assert_eq!(out.id, c.id);
        assert_eq!(
            match out.credential {
                CredentialType::OAuth { access_token, .. } => access_token,
                _ => panic!("expected OAuth"),
            },
            "old-access"
        );
    }

    #[tokio::test]
    async fn ensure_fresh_calls_transport_when_due() {
        let store = InMemoryCredentialStore::new();
        let c = oauth_cred(Duration::minutes(1), true);
        store.upsert(c.clone()).await.unwrap();

        struct MockTransport;
        #[async_trait]
        impl RefreshTransport for MockTransport {
            async fn refresh(&self, req: RefreshRequest) -> Result<RefreshResponse, RefreshError> {
                assert_eq!(req.provider.as_str(), "anthropic");
                assert_eq!(req.refresh_token, "refresh-tok");
                Ok(RefreshResponse {
                    access_token: "new-access".into(),
                    refresh_token: Some("new-refresh".into()),
                    expires_at: Some(Utc::now() + Duration::hours(1)),
                })
            }
        }

        let out = ensure_fresh(c.clone(), &MockTransport, &store, RefreshPolicy::default())
            .await
            .unwrap();
        match out.credential {
            CredentialType::OAuth {
                access_token,
                refresh_token,
                expires_at,
            } => {
                assert_eq!(access_token, "new-access");
                assert_eq!(refresh_token.unwrap(), "new-refresh");
                assert!(expires_at.is_some());
            }
            _ => panic!("expected OAuth"),
        }
        // The new credential was persisted.
        let stored = store.get(&c.id).await.unwrap();
        match stored.credential {
            CredentialType::OAuth { access_token, .. } => {
                assert_eq!(access_token, "new-access");
            }
            _ => panic!("expected OAuth"),
        }
    }

    #[tokio::test]
    async fn ensure_fresh_errors_without_refresh_token() {
        let store = InMemoryCredentialStore::new();
        let c = oauth_cred(Duration::minutes(1), false);
        store.upsert(c.clone()).await.unwrap();
        let err = ensure_fresh(c, &NoopTransport, &store, RefreshPolicy::default())
            .await
            .unwrap_err();
        assert!(matches!(err, RefreshError::NoRefreshToken));
    }

    #[tokio::test]
    async fn ensure_fresh_errors_for_non_oauth() {
        let store = InMemoryCredentialStore::new();
        let c = Credential::new(
            "anthropic".into(),
            "default",
            CredentialType::ApiKey { key: "sk".into() },
        );
        store.upsert(c.clone()).await.unwrap();
        let err = ensure_fresh(c, &NoopTransport, &store, RefreshPolicy::default())
            .await
            .unwrap_err();
        assert!(matches!(err, RefreshError::NotOAuth));
    }

    #[tokio::test]
    async fn refresh_due_for_provider_counts_refreshed() {
        let store: Arc<dyn CredentialService> = Arc::new(InMemoryCredentialStore::new());
        // Two creds with distinct labels so the upsert de-dup doesn't
        // collapse them. One is due (1 min), one is not (1 hour).
        let due = {
            let mut c = oauth_cred(Duration::minutes(1), true);
            c.label = "due".into();
            c
        };
        let fresh = {
            let mut c = oauth_cred(Duration::hours(1), true);
            c.label = "fresh".into();
            c
        };
        store.upsert(due).await.unwrap();
        store.upsert(fresh).await.unwrap();
        struct MockTransport;
        #[async_trait]
        impl RefreshTransport for MockTransport {
            async fn refresh(&self, _: RefreshRequest) -> Result<RefreshResponse, RefreshError> {
                Ok(RefreshResponse {
                    access_token: "x".into(),
                    refresh_token: Some("y".into()),
                    expires_at: Some(Utc::now() + Duration::hours(1)),
                })
            }
        }
        let count = refresh_due_for_provider(
            &"anthropic".into(),
            &MockTransport,
            store,
            RefreshPolicy::default(),
        )
        .await
        .unwrap();
        assert_eq!(count, 1, "only the due credential should be refreshed");
    }
}
