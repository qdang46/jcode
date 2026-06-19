//! Integration: provider login flows and credential state.
//!
//! Phase 2 of the master plan.
//!
//! The Integration layer is responsible for:
//!
//! - Registering providers (id, label, supported auth methods).
//! - Detecting whether a provider is *connected* — i.e. has a usable
//!   credential, either from the keychain, an env var, or a
//!   `--api-key=...` override.
//! - Driving the OAuth login lifecycle (`OAuthAttempt` with a 10-minute TTL,
//!   matching opencode's behavior).
//!
//! It does **not** store credentials itself; it asks the
//! [`CredentialService`] (Phase 1) for those.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

use crate::attempt::OAuthAttempt;
use crate::credential::CredentialId;
use crate::types::ProviderId;

/// How a provider authenticates with its upstream service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AuthMethod {
    /// OAuth / browser-based login. The provider's `oauth` callback URL is
    /// opened in the user's browser; the resulting authorization code is
    /// exchanged for an access token.
    OAuth {
        /// URL the user should be redirected to (e.g.
        /// `https://claude.ai/oauth/authorize?...`).
        authorization_url: String,
    },
    /// Static API key, read from an env var by default but overridable via
    /// the `jcode provider connect <p> --api-key=...` CLI.
    ApiKey { env_var: String },
    /// Bearer token in the `Authorization` header, sourced from the named
    /// env var.
    BearerEnv { env_var: String },
    /// Custom header, useful for providers that use `x-api-key` instead of
    /// `Authorization`.
    CustomHeader { name: String, env_var: String },
}

impl AuthMethod {
    /// Short, user-visible label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::OAuth { .. } => "OAuth",
            Self::ApiKey { .. } => "API key",
            Self::BearerEnv { .. } => "Bearer (env)",
            Self::CustomHeader { .. } => "Custom header",
        }
    }
}

/// A registered provider's login options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginProvider {
    pub id: ProviderId,
    /// User-visible display name (e.g. "Anthropic", "OpenAI").
    pub label: String,
    /// Auth methods supported by this provider, in display order.
    pub auth_methods: Vec<AuthMethod>,
    /// Environment variables that, if set, indicate the provider is
    /// configured (e.g. `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`).
    pub env_keys: Vec<String>,
    /// Whether OAuth is the recommended login flow for this provider.
    pub oauth_preferred: bool,
}

impl LoginProvider {
    pub fn supports_oauth(&self) -> bool {
        self.auth_methods
            .iter()
            .any(|m| matches!(m, AuthMethod::OAuth { .. }))
    }

    pub fn oauth_method(&self) -> Option<&AuthMethod> {
        self.auth_methods
            .iter()
            .find(|m| matches!(m, AuthMethod::OAuth { .. }))
    }
}

/// What the integration layer knows about the current connection state of a
/// provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConnectionStatus {
    /// No credentials found; user must run `provider connect`.
    NotConfigured,
    /// Inline API key passed via CLI or env, no persisted credential.
    /// Carries the source (env var name) for diagnostics.
    InlineEnv { env_var: String },
    /// API key persisted in the credential store.
    ApiKey { credential_id: CredentialId, label: String },
    /// OAuth login present, valid (or refreshable).
    OAuth {
        credential_id: CredentialId,
        label: String,
        expires_at: Option<DateTime<Utc>>,
    },
}

impl ConnectionStatus {
    pub fn is_connected(&self) -> bool {
        !matches!(self, Self::NotConfigured)
    }

    /// What to call this in user-facing strings.
    pub fn summary(&self) -> String {
        match self {
            Self::NotConfigured => "not configured".into(),
            Self::InlineEnv { env_var } => format!("env:{}", env_var),
            Self::ApiKey { label, .. } => format!("api key:{}", label),
            Self::OAuth { label, expires_at, .. } => match expires_at {
                Some(t) => format!("oauth:{} (expires {})", label, t),
                None => format!("oauth:{}", label),
            },
        }
    }
}

#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("unknown provider: {0}")]
    UnknownProvider(ProviderId),
    #[error("provider does not support {0}")]
    UnsupportedAuth(&'static str),
    #[error("oauth attempt not found: {0}")]
    OAuthAttemptNotFound(String),
    #[error("oauth attempt expired")]
    OAuthAttemptExpired,
    #[error("storage failure: {0}")]
    Storage(String),
}

impl From<anyhow::Error> for IntegrationError {
    fn from(e: anyhow::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

/// Integration service: register providers, detect connections, drive
/// OAuth attempts. Implementations are typically in-memory + backed by
/// [`crate::credential::CredentialService`] for persistence.
#[async_trait]
pub trait IntegrationService: Send + Sync {
    /// Register a provider's login options.
    async fn register(&self, provider: LoginProvider) -> Result<(), IntegrationError>;

    /// Look up a provider by id.
    async fn get(&self, id: &ProviderId) -> Result<LoginProvider, IntegrationError>;

    /// List all registered providers.
    async fn list(&self) -> Result<Vec<LoginProvider>, IntegrationError>;

    /// Detect the current [`ConnectionStatus`] for a provider, considering
    /// env vars, persisted credentials, and inline CLI flags.
    async fn detect(&self, id: &ProviderId) -> Result<ConnectionStatus, IntegrationError>;

    /// Start an OAuth attempt for a provider. Returns the attempt record
    /// (with its TTL) so the caller can drive the browser flow.
    async fn start_oauth(
        &self,
        id: &ProviderId,
    ) -> Result<OAuthAttempt, IntegrationError>;

    /// Look up an in-flight OAuth attempt.
    async fn get_oauth_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<OAuthAttempt, IntegrationError>;

    /// Finalize an OAuth attempt with the received credentials.
    /// Stores the credential via the [`crate::credential::CredentialService`]
    /// and clears the attempt.
    async fn complete_oauth(
        &self,
        attempt_id: &str,
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<CredentialId, IntegrationError>;

    /// Cancel an in-flight OAuth attempt.
    async fn cancel_oauth(&self, attempt_id: &str) -> Result<(), IntegrationError>;

    /// Persist an API key for a provider. If a credential with the same
    /// `(provider, label)` already exists, it is replaced.
    async fn save_api_key(
        &self,
        id: &ProviderId,
        label: &str,
        key: &str,
    ) -> Result<CredentialId, IntegrationError>;
}

// ---------------------------------------------------------------------------
// In-memory reference implementation
// ---------------------------------------------------------------------------

/// Simple in-memory integration service. Used for tests, the Phase 0 boot
/// path, and as a fallback when no persistent backend is available.
pub struct InMemoryIntegration {
    providers: Mutex<std::collections::HashMap<ProviderId, LoginProvider>>,
    attempts: Mutex<std::collections::HashMap<String, OAuthAttempt>>,
}

impl InMemoryIntegration {
    pub fn new() -> Self {
        Self {
            providers: Mutex::new(Default::default()),
            attempts: Mutex::new(Default::default()),
        }
    }
}

impl Default for InMemoryIntegration {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IntegrationService for InMemoryIntegration {
    async fn register(&self, provider: LoginProvider) -> Result<(), IntegrationError> {
        let mut map = self.providers.lock().await;
        map.insert(provider.id.clone(), provider);
        Ok(())
    }

    async fn get(&self, id: &ProviderId) -> Result<LoginProvider, IntegrationError> {
        let map = self.providers.lock().await;
        map.get(id)
            .cloned()
            .ok_or_else(|| IntegrationError::UnknownProvider(id.clone()))
    }

    async fn list(&self) -> Result<Vec<LoginProvider>, IntegrationError> {
        let map = self.providers.lock().await;
        Ok(map.values().cloned().collect())
    }

    async fn detect(&self, id: &ProviderId) -> Result<ConnectionStatus, IntegrationError> {
        // Phase 0 stub: just return NotConfigured. Phase 2b will wire this
        // up to the CredentialService + env var detection.
        let _ = self.get(id).await?;
        Ok(ConnectionStatus::NotConfigured)
    }

    async fn start_oauth(
        &self,
        id: &ProviderId,
    ) -> Result<OAuthAttempt, IntegrationError> {
        let provider = self.get(id).await?;
        let method = provider
            .oauth_method()
            .cloned()
            .ok_or(IntegrationError::UnsupportedAuth("oauth"))?;
        let attempt = OAuthAttempt::new(id.clone(), method, Duration::minutes(10));
        self.attempts
            .lock()
            .await
            .insert(attempt.id.clone(), attempt.clone());
        Ok(attempt)
    }

    async fn get_oauth_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<OAuthAttempt, IntegrationError> {
        let map = self.attempts.lock().await;
        map.get(attempt_id)
            .cloned()
            .ok_or_else(|| IntegrationError::OAuthAttemptNotFound(attempt_id.to_string()))
    }

    async fn complete_oauth(
        &self,
        _attempt_id: &str,
        _access_token: String,
        _refresh_token: Option<String>,
        _expires_at: Option<DateTime<Utc>>,
    ) -> Result<CredentialId, IntegrationError> {
        // Phase 0 stub. The real implementation needs the CredentialService
        // injected; that lands in Phase 1.
        Err(IntegrationError::Storage(
            "complete_oauth requires CredentialService (Phase 1)".into(),
        ))
    }

    async fn cancel_oauth(&self, attempt_id: &str) -> Result<(), IntegrationError> {
        self.attempts.lock().await.remove(attempt_id);
        Ok(())
    }

    async fn save_api_key(
        &self,
        _id: &ProviderId,
        _label: &str,
        _key: &str,
    ) -> Result<CredentialId, IntegrationError> {
        Err(IntegrationError::Storage(
            "save_api_key requires CredentialService (Phase 1)".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anthropic() -> LoginProvider {
        LoginProvider {
            id: "anthropic".into(),
            label: "Anthropic".into(),
            auth_methods: vec![
                AuthMethod::OAuth {
                    authorization_url: "https://claude.ai/oauth/authorize".into(),
                },
                AuthMethod::ApiKey {
                    env_var: "ANTHROPIC_API_KEY".into(),
                },
            ],
            env_keys: vec!["ANTHROPIC_API_KEY".into()],
            oauth_preferred: true,
        }
    }

    #[tokio::test]
    async fn register_and_get() {
        let svc = InMemoryIntegration::new();
        svc.register(anthropic()).await.unwrap();
        let got = svc.get(&"anthropic".into()).await.unwrap();
        assert_eq!(got.label, "Anthropic");
        assert!(got.supports_oauth());
    }

    #[tokio::test]
    async fn get_unknown_errors() {
        let svc = InMemoryIntegration::new();
        let err = svc.get(&"mystery".into()).await.unwrap_err();
        assert!(matches!(err, IntegrationError::UnknownProvider(_)));
    }

    #[tokio::test]
    async fn start_oauth_creates_attempt_with_ttl() {
        let svc = InMemoryIntegration::new();
        svc.register(anthropic()).await.unwrap();
        let attempt = svc.start_oauth(&"anthropic".into()).await.unwrap();
        assert_eq!(attempt.provider.as_str(), "anthropic");
        assert!(!attempt.is_expired());
        assert!(attempt.remaining() > Duration::minutes(9));
    }

    #[tokio::test]
    async fn start_oauth_fails_when_unsupported() {
        let mut p = anthropic();
        p.auth_methods
            .retain(|m| !matches!(m, AuthMethod::OAuth { .. }));
        let svc = InMemoryIntegration::new();
        svc.register(p).await.unwrap();
        let err = svc.start_oauth(&"anthropic".into()).await.unwrap_err();
        assert!(matches!(err, IntegrationError::UnsupportedAuth(_)));
    }

    #[tokio::test]
    async fn cancel_oauth_removes_attempt() {
        let svc = InMemoryIntegration::new();
        svc.register(anthropic()).await.unwrap();
        let attempt = svc.start_oauth(&"anthropic".into()).await.unwrap();
        svc.cancel_oauth(&attempt.id).await.unwrap();
        let err = svc.get_oauth_attempt(&attempt.id).await.unwrap_err();
        assert!(matches!(err, IntegrationError::OAuthAttemptNotFound(_)));
    }

    #[test]
    fn auth_method_label_is_stable() {
        assert_eq!(
            AuthMethod::ApiKey {
                env_var: "X".into()
            }
            .label(),
            "API key"
        );
    }

    #[test]
    fn connection_status_summary() {
        assert_eq!(ConnectionStatus::NotConfigured.summary(), "not configured");
        assert_eq!(
            ConnectionStatus::InlineEnv {
                env_var: "FOO".into()
            }
            .summary(),
            "env:FOO"
        );
    }
}
