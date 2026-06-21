//! Auth credential rotation: a/b/c fallback chain.
//!
//! Plan §7 references oh-my-pi's auth-retry pattern:
//!   > Auth credential rotation: a/b/c rotation: resolve ->
//!   > refresh -> switch account
//!
//! When a provider returns 401/403, the runtime can try a fallback
//! credential (a, then b, then c) before giving up. This is useful
//! when a user has multiple API keys for the same provider (e.g.
//! work + personal) and the primary key is rate-limited or
//! revoked.
//!
//! The rotation state is persisted via the [`crate::credential::CredentialService`]:
//! each labeled credential under a provider is tried in order. The
//! caller can specify the labels (default: `["default", "work",
//! "personal"]`) or pass a custom list.

use std::sync::Arc;

use thiserror::Error;

use crate::credential::{Credential, CredentialService};
use crate::types::ProviderId;

#[derive(Debug, Error)]
pub enum RotationError {
    #[error("provider has no credentials to rotate through")]
    NoCredentials,
    #[error("all {attempts} credentials failed: {last}")]
    AllFailed { attempts: usize, last: String },
    #[error("storage error: {0}")]
    Store(String),
}

/// Outcome of a single attempt in the rotation chain.
#[derive(Debug, Clone)]
pub enum AttemptOutcome {
    /// The credential was used and the request succeeded.
    Success { label: String },
    /// The credential failed; the chain should try the next.
    Failure { label: String, error: String },
    /// The chain ran out of credentials.
    Exhausted { attempts: usize },
}

/// Run a rotation: try each labeled credential in `labels` order
/// until one succeeds. The `try_one` closure does the actual
/// request; the chain calls it with each credential in order.
pub async fn run_rotation<F, Fut>(
    credentials: Arc<dyn CredentialService>,
    provider: &ProviderId,
    labels: &[String],
    mut try_one: F,
) -> Result<AttemptOutcome, RotationError>
where
    F: FnMut(String, Credential) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    if labels.is_empty() {
        return Err(RotationError::NoCredentials);
    }
    let creds = credentials
        .list(provider)
        .await
        .map_err(|e| RotationError::Store(e.to_string()))?;
    if creds.is_empty() {
        return Err(RotationError::NoCredentials);
    }
    // Order: caller-supplied labels first, then any remaining
    // credentials in alphabetical order. This way the caller's
    // intent (e.g. "try default first, then work, then personal")
    // is honored even if some labels are missing.
    let mut tried: Vec<&Credential> = Vec::new();
    let mut _last_error: Option<String> = None;
    for label in labels {
        if let Some(c) = creds.iter().find(|c| &c.label == label) {
            let label_clone = c.label.clone();
            let cred = c.clone();
            match try_one(label_clone.clone(), cred).await {
                Ok(()) => {
                    return Ok(AttemptOutcome::Success { label: label_clone });
                }
                Err(e) => {
                    _last_error = Some(e.clone());
                    tried.push(c);
                }
            }
        }
    }
    // Try any remaining credentials not in the explicit label list.
    for c in &creds {
        if tried.iter().any(|t| t.id == c.id) {
            continue;
        }
        if labels.contains(&c.label) {
            continue;
        }
        let label_clone = c.label.clone();
        let cred = c.clone();
        match try_one(label_clone.clone(), cred).await {
            Ok(()) => {
                return Ok(AttemptOutcome::Success { label: label_clone });
            }
            Err(e) => {
                _last_error = Some(e.clone());
            }
        }
    }
    Ok(AttemptOutcome::Exhausted {
        attempts: tried.len(),
    })
}

impl AttemptOutcome {
    /// The label of the credential that succeeded, if any.
    pub fn success_label(&self) -> Option<&str> {
        match self {
            Self::Success { label } => Some(label),
            _ => None,
        }
    }

    /// True if the chain reached the end without success.
    pub fn exhausted(&self) -> bool {
        matches!(self, Self::Exhausted { .. })
    }
}

/// Default labels for the rotation: a/b/c ordering matches the
/// oh-my-pi reference ("work", "personal", "default" ordered
/// alphabetically — the caller passes them in their preferred
/// order).
pub fn default_rotation_labels() -> Vec<String> {
    vec![
        "default".to_string(),
        "work".to_string(),
        "personal".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::in_memory::InMemoryCredentialStore;

    async fn store_with_3_keys() -> Arc<InMemoryCredentialStore> {
        let s = InMemoryCredentialStore::new();
        s.upsert(Credential::new(
            "anthropic".into(),
            "default",
            CredentialType::ApiKey { key: "k1".into() },
        ))
        .await
        .unwrap();
        s.upsert(Credential::new(
            "anthropic".into(),
            "work",
            CredentialType::ApiKey { key: "k2".into() },
        ))
        .await
        .unwrap();
        s.upsert(Credential::new(
            "anthropic".into(),
            "personal",
            CredentialType::ApiKey { key: "k3".into() },
        ))
        .await
        .unwrap();
        Arc::new(s)
    }

    #[tokio::test]
    async fn first_label_succeeds() {
        let s = store_with_3_keys().await;
        let outcome = run_rotation(
            s.clone(),
            &"anthropic".into(),
            &["default".to_string(), "work".to_string()],
            |label, _cred| async move {
                assert_eq!(label, "default");
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.success_label(), Some("default"));
    }

    #[tokio::test]
    async fn falls_through_to_second_label() {
        let s = store_with_3_keys().await;
        let outcome = run_rotation(
            s.clone(),
            &"anthropic".into(),
            &["default".to_string(), "work".to_string()],
            |label, _cred| async move {
                if label == "default" {
                    Err("rate limited".into())
                } else {
                    Ok(())
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.success_label(), Some("work"));
    }

    #[tokio::test]
    async fn exhausts_after_all_fail() {
        let s = store_with_3_keys().await;
        let outcome = run_rotation(
            s.clone(),
            &"anthropic".into(),
            &default_rotation_labels(),
            |_label, _cred| async move { Err("all fail".into()) },
        )
        .await
        .unwrap();
        assert!(outcome.exhausted());
    }

    #[tokio::test]
    async fn empty_labels_is_error() {
        let s = store_with_3_keys().await;
        let err = run_rotation(s.clone(), &"anthropic".into(), &[], |_, _| async { Ok(()) })
            .await
            .unwrap_err();
        assert!(matches!(err, RotationError::NoCredentials));
    }

    #[tokio::test]
    async fn no_credentials_is_error() {
        let s: Arc<InMemoryCredentialStore> = Arc::new(InMemoryCredentialStore::new());
        let err = run_rotation(
            s.clone(),
            &"anthropic".into(),
            &["default".to_string()],
            |_, _| async { Ok(()) },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RotationError::NoCredentials));
    }

    #[tokio::test]
    async fn fallback_includes_unlisted_labels() {
        // The chain should try work (the only label we pass), then
        // fall through to any other credentials on the provider.
        let s = store_with_3_keys().await;
        let mut tried = Vec::new();
        let outcome = run_rotation(
            s.clone(),
            &"anthropic".into(),
            &["work".to_string()],
            |label, _cred| {
                tried.push(label.clone());
                async move { Err("all fail".into()) }
            },
        )
        .await
        .unwrap();
        assert!(outcome.exhausted());
        // The explicit label 'work' must be tried first.
        assert_eq!(tried.first().map(String::as_str), Some("work"));
        // All three credentials should have been tried.
        let mut sorted = tried.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["default", "personal", "work"]);
    }

    #[test]
    fn default_labels_includes_default_work_personal() {
        let labels = default_rotation_labels();
        assert_eq!(labels, vec!["default", "work", "personal"]);
    }
}
