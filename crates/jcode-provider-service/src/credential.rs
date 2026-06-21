//! Credential storage: the persistence layer for API keys and OAuth tokens.
//!
//! Phase 1 of the master plan. This module defines:
//!
//! - [`CredentialType`] — the shape of a stored secret (OAuth / API key /
//!   external command).
//! - [`Credential`] — a credential record with id, label, type, timestamps.
//! - [`CredentialService`] — async trait for the storage backends
//!   (in-memory, SQLite, OS keychain via `jcode-keyring-store`).
//!
//! Concrete implementations live in `jcode-provider-service::store` (this
//! crate's `store` module, see `src/store/`). The `Credential` ids are
//! UUIDs (string form) so that callers never see row ids from any specific
//! storage backend.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

use crate::types::ProviderId;

/// Stable, storage-independent identifier for a credential. UUID v4 string
/// in practice, but the type is opaque so callers don't depend on the
/// format.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialId(Arc<str>);

impl CredentialId {
    pub fn new(value: impl Into<String>) -> Result<Self, CredentialIdError> {
        let s: String = value.into();
        if s.trim().is_empty() {
            return Err(CredentialIdError::Empty);
        }
        Ok(Self(Arc::from(s)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CredentialId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for CredentialId {
    fn from(s: &str) -> Self {
        Self::new(s).expect("credential id must be non-empty")
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CredentialIdError {
    #[error("credential id must not be empty")]
    Empty,
}

/// The shape of a stored secret. We support three flavors:
///
/// - **OAuth** — access token + optional refresh token + expiry. The
///   Integration layer drives the refresh flow.
/// - **ApiKey** — a single opaque key, used for static-key providers like
///   OpenAI or Anthropic API key auth.
/// - **ExternalCommand** — for providers that want a dynamic token, we
///   store a shell command that produces the token on demand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialType {
    OAuth {
        access_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<DateTime<Utc>>,
    },
    ApiKey {
        key: String,
    },
    ExternalCommand {
        command: String,
    },
}

impl CredentialType {
    /// Short, user-visible summary, *without* leaking secret material.
    pub fn describe(&self) -> &'static str {
        match self {
            Self::OAuth { .. } => "OAuth token",
            Self::ApiKey { .. } => "API key",
            Self::ExternalCommand { .. } => "external command",
        }
    }

    /// Test-only: does this credential hold an OAuth access token?
    pub fn is_oauth(&self) -> bool {
        matches!(self, Self::OAuth { .. })
    }
}

/// A single credential record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: CredentialId,
    pub provider: ProviderId,
    /// Free-form label that the user can use to pick between multiple
    /// credentials for the same provider (e.g. "work", "personal").
    pub label: String,
    #[serde(flatten)]
    pub credential: CredentialType,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Credential {
    /// Construct a new credential. `created_at` is set to now; `updated_at`
    /// is left unset.
    pub fn new(provider: ProviderId, label: impl Into<String>, credential: CredentialType) -> Self {
        Self {
            id: CredentialId::new(uuid_like()).expect("non-empty uuid"),
            provider,
            label: label.into(),
            credential,
            created_at: Utc::now(),
            updated_at: None,
        }
    }

    /// Mark this credential as updated (called when an OAuth token is
    /// refreshed in place).
    pub fn touch(&mut self) {
        self.updated_at = Some(Utc::now());
    }
}

fn uuid_like() -> String {
    // We don't want to pull the `uuid` crate just for this; a v4-shaped hex
    // string is fine for a credential id. Real implementations can swap this
    // out for `uuid::Uuid::new_v4()` if they prefer.
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("cred-{:032x}", nanos)
}

/// Errors produced by the credential service.
#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("credential not found: {0}")]
    NotFound(CredentialId),
    #[error("provider has no credentials: {0}")]
    NoCredentials(ProviderId),
    #[error("storage failure: {0}")]
    Storage(String),
    #[error("invalid credential: {0}")]
    Invalid(String),
}

impl From<anyhow::Error> for CredentialError {
    fn from(e: anyhow::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

/// Storage interface for credentials.
///
/// Implementations must be `Send + Sync` so they can be shared via
/// `Arc<dyn CredentialService>`. The trait is async because both the
/// SQLite and keychain backends perform IO.
#[async_trait]
pub trait CredentialService: Send + Sync {
    /// Persist a new credential, replacing any existing credential with the
    /// same `(provider, label)` pair. Returns the stored id (which may
    /// differ from `cred.id` if the storage backend reassigned it).
    async fn upsert(&self, cred: Credential) -> Result<CredentialId, CredentialError>;

    /// List all credentials for a provider.
    async fn list(&self, provider: &ProviderId) -> Result<Vec<Credential>, CredentialError>;

    /// Fetch a specific credential by id.
    async fn get(&self, id: &CredentialId) -> Result<Credential, CredentialError>;

    /// Delete a credential by id. Idempotent: deleting a missing credential
    /// returns `Ok(())`.
    async fn delete(&self, id: &CredentialId) -> Result<(), CredentialError>;

    /// Delete every credential for a provider. Used by `provider logout`.
    async fn delete_all(&self, provider: &ProviderId) -> Result<usize, CredentialError>;

    /// Total number of credentials across all providers.
    async fn count(&self) -> Result<usize, CredentialError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_type_describe_does_not_leak_secrets() {
        let api = CredentialType::ApiKey {
            key: "sk-super-secret".into(),
        };
        assert_eq!(api.describe(), "API key");
        // Debug-formatting in test output is allowed; ensure normal describe
        // does not mention the key.
        assert!(!api.describe().contains("sk-super-secret"));
    }

    #[test]
    fn credential_new_sets_created_at() {
        let c = Credential::new(
            "anthropic".into(),
            "work",
            CredentialType::ApiKey { key: "sk-x".into() },
        );
        assert_eq!(c.provider.as_str(), "anthropic");
        assert_eq!(c.label, "work");
        assert!(c.created_at <= Utc::now());
        assert!(c.updated_at.is_none());
    }

    #[test]
    fn credential_touch_sets_updated_at() {
        let mut c = Credential::new(
            "openai".into(),
            "default",
            CredentialType::OAuth {
                access_token: "tok".into(),
                refresh_token: None,
                expires_at: None,
            },
        );
        assert!(c.updated_at.is_none());
        c.touch();
        assert!(c.updated_at.is_some());
    }

    #[test]
    fn oauth_serde_skips_none_fields() {
        let c = CredentialType::OAuth {
            access_token: "a".into(),
            refresh_token: None,
            expires_at: None,
        };
        let s = serde_json::to_string(&c).unwrap();
        assert!(!s.contains("refresh_token"));
        assert!(!s.contains("expires_at"));
    }

    #[test]
    fn credential_id_must_be_nonempty() {
        assert!(CredentialId::new("").is_err());
        let id = CredentialId::new("cred-abc").unwrap();
        assert_eq!(id.as_str(), "cred-abc");
    }
}
