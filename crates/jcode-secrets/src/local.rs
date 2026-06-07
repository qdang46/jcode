//! LocalSecretsBackend: age-encrypted file backed by OS keychain passphrase.
//!
//! Stores all secrets in a single `{jcode_home}/secrets/local.age` file that
//! is encrypted with the `age` crate using a scrypt passphrase. The passphrase
//! itself is stored in the OS keychain via the `KeyringStore` trait.
//!
//! Reference: codex `codex-rs/secrets/src/local.rs`

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use jcode_keyring_store::KeyringStore;

use super::{SecretListEntry, SecretName, SecretScope, SecretsBackend, SERVICE_NAME, PASS_ACCOUNT};

const SECRETS_RELATIVE_PATH: &str = "secrets/local.age";
const FILE_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct SecretsFile {
    version: u32,
    secrets: BTreeMap<String, String>,
}

/// Backend that stores secrets in an age-encrypted JSON file.
///
/// The encryption passphrase is managed by the OS keychain — it is
/// auto-generated on first save and transparently loaded thereafter.
pub struct LocalSecretsBackend {
    secrets_path: PathBuf,
    keyring_store: Arc<dyn KeyringStore>,
}

impl LocalSecretsBackend {
    /// Create a new backend backed by `{jcode_home}/secrets/local.age`.
    pub fn new(jcode_home: PathBuf, keyring_store: Arc<dyn KeyringStore>) -> Self {
        let secrets_path = jcode_home.join(SECRETS_RELATIVE_PATH);
        Self {
            secrets_path,
            keyring_store,
        }
    }

    /// Read, decrypt, and parse the secrets file.
    ///
    /// Returns an empty map when the file does not exist yet.
    fn load_secrets(&self) -> Result<BTreeMap<String, String>> {
        if !self.secrets_path.exists() {
            return Ok(BTreeMap::new());
        }

        // Load passphrase from OS keychain
        let passphrase = self
            .keyring_store
            .load(SERVICE_NAME, PASS_ACCOUNT)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No passphrase found in OS keychain for jcode-secrets.\n\
                     Run `jcode secrets init` to set up the encrypted store."
                )
            })?;

        let encrypted = std::fs::read(&self.secrets_path)
            .with_context(|| format!("Failed to read {}", self.secrets_path.display()))?;

        // Decrypt with age scrypt
        let decryptor = age::Decryptor::new(&encrypted[..])
            .context("Failed to create age decryptor")?;
        let mut decrypted = Vec::new();
        let pass = age::secrecy::SecretString::from(passphrase);
        let identity = age::scrypt::Identity::new(pass);
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .context("Failed to decrypt secrets file")?;
        std::io::Read::read_to_end(&mut reader, &mut decrypted)
            .context("Failed to read decrypted content")?;

        let file: SecretsFile =
            serde_json::from_slice(&decrypted).context("Failed to parse secrets file")?;

        Ok(file.secrets)
    }

    /// Serialize, encrypt, and atomically write the secrets file.
    ///
    /// If the passphrase does not exist in the keychain yet, it is
    /// auto-generated and saved.
    fn save_secrets(&self, secrets: &BTreeMap<String, String>) -> Result<()> {
        // Ensure parent dir exists
        if let Some(parent) = self.secrets_path.parent() {
            jcode_storage::ensure_dir(parent)?;
        }

        // Get or create passphrase
        let passphrase = match self.keyring_store.load(SERVICE_NAME, PASS_ACCOUNT)? {
            Some(p) => p,
            None => {
                let new_pass = super::generate_passphrase();
                self.keyring_store
                    .save(SERVICE_NAME, PASS_ACCOUNT, &new_pass)?;
                new_pass
            }
        };

        let file = SecretsFile {
            version: FILE_VERSION,
            secrets: secrets.clone(),
        };
        let json = serde_json::to_vec(&file)?;

        // Encrypt with age scrypt (streaming API)
        let pass = age::secrecy::SecretString::from(passphrase);
        let encryptor = age::Encryptor::with_user_passphrase(pass);
        let mut encrypted = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut encrypted)
            .context("Failed to wrap age output")?;
        std::io::Write::write_all(&mut writer, &json)
            .context("Failed to write encrypted data")?;
        writer.finish().context("Failed to finish age encryption")?;

        // Atomic, owner-only (0600) write: a crash mid-write cannot truncate or
        // corrupt the existing encrypted store (temp file + fsync + rename, with
        // a .bak fallback retained by jcode-storage).
        jcode_storage::write_bytes_secret(&self.secrets_path, &encrypted)?;
        Ok(())
    }

    // ─── Public backend methods ───────────────────────────────────────────

    pub fn set(&self, scope: &SecretScope, name: &SecretName, value: &str) -> Result<()> {
        let mut secrets = self.load_secrets()?;
        let key = scope.canonical_key(name);
        secrets.insert(key, value.to_string());
        self.save_secrets(&secrets)
    }

    pub fn get(&self, scope: &SecretScope, name: &SecretName) -> Result<Option<String>> {
        let secrets = self.load_secrets()?;
        let key = scope.canonical_key(name);
        Ok(secrets.get(&key).cloned())
    }

    pub fn delete(&self, scope: &SecretScope, name: &SecretName) -> Result<bool> {
        let mut secrets = self.load_secrets()?;
        let key = scope.canonical_key(name);
        let existed = secrets.remove(&key).is_some();
        if existed {
            self.save_secrets(&secrets)?;
        }
        Ok(existed)
    }

    pub fn list(&self, scope_filter: Option<&SecretScope>) -> Result<Vec<SecretListEntry>> {
        let secrets = self.load_secrets()?;
        let mut entries = Vec::new();
        for key in secrets.keys() {
            if let Some(entry) = parse_canonical_key(key) {
                let matches = scope_filter.map_or(true, |filter| {
                    entry.scope == *filter
                });
                if matches {
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }
}

impl SecretsBackend for LocalSecretsBackend {
    fn set(&self, scope: &SecretScope, name: &SecretName, value: &str) -> Result<()> {
        self.set(scope, name, value)
    }
    fn get(&self, scope: &SecretScope, name: &SecretName) -> Result<Option<String>> {
        self.get(scope, name)
    }
    fn delete(&self, scope: &SecretScope, name: &SecretName) -> Result<bool> {
        self.delete(scope, name)
    }
    fn list(&self, scope_filter: Option<&SecretScope>) -> Result<Vec<SecretListEntry>> {
        self.list(scope_filter)
    }

    fn initialize(&self) -> Result<()> {
        // Re-saving the current contents creates the keychain passphrase and
        // the encrypted file when absent. load_secrets() propagates a real
        // decrypt error rather than clobbering an existing store with empties.
        let secrets = self.load_secrets()?;
        self.save_secrets(&secrets)
    }

    fn purge(&self) -> Result<()> {
        // Remove the encrypted store file if present.
        if self.secrets_path.exists() {
            std::fs::remove_file(&self.secrets_path).with_context(|| {
                format!("Failed to remove {}", self.secrets_path.display())
            })?;
        }
        // Remove the OS keychain passphrase entry (idempotent).
        self.keyring_store.delete(SERVICE_NAME, PASS_ACCOUNT)?;
        Ok(())
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Parse a canonical key string back into a `SecretListEntry`.
///
/// Expected formats:
/// - `"global/SECRET_NAME"` → `SecretScope::Global`
/// - `"env/{env_id}/SECRET_NAME"` → `SecretScope::Environment(env_id)`
fn parse_canonical_key(key: &str) -> Option<SecretListEntry> {
    if let Some(name) = key.strip_prefix("global/") {
        Some(SecretListEntry {
            scope: SecretScope::Global,
            name: SecretName::new(name).ok()?,
        })
    } else if let Some(rest) = key.strip_prefix("env/") {
        let (env_id, name) = rest.split_once('/')?;
        Some(SecretListEntry {
            scope: SecretScope::environment(env_id.to_string()).ok()?,
            name: SecretName::new(name).ok()?,
        })
    } else {
        None
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_keyring_store::MockKeyringStore;

    fn test_backend() -> (LocalSecretsBackend, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let keyring = Arc::new(MockKeyringStore::new()) as Arc<dyn KeyringStore>;
        let backend = LocalSecretsBackend::new(dir.path().to_path_buf(), keyring);
        (backend, dir)
    }

    #[test]
    fn test_initialize_creates_store() {
        let (backend, _dir) = test_backend();
        assert!(!backend.secrets_path.exists());
        backend.initialize().unwrap();
        assert!(backend.secrets_path.exists());
        // An initialized store is empty but readable.
        assert!(backend.list(None).unwrap().is_empty());
    }

    #[test]
    fn test_purge_removes_file_and_secrets() {
        let (backend, _dir) = test_backend();
        let scope = SecretScope::Global;
        backend
            .set(&scope, &SecretName::new("K1").unwrap(), "v1")
            .unwrap();
        assert!(backend.secrets_path.exists());

        backend.purge().unwrap();
        assert!(!backend.secrets_path.exists());
        // After purge the store reads as empty again.
        assert!(backend.list(None).unwrap().is_empty());
        // Purge is idempotent.
        backend.purge().unwrap();
    }

    #[test]
    fn test_set_and_get_global() {
        let (backend, _dir) = test_backend();
        let scope = SecretScope::Global;
        let name = SecretName::new("GITHUB_TOKEN").unwrap();

        backend.set(&scope, &name, "ghp_abc123").unwrap();
        let value = backend.get(&scope, &name).unwrap();
        assert_eq!(value, Some("ghp_abc123".to_string()));
    }

    #[test]
    fn test_set_and_get_environment() {
        let (backend, _dir) = test_backend();
        let scope = SecretScope::environment("my-repo".to_string()).unwrap();
        let name = SecretName::new("OPENAI_API_KEY").unwrap();

        backend.set(&scope, &name, "sk-xxx").unwrap();
        let value = backend.get(&scope, &name).unwrap();
        assert_eq!(value, Some("sk-xxx".to_string()));

        // Global scope should NOT see env-scoped secret
        let global_value = backend.get(&SecretScope::Global, &name).unwrap();
        assert_eq!(global_value, None);
    }

    #[test]
    fn test_delete() {
        let (backend, _dir) = test_backend();
        let scope = SecretScope::Global;
        let name = SecretName::new("TEST_KEY").unwrap();

        backend.set(&scope, &name, "value").unwrap();
        assert!(backend.delete(&scope, &name).unwrap());
        // Second delete should return false (already gone)
        assert!(!backend.delete(&scope, &name).unwrap());
        assert_eq!(backend.get(&scope, &name).unwrap(), None);
    }

    #[test]
    fn test_list_with_filter() {
        let (backend, _dir) = test_backend();
        let global = SecretScope::Global;
        let env = SecretScope::environment("proj".to_string()).unwrap();

        backend
            .set(&global, &SecretName::new("GLOBAL_SECRET").unwrap(), "val1")
            .unwrap();
        backend
            .set(&env, &SecretName::new("ENV_SECRET").unwrap(), "val2")
            .unwrap();

        let all = backend.list(None).unwrap();
        assert_eq!(all.len(), 2);

        let globals = backend.list(Some(&global)).unwrap();
        assert_eq!(globals.len(), 1);

        let envs = backend.list(Some(&env)).unwrap();
        assert_eq!(envs.len(), 1);
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let keyring = Arc::new(MockKeyringStore::new()) as Arc<dyn KeyringStore>;

        let scope = SecretScope::Global;
        let name = SecretName::new("PERSISTENT_KEY").unwrap();

        // First instance: set a secret
        {
            let backend = LocalSecretsBackend::new(dir.path().to_path_buf(), keyring.clone());
            backend.set(&scope, &name, "stored-value").unwrap();
        }

        // Second instance on same dir: must read the file
        {
            let backend2 = LocalSecretsBackend::new(dir.path().to_path_buf(), keyring);
            let value = backend2.get(&scope, &name).unwrap();
            assert_eq!(value, Some("stored-value".to_string()));
        }
    }

    #[test]
    fn test_empty_store() {
        let (backend, _dir) = test_backend();
        let entries = backend.list(None).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_overwrite_existing() {
        let (backend, _dir) = test_backend();
        let scope = SecretScope::Global;
        let name = SecretName::new("MY_KEY").unwrap();

        backend.set(&scope, &name, "v1").unwrap();
        backend.set(&scope, &name, "v2").unwrap();
        let value = backend.get(&scope, &name).unwrap();
        assert_eq!(value, Some("v2".to_string()));
    }

    #[test]
    fn test_parse_canonical_key_global() {
        let entry = parse_canonical_key("global/MY_KEY").unwrap();
        assert!(matches!(entry.scope, SecretScope::Global));
        assert_eq!(entry.name.as_str(), "MY_KEY");
    }

    #[test]
    fn test_parse_canonical_key_environment() {
        let entry = parse_canonical_key("env/my-repo/MY_KEY").unwrap();
        assert!(matches!(entry.scope, SecretScope::Environment(_)));
        assert!(format!("{}", entry.scope).contains("my-repo"));
        assert_eq!(entry.name.as_str(), "MY_KEY");
    }
}
