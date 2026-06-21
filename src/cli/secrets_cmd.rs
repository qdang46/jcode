//! Handlers for the `jcode secrets` subcommand group.
//!
//! Reads and writes the encrypted, OS-keychain-backed secrets store via
//! [`jcode_secrets::SecretsManager`]. Secret values are only ever printed by
//! the explicit `get` command; `set`/`delete`/`list` never echo values.
//!
//! Each `run_*` entry point resolves the real manager + scope, then delegates
//! to an inner `*_with` function that takes an explicit `&SecretsManager` and
//! returns the output string, so the formatting and store logic are unit-testable
//! without the OS keychain.

use anyhow::{Context, Result};
use std::io::Read;

use jcode_secrets::{
    SecretName, SecretScope, SecretsBackendKind, SecretsManager, environment_id_from_cwd,
};
use serde::Serialize;

fn manager() -> Result<SecretsManager> {
    let jcode_home = jcode_storage::jcode_dir().context("resolve jcode home directory")?;
    SecretsManager::new(jcode_home, SecretsBackendKind::Local)
}

/// `--env` selects an environment scope keyed to the current git repo / cwd;
/// otherwise the global scope is used.
fn scope_for(env: bool) -> Result<SecretScope> {
    if env {
        let cwd = std::env::current_dir().context("resolve current directory")?;
        Ok(SecretScope::Environment(environment_id_from_cwd(&cwd)))
    } else {
        Ok(SecretScope::Global)
    }
}

fn read_value_from_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("read secret value from stdin")?;
    let trimmed = buf.trim_end_matches(['\n', '\r']).to_string();
    if trimmed.is_empty() {
        anyhow::bail!("No secret value provided (stdin was empty)");
    }
    Ok(trimmed)
}

#[derive(Serialize)]
struct SetOutput {
    name: String,
    scope: String,
    status: &'static str,
}

pub fn run_set(name: &str, value: Option<&str>, env: bool, json: bool, toon: bool) -> Result<()> {
    let manager = manager()?;
    let scope = scope_for(env)?;
    let value = match value {
        Some(v) => v.to_string(),
        None => read_value_from_stdin()?,
    };
    let secret_name = SecretName::new(name)?;
    manager.set(&scope, &secret_name, &value)?;
    let out = SetOutput {
        name: secret_name.as_str().to_string(),
        scope: scope.to_string(),
        status: "set",
    };
    if json || toon {
        let fmt = if toon {
            crate::cli::output::OutputFormat::Toon
        } else {
            crate::cli::output::OutputFormat::Json
        };
        crate::cli::output::emit_json_or_toon(&out, fmt)?;
    } else {
        println!("Set {} ({}).", out.name, out.scope);
    }
    Ok(())
}

#[allow(dead_code)]
fn set_with(
    manager: &SecretsManager,
    scope: &SecretScope,
    name: &str,
    value: &str,
    json: bool,
) -> Result<String> {
    let secret_name = SecretName::new(name)?;
    manager.set(scope, &secret_name, value)?;
    let out = SetOutput {
        name: secret_name.as_str().to_string(),
        scope: scope.to_string(),
        status: "set",
    };
    Ok(if json {
        serde_json::to_string_pretty(&out)?
    } else {
        format!("Set {} ({}).", out.name, out.scope)
    })
}

#[derive(Serialize)]
struct GetOutput {
    name: String,
    scope: String,
    found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

pub fn run_get(name: &str, env: bool, json: bool, toon: bool) -> Result<()> {
    let manager = manager()?;
    let scope = scope_for(env)?;
    let secret_name = SecretName::new(name)?;
    let value = manager.get(&scope, &secret_name)?;
    if json || toon {
        let out = GetOutput {
            name: secret_name.as_str().to_string(),
            scope: scope.to_string(),
            found: value.is_some(),
            value,
        };
        let fmt = if toon {
            crate::cli::output::OutputFormat::Toon
        } else {
            crate::cli::output::OutputFormat::Json
        };
        crate::cli::output::emit_json_or_toon(&out, fmt)?;
    } else {
        match value {
            Some(v) => println!("{v}"),
            None => anyhow::bail!("No secret named {} in {} scope", secret_name, scope),
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn get_with(
    manager: &SecretsManager,
    scope: &SecretScope,
    name: &str,
    json: bool,
) -> Result<String> {
    let secret_name = SecretName::new(name)?;
    let value = manager.get(scope, &secret_name)?;
    if json {
        let out = GetOutput {
            name: secret_name.as_str().to_string(),
            scope: scope.to_string(),
            found: value.is_some(),
            value,
        };
        return Ok(serde_json::to_string_pretty(&out)?);
    }
    match value {
        // Raw value so it is scriptable: KEY=$(jcode secrets get NAME)
        Some(v) => Ok(v),
        None => anyhow::bail!("No secret named {} in {} scope", secret_name, scope),
    }
}

#[derive(Serialize)]
struct DeleteOutput {
    name: String,
    scope: String,
    deleted: bool,
}

pub fn run_delete(name: &str, env: bool, json: bool, toon: bool) -> Result<()> {
    let manager = manager()?;
    let scope = scope_for(env)?;
    let secret_name = SecretName::new(name)?;
    let deleted = manager.delete(&scope, &secret_name)?;
    let out = DeleteOutput {
        name: secret_name.as_str().to_string(),
        scope: scope.to_string(),
        deleted,
    };
    if json || toon {
        let fmt = if toon {
            crate::cli::output::OutputFormat::Toon
        } else {
            crate::cli::output::OutputFormat::Json
        };
        crate::cli::output::emit_json_or_toon(&out, fmt)?;
    } else if deleted {
        println!("Deleted {} ({}).", out.name, out.scope);
    } else {
        println!("No secret named {} in {} scope.", out.name, out.scope);
    }
    Ok(())
}

#[allow(dead_code)]
fn delete_with(
    manager: &SecretsManager,
    scope: &SecretScope,
    name: &str,
    json: bool,
) -> Result<String> {
    let secret_name = SecretName::new(name)?;
    let deleted = manager.delete(scope, &secret_name)?;
    let out = DeleteOutput {
        name: secret_name.as_str().to_string(),
        scope: scope.to_string(),
        deleted,
    };
    Ok(if json {
        serde_json::to_string_pretty(&out)?
    } else if deleted {
        format!("Deleted {} ({}).", out.name, out.scope)
    } else {
        format!("No secret named {} in {} scope.", out.name, out.scope)
    })
}

#[derive(Serialize)]
struct ListEntryOut {
    scope: String,
    name: String,
}

#[derive(Serialize)]
struct ListOutput {
    secrets: Vec<ListEntryOut>,
}

pub fn run_list(env: bool, json: bool, toon: bool) -> Result<()> {
    let manager = manager()?;
    let filter = if env { Some(scope_for(true)?) } else { None };
    let mut entries = manager.list(filter.as_ref())?;
    entries.sort_by(|a, b| {
        a.scope
            .to_string()
            .cmp(&b.scope.to_string())
            .then_with(|| a.name.as_str().cmp(b.name.as_str()))
    });
    let out = ListOutput {
        secrets: entries
            .iter()
            .map(|e| ListEntryOut {
                scope: e.scope.to_string(),
                name: e.name.as_str().to_string(),
            })
            .collect(),
    };
    if json || toon {
        let fmt = if toon {
            crate::cli::output::OutputFormat::Toon
        } else {
            crate::cli::output::OutputFormat::Json
        };
        crate::cli::output::emit_json_or_toon(&out, fmt)?;
    } else if out.secrets.is_empty() {
        println!("No secrets stored.");
    } else {
        for e in &out.secrets {
            println!("{}\t{}", e.scope, e.name);
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn list_with(manager: &SecretsManager, filter: Option<&SecretScope>, json: bool) -> Result<String> {
    let mut entries = manager.list(filter)?;
    entries.sort_by(|a, b| {
        a.scope
            .to_string()
            .cmp(&b.scope.to_string())
            .then_with(|| a.name.as_str().cmp(b.name.as_str()))
    });
    let out = ListOutput {
        secrets: entries
            .iter()
            .map(|e| ListEntryOut {
                scope: e.scope.to_string(),
                name: e.name.as_str().to_string(),
            })
            .collect(),
    };
    Ok(if json {
        serde_json::to_string_pretty(&out)?
    } else if out.secrets.is_empty() {
        "No secrets stored.".to_string()
    } else {
        out.secrets
            .iter()
            .map(|e| format!("{}\t{}", e.scope, e.name))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

#[derive(Serialize)]
struct InitOutput {
    status: &'static str,
}

pub fn run_init(json: bool, toon: bool) -> Result<()> {
    let manager = manager()?;
    manager.initialize()?;
    if json || toon {
        let fmt = if toon {
            crate::cli::output::OutputFormat::Toon
        } else {
            crate::cli::output::OutputFormat::Json
        };
        crate::cli::output::emit_json_or_toon(
            &InitOutput {
                status: "initialized",
            },
            fmt,
        )?;
    } else {
        println!("Initialized encrypted secrets store and OS keychain passphrase.");
    }
    Ok(())
}

#[allow(dead_code)]
fn init_with(manager: &SecretsManager, json: bool) -> Result<String> {
    manager.initialize()?;
    Ok(if json {
        serde_json::to_string_pretty(&InitOutput {
            status: "initialized",
        })?
    } else {
        "Initialized encrypted secrets store and OS keychain passphrase.".to_string()
    })
}

#[derive(Serialize)]
struct PurgeOutput {
    status: &'static str,
}

pub fn run_purge(yes: bool, json: bool, toon: bool) -> Result<()> {
    if !yes {
        anyhow::bail!(
            "Refusing to purge. This permanently deletes ALL stored secrets and the \
             keychain passphrase. Re-run with --yes to confirm."
        );
    }
    let manager = manager()?;
    manager.purge()?;
    if json || toon {
        let fmt = if toon {
            crate::cli::output::OutputFormat::Toon
        } else {
            crate::cli::output::OutputFormat::Json
        };
        crate::cli::output::emit_json_or_toon(&PurgeOutput { status: "purged" }, fmt)?;
    } else {
        println!("Purged all stored secrets and removed the keychain passphrase.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_keyring_store::{KeyringStore, MockKeyringStore};
    use std::sync::Arc;

    fn test_manager(dir: &std::path::Path) -> SecretsManager {
        let keyring = Arc::new(MockKeyringStore::new()) as Arc<dyn KeyringStore>;
        SecretsManager::new_with_keyring_store(
            dir.to_path_buf(),
            SecretsBackendKind::Local,
            keyring,
        )
    }

    #[test]
    fn set_then_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        let scope = SecretScope::Global;

        let msg = set_with(&m, &scope, "MY_KEY", "secret-val", false).unwrap();
        assert_eq!(msg, "Set MY_KEY (global).");
        assert_eq!(get_with(&m, &scope, "MY_KEY", false).unwrap(), "secret-val");
    }

    #[test]
    fn get_missing_text_errors() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        let err = get_with(&m, &SecretScope::Global, "NOPE", false).unwrap_err();
        assert!(err.to_string().contains("No secret named NOPE"));
    }

    #[test]
    fn get_missing_json_reports_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        let out = get_with(&m, &SecretScope::Global, "NOPE", true).unwrap();
        assert!(out.contains("\"found\": false"));
        assert!(!out.contains("\"value\""));
    }

    #[test]
    fn invalid_name_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        assert!(set_with(&m, &SecretScope::Global, "lower-case", "v", false).is_err());
    }

    #[test]
    fn delete_reports_existence() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        let scope = SecretScope::Global;
        set_with(&m, &scope, "K", "v", false).unwrap();
        assert_eq!(
            delete_with(&m, &scope, "K", false).unwrap(),
            "Deleted K (global)."
        );
        assert_eq!(
            delete_with(&m, &scope, "K", false).unwrap(),
            "No secret named K in global scope."
        );
    }

    #[test]
    fn list_never_prints_values() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        let scope = SecretScope::Global;
        set_with(&m, &scope, "ALPHA", "super-secret-value", false).unwrap();
        set_with(&m, &scope, "BETA", "another-secret", false).unwrap();

        let text = list_with(&m, None, false).unwrap();
        assert!(text.contains("ALPHA") && text.contains("BETA"));
        assert!(!text.contains("super-secret-value"));
        assert!(!text.contains("another-secret"));

        let json = list_with(&m, None, true).unwrap();
        assert!(json.contains("ALPHA"));
        assert!(!json.contains("super-secret-value"));
    }

    #[test]
    fn init_creates_store() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        let msg = init_with(&m, false).unwrap();
        assert!(msg.contains("Initialized"));
        // Empty but readable after init.
        assert_eq!(list_with(&m, None, false).unwrap(), "No secrets stored.");
    }

    #[test]
    fn scope_for_global_is_default() {
        assert!(matches!(scope_for(false).unwrap(), SecretScope::Global));
    }

    #[test]
    fn purge_removes_all_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let m = test_manager(dir.path());
        let scope = SecretScope::Global;
        set_with(&m, &scope, "K", "v", false).unwrap();
        // Purge happens at the manager level
        let _ = m.purge();
        assert_eq!(list_with(&m, None, false).unwrap(), "No secrets stored.");
    }
}
