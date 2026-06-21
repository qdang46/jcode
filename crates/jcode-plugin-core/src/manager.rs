use crate::errors::PluginError;
use crate::manifest::PluginManifest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginSource {
    // Load from a local path (file or directory)
    Local { path: PathBuf },
    // Clone a git repository
    Git { url: String, rev: Option<String> },
    // Reference a workspace crate
    WorkspaceCrate { crate_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub package_name: String,
    pub source: PluginSource,
    pub install_path: PathBuf,
    pub manifest: PluginManifest,
    pub installed_at: chrono::DateTime<chrono::Utc>,
    pub enabled: bool,
    pub settings: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginState {
    pub installed: HashMap<String, InstalledPlugin>,
    pub last_known_good: HashMap<String, InstalledPlugin>,
}

pub struct PluginManager {
    state: Arc<RwLock<PluginState>>,
    install_root: PathBuf,
    lock_path: PathBuf,
}

impl PluginManager {
    pub async fn new(install_root: PathBuf) -> Self {
        let lock_path = install_root.join("installed.json");
        let state = Self::load_state(&lock_path).await.unwrap_or_default();
        Self {
            state: Arc::new(RwLock::new(state)),
            install_root,
            lock_path,
        }
    }

    pub async fn load(
        &self,
        name: &str,
        source: PluginSource,
    ) -> Result<InstalledPlugin, PluginError> {
        let backup = self.state.read().await.last_known_good.clone();
        let install_path = self.install_root.join(name);

        match &source {
            PluginSource::Git { url, rev } => {
                let sanitized = Self::sanitize_url(url)?;
                tokio::fs::create_dir_all(&install_path)
                    .await
                    .map_err(|e| PluginError::Other(e.to_string()))?;

                let output = Command::new("git")
                    .args(["clone", "--depth", "1", &sanitized])
                    .arg(&install_path)
                    .output()
                    .await
                    .map_err(|e| PluginError::Other(format!("failed to execute git clone: {e}")))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(PluginError::Other(format!(
                        "git clone failed for {url}: {stderr}"
                    )));
                }

                if let Some(rev) = rev {
                    let co = Command::new("git")
                        .args(["checkout", rev])
                        .current_dir(&install_path)
                        .output()
                        .await
                        .map_err(|e| {
                            PluginError::Other(format!("failed to execute git checkout: {e}"))
                        })?;

                    if !co.status.success() {
                        let stderr = String::from_utf8_lossy(&co.stderr);
                        return Err(PluginError::Other(format!(
                            "git checkout {rev} failed in {url}: {stderr}"
                        )));
                    }
                }
            }
            _ => {
                tokio::fs::create_dir_all(&install_path)
                    .await
                    .map_err(|e| PluginError::Other(e.to_string()))?;
            }
        }

        let manifest = PluginManifest::default(); // minimal manifest for now
        let installed = InstalledPlugin {
            package_name: name.into(),
            source,
            install_path,
            manifest,
            installed_at: chrono::Utc::now(),
            enabled: true,
            settings: HashMap::new(),
        };

        let mut state = self.state.write().await;
        state.last_known_good = backup;
        state.installed.insert(name.into(), installed.clone());
        self.save_state(&state).await?;
        Ok(installed)
    }

    pub async fn unload(&self, name: &str) -> Result<(), PluginError> {
        let mut state = self.state.write().await;
        state.installed.remove(name);
        self.save_state(&state).await
    }

    pub async fn list(&self) -> Vec<InstalledPlugin> {
        let state = self.state.read().await;
        state.installed.values().cloned().collect()
    }

    pub async fn enable(&self, name: &str) -> Result<(), PluginError> {
        let mut state = self.state.write().await;
        if let Some(p) = state.installed.get_mut(name) {
            p.enabled = true;
        }
        self.save_state(&state).await
    }

    pub async fn disable(&self, name: &str) -> Result<(), PluginError> {
        let mut state = self.state.write().await;
        if let Some(p) = state.installed.get_mut(name) {
            p.enabled = false;
        }
        self.save_state(&state).await
    }

    async fn save_state(&self, state: &PluginState) -> Result<(), PluginError> {
        let json = serde_json::to_string_pretty(state)?;
        if let Some(parent) = self.lock_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&self.lock_path, json).await?;
        Ok(())
    }

    async fn load_state(lock_path: &PathBuf) -> Option<PluginState> {
        let content = tokio::fs::read_to_string(lock_path).await.ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Validate a git URL — must start with `https://` or `git@`.
    /// Rejects URLs containing shell-injection characters (`;`, `|`, `` ` ``, `$`, or spaces).
    fn sanitize_url(url: &str) -> Result<String, PluginError> {
        if !url.starts_with("https://") && !url.starts_with("git@") {
            return Err(PluginError::Other(format!(
                "unsupported URL scheme in plugin git URL: must start with https:// or git@, got {url:?}"
            )));
        }

        let forbidden = [';', '|', '`', '$', ' '];
        if let Some(ch) = url.chars().find(|c| forbidden.contains(c)) {
            return Err(PluginError::Other(format!(
                "plugin git URL contains forbidden character {ch:?}, rejecting for shell safety: {url:?}"
            )));
        }

        Ok(url.to_owned())
    }

    /// Derive a filesystem-safe install name from a git URL.
    /// Extracts the repo name from the last path segment (stripping `.git` suffix).
    #[expect(dead_code)]
    fn install_name_from_url(url: &str) -> String {
        let path = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("git@"))
            .unwrap_or(url);

        // For git@ URLs: "git@github.com:user/repo.git" → "user/repo.git"
        // Normalise colons in the host:path part to slashes for extraction
        let path = path.replace(':', "/");

        let name = path.split('/').rfind(|s| !s.is_empty()).unwrap_or("plugin");

        name.strip_suffix(".git").unwrap_or(name).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_and_list_plugin() {
        let tmp = std::env::temp_dir().join(format!("jcode-manager-test-{}", uuid::Uuid::new_v4()));
        let mgr = PluginManager::new(tmp.clone()).await;
        let p = mgr
            .load(
                "test",
                PluginSource::Local {
                    path: tmp.join("src"),
                },
            )
            .await
            .unwrap();
        assert_eq!(p.package_name, "test");
        let list = mgr.list().await;
        assert_eq!(list.len(), 1);
        let _ = tokio::fs::remove_dir_all(tmp).await;
    }

    #[tokio::test]
    async fn test_enable_disable_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("jcode-manager-test-{}", uuid::Uuid::new_v4()));
        let mgr = PluginManager::new(tmp.clone()).await;
        mgr.load(
            "test",
            PluginSource::Local {
                path: tmp.join("src"),
            },
        )
        .await
        .unwrap();
        mgr.disable("test").await.unwrap();
        let list = mgr.list().await;
        assert!(!list.iter().any(|p| p.package_name == "test" && p.enabled));
        mgr.enable("test").await.unwrap();
        let list = mgr.list().await;
        assert!(list.iter().any(|p| p.package_name == "test" && p.enabled));
        let _ = tokio::fs::remove_dir_all(tmp).await;
    }

    #[tokio::test]
    async fn test_unload_is_idempotent() {
        let tmp = std::env::temp_dir().join(format!("jcode-manager-test-{}", uuid::Uuid::new_v4()));
        let mgr = PluginManager::new(tmp.clone()).await;
        // unload non-existent — should not error
        mgr.unload("nonexistent").await.unwrap();
        mgr.load(
            "test",
            PluginSource::Local {
                path: tmp.join("src"),
            },
        )
        .await
        .unwrap();
        mgr.unload("test").await.unwrap();
        let list = mgr.list().await;
        assert!(list.is_empty());
        let _ = tokio::fs::remove_dir_all(tmp).await;
    }
}
