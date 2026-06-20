//! In-memory [`CredentialService`] implementation.
//!
//! Used for tests and the Phase 0 boot path. State is lost on restart.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::credential::{Credential, CredentialError, CredentialId, CredentialService};
use crate::types::ProviderId;

/// Test/in-memory credential store. Thread-safe via [`tokio::sync::RwLock`].
#[derive(Clone, Default)]
pub struct InMemoryCredentialStore {
    inner: Arc<RwLock<HashMap<CredentialId, Credential>>>,
}

impl InMemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CredentialService for InMemoryCredentialStore {
    async fn upsert(&self, cred: Credential) -> Result<CredentialId, CredentialError> {
        let mut map = self.inner.write().await;
        // If a credential already exists for (provider, label), remove it
        // first so that lookup is unique per (provider, label). This matches
        // opencode's "deletes old, inserts new" transactional behavior.
        let key = (cred.provider.clone(), cred.label.clone());
        map.retain(|_, existing| !(existing.provider == key.0 && existing.label == key.1));
        let id = cred.id.clone();
        map.insert(id.clone(), cred);
        Ok(id)
    }

    async fn list(&self, provider: &ProviderId) -> Result<Vec<Credential>, CredentialError> {
        let map = self.inner.read().await;
        Ok(map
            .values()
            .filter(|c| &c.provider == provider)
            .cloned()
            .collect())
    }

    async fn get(&self, id: &CredentialId) -> Result<Credential, CredentialError> {
        let map = self.inner.read().await;
        map.get(id)
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(id.clone()))
    }

    async fn delete(&self, id: &CredentialId) -> Result<(), CredentialError> {
        let mut map = self.inner.write().await;
        map.remove(id);
        Ok(())
    }

    async fn delete_all(&self, provider: &ProviderId) -> Result<usize, CredentialError> {
        let mut map = self.inner.write().await;
        let before = map.len();
        map.retain(|_, c| &c.provider != provider);
        Ok(before - map.len())
    }

    async fn count(&self) -> Result<usize, CredentialError> {
        Ok(self.inner.read().await.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::CredentialType;

    fn cred(provider: &str, label: &str, key: &str) -> Credential {
        Credential::new(
            provider.into(),
            label,
            CredentialType::ApiKey { key: key.into() },
        )
    }

    #[tokio::test]
    async fn upsert_then_get() {
        let store = InMemoryCredentialStore::new();
        let id = store
            .upsert(cred("anthropic", "work", "sk-x"))
            .await
            .unwrap();
        let got = store.get(&id).await.unwrap();
        assert_eq!(got.label, "work");
    }

    #[tokio::test]
    async fn upsert_replaces_same_label() {
        let store = InMemoryCredentialStore::new();
        let id1 = store
            .upsert(cred("anthropic", "work", "sk-1"))
            .await
            .unwrap();
        let _id2 = store
            .upsert(cred("anthropic", "work", "sk-2"))
            .await
            .unwrap();
        let all = store.list(&"anthropic".into()).await.unwrap();
        assert_eq!(all.len(), 1, "same label should be replaced");
        // id1 is now gone; the surviving credential is the latest one.
        let err = store.get(&id1).await.unwrap_err();
        assert!(matches!(err, CredentialError::NotFound(_)));
        let got = all[0].clone();
        match got.credential {
            CredentialType::ApiKey { key } => assert_eq!(key, "sk-2"),
            _ => panic!("expected API key"),
        }
    }

    #[tokio::test]
    async fn list_isolated_per_provider() {
        let store = InMemoryCredentialStore::new();
        store.upsert(cred("anthropic", "a", "1")).await.unwrap();
        store.upsert(cred("openai", "b", "2")).await.unwrap();
        let a = store.list(&"anthropic".into()).await.unwrap();
        let o = store.list(&"openai".into()).await.unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(o.len(), 1);
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let store = InMemoryCredentialStore::new();
        store.delete(&"missing".into()).await.unwrap();
    }

    #[tokio::test]
    async fn delete_all_only_targets_provider() {
        let store = InMemoryCredentialStore::new();
        store.upsert(cred("anthropic", "a", "1")).await.unwrap();
        store.upsert(cred("anthropic", "b", "2")).await.unwrap();
        store.upsert(cred("openai", "c", "3")).await.unwrap();
        let removed = store.delete_all(&"anthropic".into()).await.unwrap();
        assert_eq!(removed, 2);
        let remaining = store.count().await.unwrap();
        assert_eq!(remaining, 1);
    }
}
