use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use crate::manifest::PluginManifest;
use crate::errors::PluginError;

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
        Self { state: Arc::new(RwLock::new(state)), install_root, lock_path }
    }

    pub async fn load(&self, name: &str, source: PluginSource) -> Result<InstalledPlugin, PluginError> {
        let backup = self.state.read().await.last_known_good.clone();
        let install_path = self.install_root.join(name);
        tokio::fs::create_dir_all(&install_path).await.map_err(|e| PluginError::Other(e.to_string()))?;

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
        if let Some(p) = state.installed.get_mut(name) { p.enabled = true; }
        self.save_state(&state).await
    }

    pub async fn disable(&self, name: &str) -> Result<(), PluginError> {
        let mut state = self.state.write().await;
        if let Some(p) = state.installed.get_mut(name) { p.enabled = false; }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_and_list_plugin() {
        let tmp = std::env::temp_dir().join(format!("jcode-manager-test-{}", uuid::Uuid::new_v4()));
        let mgr = PluginManager::new(tmp.clone()).await;
        let p = mgr.load("test", PluginSource::Local { path: tmp.join("src") }).await.unwrap();
        assert_eq!(p.package_name, "test");
        let list = mgr.list().await;
        assert_eq!(list.len(), 1);
        let _ = tokio::fs::remove_dir_all(tmp).await;
    }

    #[tokio::test]
    async fn test_enable_disable_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("jcode-manager-test-{}", uuid::Uuid::new_v4()));
        let mgr = PluginManager::new(tmp.clone()).await;
        mgr.load("test", PluginSource::Local { path: tmp.join("src") }).await.unwrap();
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
        mgr.load("test", PluginSource::Local { path: tmp.join("src") }).await.unwrap();
        mgr.unload("test").await.unwrap();
        let list = mgr.list().await;
        assert!(list.is_empty());
        let _ = tokio::fs::remove_dir_all(tmp).await;
    }
}
