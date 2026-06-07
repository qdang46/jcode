//! Centralized secrets management with OS keychain-backed encryption.
//!
//! Architecture (following OpenAI codex `codex-rs/secrets/`):
//!
//! ```text
//!   SecretsBackend (trait) ← LocalSecretsBackend (age-encrypted file)
//!   SecretsManager wraps Arc<dyn SecretsBackend>
//!   KeyringStore (trait in jcode-keyring-store) ← DefaultKeyringStore | MockKeyringStore
//! ```
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use jcode_secrets::{SecretsManager, SecretsBackendKind, SecretScope, SecretName};
//! use std::path::PathBuf;
//!
//! let manager = SecretsManager::new(
//!     PathBuf::from("/path/to/jcode_home"),
//!     SecretsBackendKind::Local,
//! ).expect("create secrets manager");
//!
//! let name = SecretName::new("GITHUB_TOKEN").unwrap();
//! let scope = SecretScope::Global;
//!
//! manager.set(&scope, &name, "ghp_abc123").unwrap();
//! let value = manager.get(&scope, &name).unwrap();
//! assert_eq!(value, Some("ghp_abc123".to_string()));
//! ```

mod local;
pub use local::LocalSecretsBackend;
pub use jcode_redact::redact_secrets;

mod resolver;
pub use resolver::{current_manager, secrets_api_key_resolver};

use anyhow::{Context, Result};
use jcode_keyring_store::KeyringStore;
use base64::Engine;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ─── SecretName ─────────────────────────────────────────────────────────────

const MAX_SECRET_NAME_LENGTH: usize = 128;

/// A validated secret name matching `[A-Z0-9_]+`.
///
/// Names are case-sensitive, uppercase-only to avoid ambiguity between
/// provider conventions (e.g. `GITHUB_TOKEN` vs `github_token`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SecretName(String);

impl SecretName {
    /// Validate and construct a new `SecretName`.
    ///
    /// # Errors
    ///
    /// - Empty name
    /// - Name longer than 128 characters
    /// - Name contains characters outside `[A-Z0-9_]`
    pub fn new(name: &str) -> Result<Self> {
        if name.is_empty() {
            anyhow::bail!("Secret name must not be empty");
        }
        if name.len() > MAX_SECRET_NAME_LENGTH {
            anyhow::bail!("Secret name too long (max {})", MAX_SECRET_NAME_LENGTH);
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        {
            anyhow::bail!("Secret name must match [A-Z0-9_]+, got: {}", name);
        }
        Ok(Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ─── EnvId ──────────────────────────────────────────────────────────────────

/// An environment identifier, derived from a git repo root or a cwd hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvId(String);

impl fmt::Display for EnvId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Derive an environment ID from the current working directory.
///
/// Strategy:
/// 1. Git repo: `<sanitized-repo-name>-<6 hex>` where the hex is a hash of the
///    canonicalized repo-root path. The path hash is a discriminator so two
///    unrelated repos that happen to share a basename (e.g. `~/a/acme` and
///    `~/b/acme`) do not collapse to the same environment and bleed secrets.
/// 2. Fallback (no git): `cwd-<12 hex>` of the canonicalized cwd.
///
/// The repo name is sanitized to `[a-zA-Z0-9_-]` so the resulting ID always
/// round-trips through [`SecretScope::environment`] validation.
///
/// The result is stable for the same directory across multiple calls.
pub fn environment_id_from_cwd(cwd: &Path) -> EnvId {
    use sha2::{Digest, Sha256};

    // Git repo: sanitized name + a discriminator hash of the canonical root path.
    if let Ok(repo_root) = get_git_repo_root(cwd) {
        if let Some(repo_name) = repo_root
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
        {
            let sanitized = sanitize_env_component(repo_name);
            let canonical = std::fs::canonicalize(&repo_root).unwrap_or(repo_root);
            let hash = Sha256::digest(canonical.to_string_lossy().as_bytes());
            let short = hex_encode(&hash[..3]); // 6 hex chars
            return EnvId(format!("{}-{}", sanitized, short));
        }
    }

    // Fallback: hash the canonical path
    let canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let hash = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let short = hex_encode(&hash[..6]); // 12 hex chars
    EnvId(format!("cwd-{}", short))
}

/// Map a raw path component to the `[a-zA-Z0-9_-]` charset accepted by
/// [`SecretScope::environment`], replacing any other character with `-`.
fn sanitize_env_component(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if mapped.is_empty() {
        "repo".to_string()
    } else {
        mapped
    }
}

fn get_git_repo_root(cwd: &Path) -> Result<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .context("failed to execute git rev-parse")?;
    if !output.status.success() {
        anyhow::bail!("git rev-parse failed");
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── SecretScope ────────────────────────────────────────────────────────────

/// Determines where a secret is visible.
///
/// - [`Global`](SecretScope::Global): accessible from any environment.
/// - [`Environment(EnvId)`](SecretScope::Environment): bound to a specific
///   project / git repository.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SecretScope {
    /// Visible across all environments.
    Global,
    /// Visible only within a specific environment (git repo root).
    Environment(EnvId),
}

impl SecretScope {
    /// Create an `Environment` scope with a validated env_id.
    ///
    /// # Errors
    ///
    /// - Empty env_id
    /// - Env_id contains characters outside `[a-zA-Z0-9_-]`
    pub fn environment(env_id: String) -> Result<Self> {
        if env_id.is_empty() {
            anyhow::bail!("Environment ID must not be empty");
        }
        if !env_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            anyhow::bail!(
                "Environment ID must match [a-zA-Z0-9_-]+, got: {}",
                env_id
            );
        }
        Ok(Self::Environment(EnvId(env_id)))
    }

    /// Produce a canonical storage key: `"global/NAME"` or `"env/{env_id}/NAME"`.
    pub fn canonical_key(&self, name: &SecretName) -> String {
        match self {
            SecretScope::Global => format!("global/{}", name),
            SecretScope::Environment(env_id) => format!("env/{}/{}", env_id, name),
        }
    }

    pub fn is_global(&self) -> bool {
        matches!(self, SecretScope::Global)
    }
}

impl fmt::Display for SecretScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretScope::Global => write!(f, "global"),
            SecretScope::Environment(id) => write!(f, "env/{}", id),
        }
    }
}

// ─── SecretListEntry ────────────────────────────────────────────────────────

/// A single entry returned by [`SecretsBackend::list`].
#[derive(Debug, Clone)]
pub struct SecretListEntry {
    pub scope: SecretScope,
    pub name: SecretName,
}

// ─── SecretsBackendKind ─────────────────────────────────────────────────────

/// Known backend implementations for manager construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretsBackendKind {
    /// Age-encrypted file with OS keychain passphrase.
    Local,
}

// ─── SecretsBackend trait ───────────────────────────────────────────────────

/// Pluggable backend for secret storage.
///
/// All implementations must be `Send + Sync` so they can be shared across
/// threads via `Arc`.
pub trait SecretsBackend: Send + Sync {
    fn set(&self, scope: &SecretScope, name: &SecretName, value: &str) -> Result<()>;
    fn get(&self, scope: &SecretScope, name: &SecretName) -> Result<Option<String>>;
    fn delete(&self, scope: &SecretScope, name: &SecretName) -> Result<bool>;
    fn list(&self, scope_filter: Option<&SecretScope>) -> Result<Vec<SecretListEntry>>;

    /// Eagerly create any backing store / keychain material so later reads and
    /// writes succeed without surprises. Backends that need no setup may keep
    /// the default no-op.
    fn initialize(&self) -> Result<()> {
        Ok(())
    }
}

// ─── SecretsManager ─────────────────────────────────────────────────────────

/// Public API for secrets management.
///
/// Wraps a [`SecretsBackend`] (concrete type selected via [`SecretsBackendKind`])
/// with convenience methods.
///
/// **Environment fallback**: when [`get`](SecretsManager::get) is called with an
/// `Environment` scope and the secret is not found, it automatically falls back
/// to `Global` scope for the same name.
pub struct SecretsManager {
    backend: Arc<dyn SecretsBackend>,
}

impl SecretsManager {
    /// Create a new `SecretsManager` with the default OS keychain store.
    pub fn new(jcode_home: PathBuf, kind: SecretsBackendKind) -> Result<Self> {
        let keyring_store: Arc<dyn KeyringStore> =
            Arc::new(jcode_keyring_store::DefaultKeyringStore::new());
        Ok(Self::new_with_keyring_store(jcode_home, kind, keyring_store))
    }

    /// Create a `SecretsManager` with a custom keyring store (e.g. mock).
    pub fn new_with_keyring_store(
        jcode_home: PathBuf,
        kind: SecretsBackendKind,
        keyring_store: Arc<dyn KeyringStore>,
    ) -> Self {
        let backend: Arc<dyn SecretsBackend> = match kind {
            SecretsBackendKind::Local => {
                Arc::new(LocalSecretsBackend::new(jcode_home, keyring_store))
            }
        };
        Self { backend }
    }

    /// Set a secret, creating or overwriting it.
    pub fn set(&self, scope: &SecretScope, name: &SecretName, value: &str) -> Result<()> {
        self.backend.set(scope, name, value)
    }

    /// Get a secret by scope and name.
    ///
    /// When `scope` is `Environment` and the secret is not found, falls back
    /// to the `Global` scope.
    pub fn get(&self, scope: &SecretScope, name: &SecretName) -> Result<Option<String>> {
        let value = self.backend.get(scope, name)?;
        if value.is_some() {
            return Ok(value);
        }
        // Environment scope fallback: try Global
        if !scope.is_global() {
            self.backend.get(&SecretScope::Global, name)
        } else {
            Ok(None)
        }
    }

    /// Delete a secret. Returns `true` if the secret existed and was removed.
    pub fn delete(&self, scope: &SecretScope, name: &SecretName) -> Result<bool> {
        self.backend.delete(scope, name)
    }

    /// List secrets, optionally filtered by scope.
    pub fn list(&self, scope_filter: Option<&SecretScope>) -> Result<Vec<SecretListEntry>> {
        self.backend.list(scope_filter)
    }

    /// Eagerly initialize the backend (e.g. create the keychain passphrase and
    /// the encrypted store file) so the store is ready for use.
    pub fn initialize(&self) -> Result<()> {
        self.backend.initialize()
    }
}

// ─── Passphrase management ─────────────────────────────────────────────────

const SERVICE_NAME: &str = "jcode-secrets";
const PASS_ACCOUNT: &str = "local-secrets-passphrase";

/// Generate a cryptographically-random 32-byte passphrase, base64-encoded.
///
/// The raw random bytes are zeroized after encoding as basic hygiene. Note this
/// is best-effort: the returned base64 string still holds the entropy, and the
/// passphrase is ultimately persisted in the OS keychain, so callers should not
/// treat the wipe as a strong confidentiality guarantee.
pub fn generate_passphrase() -> String {
    use rand::TryRngCore;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .expect("OsRng should never fail");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    // Zeroize the raw key bytes (the most sensitive form). Volatile write so the
    // compiler cannot elide it. The only `unsafe` in the crate.
    for b in &mut bytes {
        unsafe {
            std::ptr::write_volatile(b, 0);
        }
    }
    encoded
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_name_validation() {
        assert!(SecretName::new("GITHUB_TOKEN").is_ok());
        assert!(SecretName::new("OPENAI_API_KEY").is_ok());
        assert!(SecretName::new("A1_B2_C3").is_ok());
        assert!(SecretName::new("lowercase").is_err());
        assert!(SecretName::new("has space").is_err());
        assert!(SecretName::new("special!").is_err());
        assert!(SecretName::new("").is_err());
    }

    #[test]
    fn test_scope_canonical_key() {
        let name = SecretName::new("MY_SECRET").unwrap();
        let global = SecretScope::Global;
        assert_eq!(global.canonical_key(&name), "global/MY_SECRET");

        let env = SecretScope::environment("my-repo".to_string()).unwrap();
        assert_eq!(env.canonical_key(&name), "env/my-repo/MY_SECRET");
    }

    #[test]
    fn test_environment_id_from_cwd_non_git() {
        let tmp = tempfile::tempdir().unwrap();
        let env_id = environment_id_from_cwd(tmp.path());
        // Non-git dir → cwd-{12 hex chars}
        assert!(env_id.to_string().starts_with("cwd-"));
        assert_eq!(env_id.to_string().len(), 16); // "cwd-" + 12 hex
    }

    #[test]
    fn test_sanitize_env_component() {
        assert_eq!(sanitize_env_component("my-repo"), "my-repo");
        assert_eq!(sanitize_env_component("my.repo"), "my-repo");
        assert_eq!(sanitize_env_component("a/b c"), "a-b-c");
        assert_eq!(sanitize_env_component(""), "repo");
        // Result must round-trip through environment scope validation.
        assert!(SecretScope::environment(sanitize_env_component("weird.name@x")).is_ok());
    }

    #[test]
    fn test_environment_id_same_basename_repos_do_not_collide() {
        fn init_repo(parent: &Path, name: &str) -> std::path::PathBuf {
            let repo = parent.join(name);
            std::fs::create_dir_all(&repo).unwrap();
            let _ = std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&repo)
                .status();
            repo
        }
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let repo_a = init_repo(a.path(), "acme");
        let repo_b = init_repo(b.path(), "acme");

        let id_a = environment_id_from_cwd(&repo_a).to_string();
        let id_b = environment_id_from_cwd(&repo_b).to_string();

        // When git is available both share the "acme-" prefix but the
        // path-hash discriminator must keep them distinct (no secret bleed).
        if id_a.starts_with("acme-") && id_b.starts_with("acme-") {
            assert_ne!(id_a, id_b, "same-basename repos must not collide");
        }
    }

    #[test]
    fn test_environment_scope_validation() {
        assert!(SecretScope::environment("my-project".to_string()).is_ok());
        assert!(SecretScope::environment("".to_string()).is_err());
        assert!(SecretScope::environment("spaces not ok".to_string()).is_err());
    }

    #[test]
    fn test_generate_passphrase() {
        let p1 = generate_passphrase();
        let p2 = generate_passphrase();
        assert_ne!(p1, p2);
        assert!(!p1.is_empty());
        // base64 of 32 bytes = 44 chars (no padding variant is 43; STANDARD pads)
        assert_eq!(p1.len(), 44);
    }

    #[test]
    fn test_secret_name_display() {
        let name = SecretName::new("MY_KEY").unwrap();
        assert_eq!(format!("{}", name), "MY_KEY");
    }

    #[test]
    fn test_scope_display() {
        assert_eq!(format!("{}", SecretScope::Global), "global");
        let env = SecretScope::environment("test-repo".to_string()).unwrap();
        assert_eq!(format!("{}", env), "env/test-repo");
    }
}
