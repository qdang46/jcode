//! Persistent [`IntegrationService`] implementation.
//!
//! Wraps the in-memory provider registry and OAuth attempt state with a
//! [`CredentialService`] so that completed OAuth flows and `save_api_key`
//! calls actually persist credentials (per the plan's Phase 1 + 2a
//! quick wins).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jcode_keyring_store::KeyringStore;
use tokio::sync::RwLock;

use crate::credential::{
    Credential, CredentialId, CredentialService, CredentialType,
};
use crate::attempt::OAuthAttempt;
use crate::integration::{
    AuthMethod, ConnectionStatus, IntegrationError, IntegrationService, LoginProvider,
};
use crate::types::ProviderId;

const OAUTH_LABEL: &str = "oauth";
const ATTEMPT_INDEX_KEY: &str = "__oauth_attempts__";
const ATTEMPT_INDEX_SERVICE: &str = "jcode-provider-service";
const ATTEMPT_PREFIX: &str = "oauth-attempt:";

/// Integration service backed by a [`CredentialService`] for persistence
/// of completed OAuth flows and API keys.
///
/// OAuth *attempts* (in-flight login flows) are kept in memory because
/// they have a 10-minute TTL and never need to survive a restart; only
/// the final credentials do.
pub struct PersistentIntegration<K: KeyringStore + 'static> {
    providers: RwLock<HashMap<ProviderId, LoginProvider>>,
    attempts: RwLock<HashMap<String, OAuthAttempt>>,
    credentials: Arc<dyn CredentialService>,
    _phantom: std::marker::PhantomData<K>,
}

impl<K: KeyringStore + 'static> PersistentIntegration<K> {
    pub fn new(credentials: Arc<dyn CredentialService>) -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
            attempts: RwLock::new(HashMap::new()),
            credentials,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<K: KeyringStore + 'static> IntegrationService for PersistentIntegration<K> {
    async fn register(&self, provider: LoginProvider) -> Result<(), IntegrationError> {
        let mut map = self.providers.write().await;
        map.insert(provider.id.clone(), provider);
        Ok(())
    }

    async fn get(&self, id: &ProviderId) -> Result<LoginProvider, IntegrationError> {
        let map = self.providers.read().await;
        map.get(id)
            .cloned()
            .ok_or_else(|| IntegrationError::UnknownProvider(id.clone()))
    }

    async fn list(&self) -> Result<Vec<LoginProvider>, IntegrationError> {
        let map = self.providers.read().await;
        Ok(map.values().cloned().collect())
    }

    async fn detect(&self, id: &ProviderId) -> Result<ConnectionStatus, IntegrationError> {
        // Verify the provider is registered.
        let provider = self.get(id).await?;

        // 1. Check for an inline env-var credential.
        for method in &provider.auth_methods {
            if let Some(env_var) = env_var_for(method) {
                if std::env::var(&env_var).is_ok() {
                    return Ok(ConnectionStatus::InlineEnv { env_var });
                }
            }
        }

        // 2. Check the credential store.
        let creds = self
            .credentials
            .list(id)
            .await
            .map_err(|e| IntegrationError::Storage(e.to_string()))?;
        if let Some(cred) = creds.first() {
            return Ok(match &cred.credential {
                CredentialType::ApiKey { .. } => ConnectionStatus::ApiKey {
                    credential_id: cred.id.clone(),
                    label: cred.label.clone(),
                },
                CredentialType::OAuth { expires_at, .. } => ConnectionStatus::OAuth {
                    credential_id: cred.id.clone(),
                    label: cred.label.clone(),
                    expires_at: *expires_at,
                },
                CredentialType::ExternalCommand { .. } => ConnectionStatus::ApiKey {
                    credential_id: cred.id.clone(),
                    label: cred.label.clone(),
                },
            });
        }

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
        let attempt = OAuthAttempt::new(id.clone(), method, chrono::Duration::minutes(10));
        self.attempts
            .write()
            .await
            .insert(attempt.id.clone(), attempt.clone());
        // Also persist to the keychain so the attempt can be looked up by
        // the OAuth callback handler in a separate process.
        if let Some(store) = keyring_store_attempts::<K>() {
            let raw = serde_json::to_string(&attempt).unwrap_or_default();
            let _ = store.save(ATTEMPT_INDEX_SERVICE,
                &format!("{}{}", ATTEMPT_PREFIX, attempt.id), &raw);
        }
        Ok(attempt)
    }

    async fn get_oauth_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<OAuthAttempt, IntegrationError> {
        let map = self.attempts.read().await;
        map.get(attempt_id)
            .cloned()
            .ok_or_else(|| IntegrationError::OAuthAttemptNotFound(attempt_id.to_string()))
    }

    async fn complete_oauth(
        &self,
        attempt_id: &str,
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<CredentialId, IntegrationError> {
        let attempt = self.get_oauth_attempt(attempt_id).await?;
        if attempt.is_expired() {
            self.attempts.write().await.remove(attempt_id);
            return Err(IntegrationError::OAuthAttemptExpired);
        }
        let mut cred = Credential::new(
            attempt.provider.clone(),
            OAUTH_LABEL,
            CredentialType::OAuth {
                access_token,
                refresh_token,
                expires_at,
            },
        );
        cred.touch();
        let id = self
            .credentials
            .upsert(cred)
            .await
            .map_err(|e| IntegrationError::Storage(e.to_string()))?;
        self.attempts.write().await.remove(attempt_id);
        Ok(id)
    }

    async fn cancel_oauth(&self, attempt_id: &str) -> Result<(), IntegrationError> {
        self.attempts.write().await.remove(attempt_id);
        Ok(())
    }

    async fn save_api_key(
        &self,
        id: &ProviderId,
        label: &str,
        key: &str,
    ) -> Result<CredentialId, IntegrationError> {
        // Validate the provider is known.
        let _ = self.get(id).await?;
        let cred = Credential::new(
            id.clone(),
            label,
            CredentialType::ApiKey {
                key: key.to_string(),
            },
        );
        self.credentials
            .upsert(cred)
            .await
            .map_err(|e| IntegrationError::Storage(e.to_string()))
    }
}

fn env_var_for(method: &AuthMethod) -> Option<String> {
    match method {
        AuthMethod::ApiKey { env_var }
        | AuthMethod::BearerEnv { env_var }
        | AuthMethod::CustomHeader { env_var, .. } => Some(env_var.clone()),
        AuthMethod::OAuth { .. } => None,
    }
}

fn keyring_store_attempts<K: KeyringStore + 'static>() -> Option<Arc<K>> {
    // Hook for future use: expose the underlying keyring to persist
    // in-flight OAuth attempts. We don't have access to the concrete
    // keyring here without adding a trait object, so this is a no-op
    // stub for Phase 2a. Phase 2b can plumb the keyring through.
    let _ = (ATTEMPT_INDEX_KEY, std::marker::PhantomData::<K>);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{in_memory::InMemoryCredentialStore, keyring::KeyringCredentialStore};
    use jcode_keyring_store::MockKeyringStore;

    fn anthropic() -> LoginProvider {
        LoginProvider {
            id: "anthropic".into(),
            label: "Anthropic".into(),
            auth_methods: vec![
                AuthMethod::OAuth {
                    authorization_url: "https://claude.ai/oauth/authorize".into(),
                },
                AuthMethod::ApiKey {
                    env_var: "JCODE_TEST_ANTHROPIC_KEY".into(),
                },
            ],
            env_keys: vec!["JCODE_TEST_ANTHROPIC_KEY".into()],
            oauth_preferred: true,
        }
    }

    fn make_svc() -> PersistentIntegration<MockKeyringStore> {
        let keyring = Arc::new(MockKeyringStore::new());
        let creds: Arc<dyn CredentialService> = Arc::new(KeyringCredentialStore::new(keyring));
        PersistentIntegration::new(creds)
    }

    #[tokio::test]
    async fn save_api_key_persists_to_credential_store() {
        let svc = make_svc();
        svc.register(anthropic()).await.unwrap();
        let id = svc
            .save_api_key(&"anthropic".into(), "work", "sk-secret")
            .await
            .unwrap();
        let status = svc.detect(&"anthropic".into()).await.unwrap();
        match status {
            ConnectionStatus::ApiKey {
                credential_id,
                label,
            } => {
                assert_eq!(credential_id, id);
                assert_eq!(label, "work");
            }
            other => panic!("expected ApiKey, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn complete_oauth_persists_and_clears_attempt() {
        let svc = make_svc();
        svc.register(anthropic()).await.unwrap();
        let attempt = svc.start_oauth(&"anthropic".into()).await.unwrap();
        let id = svc
            .complete_oauth(
                &attempt.id,
                "access-tok".into(),
                Some("refresh-tok".into()),
                Some(Utc::now() + chrono::Duration::hours(1)),
            )
            .await
            .unwrap();
        // Attempt is cleared.
        let err = svc.get_oauth_attempt(&attempt.id).await.unwrap_err();
        assert!(matches!(err, IntegrationError::OAuthAttemptNotFound(_)));
        // Credential is persisted.
        let status = svc.detect(&"anthropic".into()).await.unwrap();
        match status {
            ConnectionStatus::OAuth { credential_id, .. } => assert_eq!(credential_id, id),
            other => panic!("expected OAuth, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn detect_falls_through_to_not_configured() {
        let svc = make_svc();
        svc.register(anthropic()).await.unwrap();
        let status = svc.detect(&"anthropic".into()).await.unwrap();
        assert_eq!(status, ConnectionStatus::NotConfigured);
    }

    #[tokio::test]
    async fn start_oauth_fails_for_provider_without_oauth() {
        let mut p = anthropic();
        p.auth_methods
            .retain(|m| !matches!(m, AuthMethod::OAuth { .. }));
        let svc = make_svc();
        svc.register(p).await.unwrap();
        let err = svc.start_oauth(&"anthropic".into()).await.unwrap_err();
        assert!(matches!(err, IntegrationError::UnsupportedAuth(_)));
    }

    #[tokio::test]
    async fn detect_picks_up_inline_env() {
        let mut p = anthropic();
        p.auth_methods = vec![AuthMethod::ApiKey {
            env_var: "JCODE_PERSISTENT_TEST_ENV_KEY".into(),
        }];
        let svc = make_svc();
        svc.register(p).await.unwrap();
        // SAFETY: test-only env mutation.
        unsafe { std::env::set_var("JCODE_PERSISTENT_TEST_ENV_KEY", "from-env") };
        let status = svc.detect(&"anthropic".into()).await.unwrap();
        unsafe { std::env::remove_var("JCODE_PERSISTENT_TEST_ENV_KEY") };
        match status {
            ConnectionStatus::InlineEnv { env_var } => {
                assert_eq!(env_var, "JCODE_PERSISTENT_TEST_ENV_KEY")
            }
            other => panic!("expected InlineEnv, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn works_with_in_memory_credential_store() {
        let creds: Arc<dyn CredentialService> = Arc::new(InMemoryCredentialStore::new());
        let svc: PersistentIntegration<MockKeyringStore> =
            PersistentIntegration::new(creds);
        svc.register(anthropic()).await.unwrap();
        let _id = svc
            .save_api_key(&"anthropic".into(), "default", "sk-x")
            .await
            .unwrap();
    }
}
