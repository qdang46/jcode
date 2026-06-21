use crate::registry::PluginRegistry;
use crate::runtime::RuntimeManager;
use crate::transpiler::Transpiler;
use crate::types::ResolvedEntry;
use jcode_plugin_core::PluginError;
use jcode_plugin_core::config::{
    DiscoveryPaths, PluginConfig, PluginSourceConfig, is_valid_package_name,
};
use jcode_plugin_core::preflight::PreflightAnalyzer;
use jcode_plugin_core::types::PluginId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Fingerprint uniquely identifies a version of a plugin file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFingerprint {
    /// Seahash of the file contents
    hash: u64,
    /// Last modification time, if the platform supports it
    mtime: Option<std::time::SystemTime>,
    /// File size in bytes
    size: usize,
}

pub struct PluginLoader {
    discovery: DiscoveryPaths,
    config: PluginConfig,
    registry: Arc<PluginRegistry>,
    transpiler: Arc<Transpiler>,
    runtime: Arc<RuntimeManager>,
    /// Cache of plugin fingerprints, used by `reload` to detect changes
    fingerprints: tokio::sync::RwLock<HashMap<PluginId, PluginFingerprint>>,
}

impl PluginLoader {
    pub fn new(
        discovery: DiscoveryPaths,
        config: PluginConfig,
        registry: Arc<PluginRegistry>,
        runtime: Arc<RuntimeManager>,
    ) -> Self {
        Self {
            discovery,
            config,
            registry,
            transpiler: Arc::new(Transpiler::new()),
            runtime,
            fingerprints: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    pub async fn load_all(&self) -> Result<Vec<PluginId>, PluginError> {
        let sources = self.discover_sources().await?;
        let mut loaded = Vec::new();
        for source in sources {
            match self.load_one(&source).await {
                Ok(id) => loaded.push(id),
                Err(e) => {
                    if self.config.fail_closed.unwrap_or(false) {
                        return Err(e);
                    }
                    tracing::warn!("Failed to load plugin {source:?}: {e}");
                }
            }
        }
        Ok(loaded)
    }

    async fn discover_sources(&self) -> Result<Vec<PluginSourceConfig>, PluginError> {
        let mut sources = Vec::new();
        if let Some(ref cfg_sources) = self.config.sources {
            sources.extend(cfg_sources.clone());
        }
        for dir in &self.discovery.plugin_dirs {
            self.scan_directory(dir, &mut sources).await?;
        }
        let npm_dir = &self.discovery.npm_cache;
        if npm_dir.exists() {
            self.scan_npm_cache(npm_dir, &mut sources).await?;
        }
        Ok(sources)
    }

    async fn scan_directory(
        &self,
        dir: &Path,
        sources: &mut Vec<PluginSourceConfig>,
    ) -> Result<(), PluginError> {
        if !dir.exists() {
            tokio::fs::create_dir_all(dir).await?;
            return Ok(());
        }
        let mut read_dir = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".ts") || name.ends_with(".js") {
                sources.push(PluginSourceConfig::File {
                    path: path.to_string_lossy().to_string(),
                });
            }
        }
        Ok(())
    }

    async fn scan_npm_cache(
        &self,
        dir: &Path,
        sources: &mut Vec<PluginSourceConfig>,
    ) -> Result<(), PluginError> {
        let mut read_dir = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
            {
                let package_name = name.replace("__", "/");
                sources.push(PluginSourceConfig::Npm {
                    package: package_name,
                    version: None,
                });
            }
        }
        Ok(())
    }

    pub(crate) async fn load_one(
        &self,
        source: &PluginSourceConfig,
    ) -> Result<PluginId, PluginError> {
        let (path, id) = match source {
            PluginSourceConfig::Npm { package, version } => {
                let entry = self.resolve_npm_entry(package, version.as_deref()).await?;
                (entry.path, PluginId::npm(package))
            }
            PluginSourceConfig::File { path } => {
                (std::path::PathBuf::from(path), PluginId::file(path))
            }
            PluginSourceConfig::Directory { path } => {
                let p = std::path::Path::new(path);
                let idx = if p.join("index.ts").exists() {
                    p.join("index.ts")
                } else {
                    p.join("index.js")
                };
                (idx, PluginId::file(path))
            }
        };

        let code = tokio::fs::read_to_string(&path).await?;

        // Preflight static analysis — catch suspicious patterns before eval
        let manifest_caps = jcode_plugin_core::manifest::PluginCapabilities::default();
        let preflight = PreflightAnalyzer::analyze(&code, &manifest_caps);
        if !preflight.warnings.is_empty() {
            for w in &preflight.warnings {
                tracing::warn!("Plugin {} preflight warning: {}", id, w);
            }
        }
        if !preflight.passed {
            return Err(PluginError::Load(format!(
                "Plugin {} blocked by preflight analysis: {}",
                id,
                preflight.blocks.join("; ")
            )));
        }

        let js_code = if path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
            self.transpiler.transpile(&code, &path.to_string_lossy())?
        } else {
            code
        };

        let context = self.runtime.create_sandbox(
            id.clone(),
            jcode_plugin_core::manifest::PluginManifest::default(),
        )?;
        context
            .eval_with_pi(&js_code, self.registry.clone())
            .await?;
        self.registry.commit();
        self.registry.register(id.clone(), ()).await?;

        // Seed the fingerprint cache on first load
        let fp = Self::fingerprint(&path).await?;
        self.fingerprints.write().await.insert(id.clone(), fp);

        Ok(id)
    }

    /// Hot-reload a single plugin by id. Compares fingerprints (seahash, mtime,
    /// size); if unchanged, this is a no-op. Otherwise re-transpiles, re-evals
    /// atomically and updates the registry.
    pub async fn reload(&self, plugin_id: &PluginId) -> Result<(), PluginError> {
        let path = self.path_for_plugin(plugin_id)?;
        let new_fp = Self::fingerprint(&path).await?;
        {
            let cache = self.fingerprints.read().await;
            if cache.get(plugin_id) == Some(&new_fp) {
                return Ok(()); // no-op — unchanged
            }
        }

        // Read source
        let code = tokio::fs::read_to_string(&path).await?;

        // Transpile if needed
        let js_code = if path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
            self.transpiler.transpile(&code, &path.to_string_lossy())?
        } else {
            code
        };

        // Preflight static analysis
        let manifest_caps = jcode_plugin_core::manifest::PluginCapabilities::default();
        let preflight = PreflightAnalyzer::analyze(&js_code, &manifest_caps);
        if !preflight.warnings.is_empty() {
            for w in &preflight.warnings {
                tracing::warn!("Plugin {} preflight warning: {}", plugin_id, w);
            }
        }
        if !preflight.passed {
            return Err(PluginError::Load(format!(
                "Plugin {plugin_id} blocked by preflight"
            )));
        }

        // Create a fresh sandbox and eval the new code BEFORE unregistering
        // the old plugin. This way if eval fails, the old plugin stays active.
        let context = self.runtime.create_sandbox(
            plugin_id.clone(),
            jcode_plugin_core::manifest::PluginManifest::default(),
        )?;
        context
            .eval_with_pi(&js_code, self.registry.clone())
            .await?;

        // Eval succeeded. Now atomically replace the old plugin state:
        //   1. Unregister old (removes old handlers from dispatcher snapshot)
        //   2. Commit pending new handlers from the fresh eval
        //   3. Register the new plugin in the HashMap
        self.registry.unregister(plugin_id).await;
        self.registry.commit();
        self.registry.register(plugin_id.clone(), ()).await?;

        // Update fingerprint cache
        self.fingerprints
            .write()
            .await
            .insert(plugin_id.clone(), new_fp);
        Ok(())
    }

    /// Remove the cached fingerprint for a plugin, so the next reload will
    /// treat the plugin as changed even if the file content hasn't been
    /// modified.
    pub async fn remove_fingerprint(&self, plugin_id: &PluginId) {
        self.fingerprints.write().await.remove(plugin_id);
    }

    /// Compute a fingerprint for a plugin file: seahash + mtime + size.
    async fn fingerprint(path: &Path) -> Result<PluginFingerprint, PluginError> {
        let bytes = tokio::fs::read(path).await?;
        let meta = tokio::fs::metadata(path).await?;
        Ok(PluginFingerprint {
            hash: seahash::hash(&bytes),
            mtime: meta.modified().ok(),
            size: bytes.len(),
        })
    }

    /// Resolve the on-disk path for a plugin by id. Supports `file:` prefixed
    /// plugin IDs as well as bare paths.
    fn path_for_plugin(&self, plugin_id: &PluginId) -> Result<std::path::PathBuf, PluginError> {
        let path_str = plugin_id.short_name().to_string();
        let path = std::path::PathBuf::from(&path_str);
        if path.exists() {
            return Ok(path);
        }
        // Fall back to raw id string
        let path2 = std::path::PathBuf::from(plugin_id.as_str());
        if path2.exists() {
            return Ok(path2);
        }
        Err(PluginError::Other(format!(
            "plugin path not found: {plugin_id} (resolved as {path_str})"
        )))
    }

    async fn resolve_npm_entry(
        &self,
        package: &str,
        _version: Option<&str>,
    ) -> Result<ResolvedEntry, PluginError> {
        let cache = self.discovery.npm_cache.join(sanitize_npm_name(package));
        if !cache.exists() {
            self.install_npm(package, None, &cache).await?;
        }
        let pkg_json = cache
            .join("node_modules")
            .join(package)
            .join("package.json");
        let content = tokio::fs::read_to_string(&pkg_json).await?;
        let json: serde_json::Value = serde_json::from_str(&content)?;
        let manifest = jcode_plugin_core::manifest::PluginManifest::from_package_json(&json)?;
        let entry = manifest
            .entry
            .server
            .as_ref()
            .or(manifest.entry.both.as_ref())
            .ok_or_else(|| PluginError::InvalidManifest("No server entry point".into()))?;
        Ok(ResolvedEntry {
            path: cache.join(entry),
            manifest,
        })
    }

    async fn install_npm(
        &self,
        package: &str,
        _version: Option<&str>,
        dir: &Path,
    ) -> Result<(), PluginError> {
        if !is_valid_package_name(package) {
            return Err(PluginError::Other("Invalid package name".into()));
        }
        tokio::fs::create_dir_all(dir).await?;
        let spec = package.to_string();
        let out = tokio::process::Command::new("npm")
            .args(["install", &spec, "--no-save", "--no-audit"])
            .current_dir(dir)
            .output()
            .await?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PluginError::Npm(stderr.to_string()));
        }
        Ok(())
    }
}

fn sanitize_npm_name(name: &str) -> String {
    name.replace('/', "__").replace('@', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::RcuDispatcher;
    use crate::registry::PluginRegistry;
    use crate::runtime::{RuntimeConfig, RuntimeManager};
    use jcode_plugin_core::config::DiscoveryPaths;
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // fingerprint unit tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_fingerprint_is_deterministic() {
        let dir = std::env::temp_dir().join("jcode-fp-test-1");
        let path = dir.join("plugin.js");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(&path, "pi.on('Test', () => {});")
            .await
            .unwrap();

        let fp1 = PluginLoader::fingerprint(&path).await.unwrap();
        let fp2 = PluginLoader::fingerprint(&path).await.unwrap();

        assert_eq!(fp1, fp2, "fingerprint should be deterministic");
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn test_fingerprint_changes_when_content_changes() {
        let dir = std::env::temp_dir().join("jcode-fp-test-2");
        let path = dir.join("plugin.js");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(&path, "version1").await.unwrap();

        let fp1 = PluginLoader::fingerprint(&path).await.unwrap();

        tokio::fs::write(&path, "version2").await.unwrap();

        let fp2 = PluginLoader::fingerprint(&path).await.unwrap();

        assert_ne!(fp1, fp2, "different content => different fingerprint");
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn test_fingerprint_nonexistent_file_errors() {
        let path = PathBuf::from("/tmp/jcode-fp-nonexistent-xyzzy.js");
        let result = PluginLoader::fingerprint(&path).await;
        assert!(
            result.is_err(),
            "fingerprint on nonexistent file should error"
        );
    }

    // -----------------------------------------------------------------------
    // path_for_plugin unit tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_path_for_plugin_resolves_file_prefix() {
        let dir = std::env::temp_dir().join("jcode-path-test-1");
        let path = dir.join("my-plugin.js");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(&path, "// test").await.unwrap();

        let registry = Arc::new(PluginRegistry::new(Arc::new(RcuDispatcher::new())));
        let runtime = Arc::new(RuntimeManager::new(RuntimeConfig::default()).unwrap());
        let loader = PluginLoader::new(
            DiscoveryPaths {
                plugin_dirs: vec![],
                npm_cache: std::env::temp_dir().join("jcode-test-npm-cache"),
                tool_dirs: vec![],
            },
            jcode_plugin_core::config::PluginConfig::default(),
            registry,
            runtime,
        );

        let id_full = PluginId::file(&path.to_string_lossy());
        let resolved = loader.path_for_plugin(&id_full).unwrap();
        assert!(
            resolved.exists(),
            "path_for_plugin should find existing file"
        );

        // Also test via short_name (bare path)
        let id_short = PluginId::from(path.to_string_lossy().to_string());
        let resolved2 = loader.path_for_plugin(&id_short).unwrap();
        assert!(resolved2.exists(), "bare plugin id should also resolve");

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn test_path_for_plugin_nonexistent_errors() {
        let registry = Arc::new(PluginRegistry::new(Arc::new(RcuDispatcher::new())));
        let runtime = Arc::new(RuntimeManager::new(RuntimeConfig::default()).unwrap());
        let loader = PluginLoader::new(
            DiscoveryPaths {
                plugin_dirs: vec![],
                npm_cache: std::env::temp_dir().join("jcode-test-npm-cache"),
                tool_dirs: vec![],
            },
            jcode_plugin_core::config::PluginConfig::default(),
            registry,
            runtime,
        );

        let id = PluginId::from("/nonexistent/path/plugin.js".to_string());
        let result = loader.path_for_plugin(&id);
        assert!(result.is_err(), "nonexistent path should error");
    }

    // -----------------------------------------------------------------------
    // reload unit tests (using the fingerprint cache as proxy)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_reload_without_prior_load_seeds_nothing() {
        // Calling reload on an unknown plugin (no prior load) should fail
        // because path_for_plugin won't find the path.
        let registry = Arc::new(PluginRegistry::new(Arc::new(RcuDispatcher::new())));
        let runtime = Arc::new(RuntimeManager::new(RuntimeConfig::default()).unwrap());
        let loader = PluginLoader::new(
            DiscoveryPaths {
                plugin_dirs: vec![],
                npm_cache: std::env::temp_dir().join("jcode-test-npm-cache"),
                tool_dirs: vec![],
            },
            jcode_plugin_core::config::PluginConfig::default(),
            registry,
            runtime,
        );

        let id = PluginId::from("/tmp/jcode-never-loaded.js".to_string());
        let result = loader.reload(&id).await;
        assert!(
            result.is_err(),
            "reload of never-loaded plugin should fail: {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // End-to-end reload test using the real hello-plugin example
    // -----------------------------------------------------------------------
    //
    // This test exercises the full pipeline: load a real plugin, then reload it
    // and verify it's still registered afterward. Because the plugin hasn't
    // changed on disk, the second reload should be a no-op (detected by
    // fingerprint), but we still verify the plugin survives in the registry.

    #[tokio::test(flavor = "current_thread")]
    async fn test_reload_e2e_hello_plugin() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("could not find workspace root from CARGO_MANIFEST_DIR");
        let example_dir = workspace_root.join("examples/plugins/hello-plugin");
        if !example_dir.exists() {
            eprintln!("skipping e2e reload test — hello-plugin dir not found");
            return;
        }

        let dispatcher = Arc::new(RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher.clone()));
        let runtime = Arc::new(
            RuntimeManager::new(RuntimeConfig::default())
                .expect("RuntimeManager::new should succeed"),
        );
        let discovery = DiscoveryPaths {
            plugin_dirs: vec![example_dir.clone()],
            npm_cache: std::env::temp_dir().join("jcode-test-npm-cache-reload"),
            tool_dirs: vec![],
        };
        let config = jcode_plugin_core::config::PluginConfig::default();
        let loader = PluginLoader::new(discovery, config, registry.clone(), runtime);

        // 1. Load all plugins
        let loaded_ids = loader.load_all().await.expect("load_all should succeed");

        assert_eq!(loaded_ids.len(), 1, "expected exactly 1 plugin loaded");
        let plugin_id = &loaded_ids[0];

        // 2. Reload the same plugin (no-op since unchanged)
        let reload_result = loader.reload(plugin_id).await;
        assert!(
            reload_result.is_ok(),
            "reload of unchanged plugin should succeed: {:?}",
            reload_result
        );

        // 3. Verify plugin is still in registry after reload
        let plugins_in_registry = registry.list().await;
        assert_eq!(plugins_in_registry.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_remove_fingerprint_clears_cache() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("could not find workspace root from CARGO_MANIFEST_DIR");
        let example_dir = workspace_root.join("examples/plugins/hello-plugin");
        if !example_dir.exists() {
            eprintln!("skipping test_remove_fingerprint_clears_cache — hello-plugin dir not found");
            return;
        }

        let dispatcher = Arc::new(RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher.clone()));
        let runtime = Arc::new(
            RuntimeManager::new(RuntimeConfig::default())
                .expect("RuntimeManager::new should succeed"),
        );
        let discovery = DiscoveryPaths {
            plugin_dirs: vec![example_dir.clone()],
            npm_cache: std::env::temp_dir().join("jcode-test-npm-cache-fpr"),
            tool_dirs: vec![],
        };
        let config = jcode_plugin_core::config::PluginConfig::default();
        let loader = PluginLoader::new(discovery, config, registry, runtime);

        // Load seeds the fingerprint
        let loaded_ids = loader.load_all().await.expect("load_all should succeed");
        assert!(
            !loaded_ids.is_empty(),
            "expected at least one plugin loaded"
        );
        let plugin_id = &loaded_ids[0];

        let has_entry = { loader.fingerprints.read().await.contains_key(plugin_id) };
        assert!(has_entry, "fingerprint should exist after load");

        // Remove it
        loader.remove_fingerprint(plugin_id).await;

        let has_entry = { loader.fingerprints.read().await.contains_key(plugin_id) };
        assert!(!has_entry, "fingerprint should be removed");
    }
}
