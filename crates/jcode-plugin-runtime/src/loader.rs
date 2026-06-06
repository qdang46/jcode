use crate::registry::PluginRegistry;
use crate::runtime::RuntimeManager;
use crate::transpiler::Transpiler;
use crate::types::ResolvedEntry;
use jcode_plugin_core::PluginError;
use jcode_plugin_core::config::{
    DiscoveryPaths, PluginConfig, PluginSource, is_valid_package_name,
};
use jcode_plugin_core::preflight::PreflightAnalyzer;
use jcode_plugin_core::types::PluginId;
use std::path::Path;
use std::sync::Arc;

pub struct PluginLoader {
    discovery: DiscoveryPaths,
    config: PluginConfig,
    registry: Arc<PluginRegistry>,
    transpiler: Arc<Transpiler>,
    runtime: Arc<RuntimeManager>,
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

    async fn discover_sources(&self) -> Result<Vec<PluginSource>, PluginError> {
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
        sources: &mut Vec<PluginSource>,
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
                sources.push(PluginSource::File {
                    path: path.to_string_lossy().to_string(),
                });
            }
        }
        Ok(())
    }

    async fn scan_npm_cache(
        &self,
        dir: &Path,
        sources: &mut Vec<PluginSource>,
    ) -> Result<(), PluginError> {
        let mut read_dir = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    let package_name = name.replace("__", "/");
                    sources.push(PluginSource::Npm {
                        package: package_name,
                        version: None,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn load_one(&self, source: &PluginSource) -> Result<PluginId, PluginError> {
        let (path, id) = match source {
            PluginSource::Npm { package, version } => {
                let entry = self.resolve_npm_entry(package, version.as_deref()).await?;
                (entry.path, PluginId::npm(package))
            }
            PluginSource::File { path } => (std::path::PathBuf::from(path), PluginId::file(path)),
            PluginSource::Directory { path } => {
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

        let js_code = if path.extension().map_or(false, |e| e == "ts" || e == "tsx") {
            self.transpiler.transpile(&code, &path.to_string_lossy())?
        } else {
            code
        };

        let context = self.runtime.create_sandbox(
            id.clone(),
            jcode_plugin_core::manifest::PluginManifest::default(),
        )?;
        context.eval(&js_code).await?;
        self.registry.register(id.clone(), ()).await?;
        Ok(id)
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
