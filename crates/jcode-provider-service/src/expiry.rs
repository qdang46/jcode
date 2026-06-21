//! Credential expiry inspection.
//!
//! Helper module for the runtime to ask: "which credentials are
//! about to expire?" Used by the TUI to surface a "refresh soon"
//! indicator and by the runtime to schedule a pre-emptive refresh.
//!
//! This is the read-only side of the auto-refresh flow; the
//! write side (refreshing the credential) lives in
//! [`crate::refresh`].

use crate::credential::{Credential, CredentialService, CredentialType};
use crate::types::ProviderId;
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpiryStatus {
    /// No expiry info available.
    NoExpiry,
    /// Token still valid for at least `threshold`.
    Fresh,
    /// Token expires within `threshold`.
    Soon { remaining: Duration },
    /// Token already expired.
    Expired,
}

impl ExpiryStatus {
    pub fn is_actionable(&self) -> bool {
        matches!(self, Self::Soon { .. } | Self::Expired)
    }
}

/// Inspect a single credential and return its expiry status.
pub fn status_of(cred: &Credential, threshold: Duration) -> ExpiryStatus {
    match &cred.credential {
        CredentialType::OAuth {
            expires_at: Some(exp),
            ..
        } => classify(*exp, threshold),
        // API keys don't expire (the provider would just return 401
        // if they were revoked, which is detected at request time).
        _ => ExpiryStatus::NoExpiry,
    }
}

fn classify(expires_at: DateTime<Utc>, threshold: Duration) -> ExpiryStatus {
    let now = Utc::now();
    let remaining = expires_at - now;
    if remaining <= Duration::zero() {
        ExpiryStatus::Expired
    } else if remaining < threshold {
        ExpiryStatus::Soon { remaining }
    } else {
        ExpiryStatus::Fresh
    }
}

/// Walk every credential for a provider and return those that are
/// within the refresh threshold. Used by the runtime to decide
/// what to refresh.
pub async fn due_for_refresh(
    credentials: &dyn CredentialService,
    provider: &ProviderId,
    threshold: Duration,
) -> Result<Vec<(Credential, ExpiryStatus)>, crate::credential::CredentialError> {
    let creds = credentials.list(provider).await?;
    Ok(creds
        .into_iter()
        .map(|c| {
            let status = status_of(&c, threshold);
            (c, status)
        })
        .filter(|(_, s)| s.is_actionable())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::{Credential, CredentialType};

    fn oauth_cred(expires_in: Option<Duration>) -> Credential {
        Credential {
            id: "cred-test".into(),
            provider: "anthropic".into(),
            label: "default".into(),
            credential: CredentialType::OAuth {
                access_token: "tok".into(),
                refresh_token: Some("rt".into()),
                expires_at: expires_in.map(|d| Utc::now() + d),
            },
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    #[test]
    fn api_key_has_no_expiry() {
        let c = Credential::new(
            "anthropic".into(),
            "default",
            CredentialType::ApiKey { key: "sk".into() },
        );
        let s = status_of(&c, Duration::minutes(5));
        assert_eq!(s, ExpiryStatus::NoExpiry);
    }

    #[test]
    fn oauth_without_expiry_is_no_expiry() {
        let c = oauth_cred(None);
        let s = status_of(&c, Duration::minutes(5));
        assert_eq!(s, ExpiryStatus::NoExpiry);
    }

    #[test]
    fn oauth_far_from_expiry_is_fresh() {
        let c = oauth_cred(Some(Duration::hours(1)));
        let s = status_of(&c, Duration::minutes(5));
        assert_eq!(s, ExpiryStatus::Fresh);
    }

    #[test]
    fn oauth_within_threshold_is_soon() {
        let c = oauth_cred(Some(Duration::minutes(2)));
        let s = status_of(&c, Duration::minutes(5));
        match s {
            ExpiryStatus::Soon { remaining } => {
                assert!(remaining <= Duration::minutes(2));
            }
            other => panic!("expected Soon, got {other:?}"),
        }
    }

    #[test]
    fn expired_oauth_reports_expired() {
        let c = oauth_cred(Some(Duration::seconds(-30)));
        let s = status_of(&c, Duration::minutes(5));
        assert_eq!(s, ExpiryStatus::Expired);
    }

    #[test]
    fn is_actionable_distinguishes_fresh_from_soon() {
        assert!(!ExpiryStatus::Fresh.is_actionable());
        assert!(!ExpiryStatus::NoExpiry.is_actionable());
        assert!(
            ExpiryStatus::Soon {
                remaining: Duration::seconds(1)
            }
            .is_actionable()
        );
        assert!(ExpiryStatus::Expired.is_actionable());
    }
}
