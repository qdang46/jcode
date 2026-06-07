//! Bridge between [`SecretsManager`] and the `jcode-provider-env` fallback
//! resolver registry.
//!
//! `jcode-provider-env` exposes `register_api_key_fallback_resolver(fn(&str) ->
//! Option<String>)`. Because that registry stores bare function pointers (no
//! captured state), the resolver here reads from a process-global
//! [`SecretsManager`] singleton initialised lazily from the jcode home dir.
//!
//! Lookup order for a given env-var name (e.g. `ANTHROPIC_API_KEY`):
//! 1. Environment scope derived from the current working directory.
//! 2. Global scope (automatic fallback inside [`SecretsManager::get`]).
//!
//! Provider-env consults this resolver only after an explicit environment
//! variable and the provider env-file both miss, so stored secrets never
//! silently override a value the user set explicitly in their environment.

use std::path::Path;

use crate::{SecretName, SecretScope, SecretsBackendKind, SecretsManager};

/// Build a [`SecretsManager`] for the current jcode home directory.
///
/// Construction is I/O-free — the backend only touches the filesystem and OS
/// keychain on `get`/`set` — so the manager is built fresh per call rather than
/// cached. This avoids a transient failure to resolve the home directory being
/// cached for the rest of the process and silently disabling secret resolution.
///
/// Returns `None` when the jcode home directory cannot be resolved.
pub fn current_manager() -> Option<SecretsManager> {
    let jcode_home = jcode_storage::jcode_dir().ok()?;
    SecretsManager::new(jcode_home, SecretsBackendKind::Local).ok()
}

/// Fallback resolver for `jcode-provider-env`.
///
/// Matches the `fn(&str) -> Option<String>` signature expected by
/// `jcode_provider_env::register_api_key_fallback_resolver`.
pub fn secrets_api_key_resolver(env_key: &str) -> Option<String> {
    let manager = current_manager()?;
    let cwd = std::env::current_dir().ok()?;
    resolve_with_manager(&manager, &cwd, env_key)
}

/// Inner resolution logic, decoupled from the global singleton and the process
/// cwd so it can be unit-tested with a mock-backed manager.
fn resolve_with_manager(
    manager: &SecretsManager,
    cwd: &Path,
    env_key: &str,
) -> Option<String> {
    let name = SecretName::new(env_key).ok()?;
    let env_id = crate::environment_id_from_cwd(cwd);
    // Environment scope first; SecretsManager::get falls back to Global.
    let scope = SecretScope::Environment(env_id);
    manager.get(&scope, &name).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_keyring_store::{KeyringStore, MockKeyringStore};
    use std::sync::Arc;

    fn manager_in(dir: &Path) -> SecretsManager {
        let keyring = Arc::new(MockKeyringStore::new()) as Arc<dyn KeyringStore>;
        SecretsManager::new_with_keyring_store(
            dir.to_path_buf(),
            SecretsBackendKind::Local,
            keyring,
        )
    }

    #[test]
    fn resolves_environment_scoped_secret() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let env_id = crate::environment_id_from_cwd(dir.path());
        let scope = SecretScope::Environment(env_id);
        let name = SecretName::new("ANTHROPIC_API_KEY").unwrap();
        manager.set(&scope, &name, "sk-ant-env").unwrap();

        let got = resolve_with_manager(&manager, dir.path(), "ANTHROPIC_API_KEY");
        assert_eq!(got.as_deref(), Some("sk-ant-env"));
    }

    #[test]
    fn falls_back_to_global_scope() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let name = SecretName::new("OPENAI_API_KEY").unwrap();
        manager.set(&SecretScope::Global, &name, "sk-global").unwrap();

        // No environment-scoped value exists; resolver must fall back to Global.
        let got = resolve_with_manager(&manager, dir.path(), "OPENAI_API_KEY");
        assert_eq!(got.as_deref(), Some("sk-global"));
    }

    #[test]
    fn returns_none_for_unknown_key() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let got = resolve_with_manager(&manager, dir.path(), "MISSING_API_KEY");
        assert_eq!(got, None);
    }

    #[test]
    fn returns_none_for_invalid_secret_name() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        // Lowercase is not a valid SecretName, so resolution is skipped.
        let got = resolve_with_manager(&manager, dir.path(), "lowercase-key");
        assert_eq!(got, None);
    }

    #[test]
    fn environment_scope_takes_precedence_over_global() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let name = SecretName::new("GROQ_API_KEY").unwrap();
        let env_id = crate::environment_id_from_cwd(dir.path());

        manager.set(&SecretScope::Global, &name, "global-val").unwrap();
        manager
            .set(&SecretScope::Environment(env_id), &name, "env-val")
            .unwrap();

        let got = resolve_with_manager(&manager, dir.path(), "GROQ_API_KEY");
        assert_eq!(got.as_deref(), Some("env-val"));
    }
}
