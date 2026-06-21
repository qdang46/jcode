//! OS-keychain-backed [`CredentialService`] implementation.
//!
//! Persists each [`Credential`] as a JSON blob in the platform-native
//! keychain via [`jcode_keyring_store::KeyringStore`]. The on-disk format
//! is just the `serde_json` representation of [`Credential`], so adding
//! new fields to the struct is backward-compatible (older entries will
//! deserialize as long as the unknown fields are optional or default).
//!
//! Service name: `jcode-provider-service`.
//! Account name: the credential id, prefixed with `cred:` for grep-ability
//! in the keychain CLI (`security find-generic-password -s jcode-provider-service`).

use std::sync::Arc;

use async_trait::async_trait;
use jcode_keyring_store::KeyringStore;

use crate::credential::{Credential, CredentialError, CredentialId, CredentialService};
use crate::types::ProviderId;

const SERVICE: &str = "jcode-provider-service";
const ACCOUNT_PREFIX: &str = "cred:";

fn account(id: &CredentialId) -> String {
    format!("{}{}", ACCOUNT_PREFIX, id.as_str())
}

#[allow(dead_code)]
fn id_from_account(account: &str) -> Option<CredentialId> {
    account
        .strip_prefix(ACCOUNT_PREFIX)
        .and_then(|s| CredentialId::new(s).ok())
}

/// Persistent credential store backed by an OS keychain.
///
/// `K` is the concrete [`KeyringStore`] (typically
/// [`jcode_keyring_store::DefaultKeyringStore`] in production and
/// [`jcode_keyring_store::MockKeyringStore`] in tests).
pub struct KeyringCredentialStore<K: KeyringStore + 'static> {
    keyring: Arc<K>,
}

impl<K: KeyringStore + 'static> KeyringCredentialStore<K> {
    pub fn new(keyring: Arc<K>) -> Self {
        Self { keyring }
    }

    fn list_existing_ids(&self) -> Result<Vec<CredentialId>, CredentialError> {
        // We don't have a "list all accounts" primitive on the KeyringStore
        // trait, so the index lives in a single well-known key
        // (`__index__`) holding a `Vec<String>` of ids.
        let raw = self
            .keyring
            .load(SERVICE, "__index__")
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        match raw {
            None => Ok(Vec::new()),
            Some(s) => serde_json::from_str(&s)
                .map_err(|e| CredentialError::Storage(format!("corrupt credential index: {}", e))),
        }
    }

    fn write_index(&self, ids: &[CredentialId]) -> Result<(), CredentialError> {
        let raw =
            serde_json::to_string(ids).map_err(|e| CredentialError::Storage(e.to_string()))?;
        self.keyring
            .save(SERVICE, "__index__", &raw)
            .map_err(|e| CredentialError::Storage(e.to_string()))
    }
}

#[async_trait]
impl<K: KeyringStore + 'static> CredentialService for KeyringCredentialStore<K> {
    async fn upsert(&self, cred: Credential) -> Result<CredentialId, CredentialError> {
        // Drop any prior credential with the same (provider, label).
        let existing_ids = self.list_existing_ids()?;
        for id in &existing_ids {
            if let Ok(existing) = self.get(id).await
                && existing.provider == cred.provider
                && existing.label == cred.label
            {
                self.delete(id).await?;
            }
        }

        let raw =
            serde_json::to_string(&cred).map_err(|e| CredentialError::Storage(e.to_string()))?;
        self.keyring
            .save(SERVICE, &account(&cred.id), &raw)
            .map_err(|e| CredentialError::Storage(e.to_string()))?;

        // Add to the index if missing.
        let mut index = existing_ids;
        if !index.iter().any(|i| i == &cred.id) {
            index.push(cred.id.clone());
            self.write_index(&index)?;
        }

        Ok(cred.id)
    }

    async fn list(&self, provider: &ProviderId) -> Result<Vec<Credential>, CredentialError> {
        let ids = self.list_existing_ids()?;
        let mut out = Vec::new();
        for id in ids {
            match self.get(&id).await {
                Ok(c) if &c.provider == provider => out.push(c),
                Ok(_) => {}
                Err(_) => continue, // skip broken entries
            }
        }
        Ok(out)
    }

    async fn get(&self, id: &CredentialId) -> Result<Credential, CredentialError> {
        let raw = self
            .keyring
            .load(SERVICE, &account(id))
            .map_err(|e| CredentialError::Storage(e.to_string()))?
            .ok_or_else(|| CredentialError::NotFound(id.clone()))?;
        serde_json::from_str(&raw)
            .map_err(|e| CredentialError::Invalid(format!("malformed credential {}: {}", id, e)))
    }

    async fn delete(&self, id: &CredentialId) -> Result<(), CredentialError> {
        self.keyring
            .delete(SERVICE, &account(id))
            .map_err(|e| CredentialError::Storage(e.to_string()))?;
        let mut index = self.list_existing_ids()?;
        index.retain(|i| i != id);
        self.write_index(&index)?;
        Ok(())
    }

    async fn delete_all(&self, provider: &ProviderId) -> Result<usize, CredentialError> {
        let ids = self.list_existing_ids()?;
        let mut removed = 0;
        for id in ids {
            if let Ok(c) = self.get(&id).await
                && &c.provider == provider
            {
                self.delete(&id).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    async fn count(&self) -> Result<usize, CredentialError> {
        Ok(self.list_existing_ids()?.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::CredentialType;
    use jcode_keyring_store::MockKeyringStore;

    fn store() -> KeyringCredentialStore<MockKeyringStore> {
        KeyringCredentialStore::new(Arc::new(MockKeyringStore::new()))
    }

    fn cred(provider: &str, label: &str, key: &str) -> Credential {
        Credential::new(
            provider.into(),
            label,
            CredentialType::ApiKey { key: key.into() },
        )
    }

    #[tokio::test]
    async fn upsert_and_get_roundtrip() {
        let s = store();
        let id = s.upsert(cred("anthropic", "work", "sk-x")).await.unwrap();
        let got = s.get(&id).await.unwrap();
        assert_eq!(got.label, "work");
        assert_eq!(s.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn upsert_replaces_same_label() {
        let s = store();
        let id1 = s.upsert(cred("anthropic", "work", "sk-1")).await.unwrap();
        let _ = s.upsert(cred("anthropic", "work", "sk-2")).await.unwrap();
        let all = s.list(&"anthropic".into()).await.unwrap();
        assert_eq!(all.len(), 1);
        // id1 was replaced; it should no longer resolve.
        let err = s.get(&id1).await.unwrap_err();
        assert!(matches!(err, CredentialError::NotFound(_)));
        let got = all[0].clone();
        match got.credential {
            CredentialType::ApiKey { key } => assert_eq!(key, "sk-2"),
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn list_filters_by_provider() {
        let s = store();
        s.upsert(cred("anthropic", "a", "1")).await.unwrap();
        s.upsert(cred("openai", "b", "2")).await.unwrap();
        let a = s.list(&"anthropic".into()).await.unwrap();
        let o = s.list(&"openai".into()).await.unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(o.len(), 1);
    }

    #[tokio::test]
    async fn delete_removes_from_index() {
        let s = store();
        let id = s.upsert(cred("anthropic", "work", "1")).await.unwrap();
        assert_eq!(s.count().await.unwrap(), 1);
        s.delete(&id).await.unwrap();
        assert_eq!(s.count().await.unwrap(), 0);
        let err = s.get(&id).await.unwrap_err();
        assert!(matches!(err, CredentialError::NotFound(_)));
    }

    #[tokio::test]
    async fn delete_all_only_targets_provider() {
        let s = store();
        s.upsert(cred("anthropic", "a", "1")).await.unwrap();
        s.upsert(cred("anthropic", "b", "2")).await.unwrap();
        s.upsert(cred("openai", "c", "3")).await.unwrap();
        let removed = s.delete_all(&"anthropic".into()).await.unwrap();
        assert_eq!(removed, 2);
        assert_eq!(s.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn empty_index_loads_as_empty_list() {
        let s = store();
        assert_eq!(s.count().await.unwrap(), 0);
        let all = s.list(&"anthropic".into()).await.unwrap();
        assert!(all.is_empty());
    }
}
