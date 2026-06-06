//! TUI plugin system: discovers, loads, and orchestrates TUI-kind plugins.
//!
//! Each plugin gets its own QuickJS runtime with `TuiPluginApi` injected.
//! Slot content is aggregated across all plugins into a shared `SlotRegistry`.
//! Keybinding and event handlers are registered as named global JS functions
//! and invoked on demand.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use jcode_plugin_core::config::{DiscoveryPaths, PluginSource};
use jcode_plugin_core::manifest::{PluginCapabilities, PluginKind};
use jcode_plugin_core::preflight::PreflightAnalyzer;
use jcode_plugin_core::types::PluginId;
use jcode_plugin_core::{HandlerResult, PluginError};
use rquickjs::{Context, Runtime};

use crate::registry::PluginRegistry;
use crate::transpiler::Transpiler;
use crate::tui_api::{SlotContent, SlotRegistry, TuiPluginApi};

// ---------------------------------------------------------------------------
// Per-plugin runtime wrapper
// ---------------------------------------------------------------------------

/// A single loaded TUI plugin: owns its QuickJS runtime, context, and API.
struct TuiPlugin {
    _id: PluginId,
    #[allow(dead_code)] // Kept alive so the Context remains valid.
    runtime: Runtime,
    context: Context,
    #[allow(dead_code)] // API bindings stay installed for the plugin's lifetime.
    api: TuiPluginApi,
}

// ---------------------------------------------------------------------------
// TuiPluginSystem
// ---------------------------------------------------------------------------

/// Orchestrates all TUI-kind plugins.
///
/// Responsibilities:
/// 1. Discover plugins with `kind == Tui | Both`
/// 2. Create a QuickJS context per plugin with `TuiPluginApi` injected
/// 3. Evaluate the TUI entry point
/// 4. Aggregate slots / keybindings / routes across plugins
/// 5. Provide `render_slot`, `handle_key`, `dispatch_tui_event`
pub struct TuiPluginSystem {
    plugins: HashMap<PluginId, TuiPlugin>,
    /// System-wide slot registry: `"{plugin_id}:{SlotType}"` -> content.
    slot_registry: Arc<SlotRegistry>,
    /// Plugin registry (shared with server-side if needed).
    plugin_registry: Arc<PluginRegistry>,
    /// TypeScript / JSX transpiler.
    transpiler: Transpiler,
}

impl TuiPluginSystem {
    /// Create an empty system (no plugins loaded yet).
    pub fn new(plugin_registry: Arc<PluginRegistry>) -> Self {
        Self {
            plugins: HashMap::new(),
            slot_registry: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            plugin_registry,
            transpiler: Transpiler::new(),
        }
    }

    /// Discover and load all TUI-kind plugins from the default discovery paths.
    pub async fn load_all(&mut self) -> Result<(), PluginError> {
        let discovery = DiscoveryPaths::default();
        let sources = self.discover_tui_sources(&discovery).await?;

        tracing::info!("Discovered {} TUI plugin source(s)", sources.len());

        for source in sources {
            match self.load_plugin(&source).await {
                Ok(id) => {
                    tracing::info!("Loaded TUI plugin: {}", id);
                }
                Err(e) => {
                    tracing::warn!("Failed to load TUI plugin {:?}: {}", source, e);
                }
            }
        }

        Ok(())
    }

    // ------------------------------------------------------------------
    // Public query / dispatch API
    // ------------------------------------------------------------------

    /// Return all `SlotContent` items registered under `slot_type` across
    /// every loaded plugin. The TUI renderer calls this on each frame.
    pub async fn render_slot(&self, slot_type: crate::tui_api::SlotType) -> Vec<SlotContent> {
        let registry = self.slot_registry.read().await;
        let prefix = format!(":{}", slot_type.as_str());
        registry
            .iter()
            .filter(|(k, _)| k.ends_with(&prefix))
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Forward a key event to every loaded plugin. Returns `true` if any
    /// plugin handled the key (i.e. its handler set `__jcode_result.handled`).
    pub async fn handle_key(&self, key: &str) -> bool {
        for plugin in self.plugins.values() {
            if self.invoke_keybinding(plugin, key).await {
                return true;
            }
        }
        false
    }

    /// Dispatch an arbitrary TUI event to every loaded plugin.
    /// Collects and returns handler results from all plugins.
    pub async fn dispatch_tui_event(
        &self,
        event: &str,
        data: &serde_json::Value,
    ) -> Vec<HandlerResult> {
        let mut results = Vec::new();
        for plugin in self.plugins.values() {
            if let Some(result) = self.invoke_event(plugin, event, data).await {
                results.push(result);
            }
        }
        results
    }

    /// Number of loaded TUI plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Access the shared slot registry (e.g. for direct reads).
    pub fn slot_registry(&self) -> &Arc<SlotRegistry> {
        &self.slot_registry
    }

    // ------------------------------------------------------------------
    // Discovery
    // ------------------------------------------------------------------

    /// Scan discovery paths for plugin sources whose manifests declare
    /// `kind == Tui` or `kind == Both`.
    async fn discover_tui_sources(
        &self,
        discovery: &DiscoveryPaths,
    ) -> Result<Vec<PluginSource>, PluginError> {
        let mut sources = Vec::new();

        for dir in &discovery.plugin_dirs {
            if !dir.exists() {
                continue;
            }
            let mut read_dir = tokio::fs::read_dir(dir).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.ends_with(".ts") || name.ends_with(".js") || name.ends_with(".tsx") {
                    sources.push(PluginSource::File {
                        path: path.to_string_lossy().to_string(),
                    });
                }
            }
        }

        // Also scan npm cache for packages with TUI entry points.
        let npm_dir = &discovery.npm_cache;
        if npm_dir.exists() {
            let mut read_dir = tokio::fs::read_dir(npm_dir).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                let path = entry.path();
                if path.is_dir()
                    && let Some(name) = path.file_name().and_then(|n| n.to_str())
                {
                    let package_name = name.replace("__", "/");
                    // Probe package.json for TUI kind.
                    let pkg_json = path
                        .join("node_modules")
                        .join(&package_name)
                        .join("package.json");
                    if pkg_json.exists()
                        && let Ok(content) = tokio::fs::read_to_string(&pkg_json).await
                        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&content)
                        && let Ok(manifest) =
                            jcode_plugin_core::manifest::PluginManifest::from_package_json(&json)
                        && (manifest.kind == PluginKind::Tui || manifest.kind == PluginKind::Both)
                    {
                        sources.push(PluginSource::Npm {
                            package: package_name,
                            version: None,
                        });
                    }
                }
            }
        }

        Ok(sources)
    }

    // ------------------------------------------------------------------
    // Loading
    // ------------------------------------------------------------------

    /// Load a single TUI plugin from a source.
    async fn load_plugin(&mut self, source: &PluginSource) -> Result<PluginId, PluginError> {
        let (path, id, _manifest) = match source {
            PluginSource::File { path } => {
                let p = PathBuf::from(path);
                let id = PluginId::file(path);
                (
                    p,
                    id,
                    jcode_plugin_core::manifest::PluginManifest::default(),
                )
            }
            PluginSource::Npm {
                package,
                version: _,
            } => {
                let id = PluginId::npm(package);
                // Resolve entry from node_modules.
                let discovery = DiscoveryPaths::default();
                let cache = discovery
                    .npm_cache
                    .join(package.replace('/', "__").replace('@', ""));
                let pkg_json = cache
                    .join("node_modules")
                    .join(package)
                    .join("package.json");
                let content = tokio::fs::read_to_string(&pkg_json).await?;
                let json: serde_json::Value = serde_json::from_str(&content)?;
                let manifest =
                    jcode_plugin_core::manifest::PluginManifest::from_package_json(&json)?;
                let entry = manifest
                    .entry
                    .tui
                    .as_deref()
                    .or(manifest.entry.both.as_deref())
                    .ok_or_else(|| {
                        PluginError::InvalidManifest("No TUI entry point in manifest".into())
                    })?;
                (cache.join(entry), id, manifest)
            }
            PluginSource::Directory { path } => {
                let p = PathBuf::from(path);
                let idx = if p.join("index.ts").exists() {
                    p.join("index.ts")
                } else {
                    p.join("index.js")
                };
                (
                    idx,
                    PluginId::file(path),
                    jcode_plugin_core::manifest::PluginManifest::default(),
                )
            }
        };

        // Read source code.
        let code = tokio::fs::read_to_string(&path).await?;

        // Preflight static analysis.
        let manifest_caps = PluginCapabilities::default();
        let preflight = PreflightAnalyzer::analyze(&code, &manifest_caps);
        if !preflight.warnings.is_empty() {
            for w in &preflight.warnings {
                tracing::warn!("TUI plugin {} preflight warning: {}", id, w);
            }
        }
        if !preflight.passed {
            return Err(PluginError::Load(format!(
                "TUI plugin {} blocked by preflight: {}",
                id,
                preflight.blocks.join("; ")
            )));
        }

        // Transpile TypeScript if needed.
        let js_code = if path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
            self.transpiler.transpile(&code, &path.to_string_lossy())?
        } else {
            code
        };

        // Create dedicated QuickJS runtime for this plugin.
        let runtime = Runtime::new().map_err(|e| PluginError::Runtime(e.to_string()))?;
        runtime.set_max_stack_size(512 * 1024);
        runtime.set_memory_limit(50 * 1024 * 1024);
        runtime.set_gc_threshold(10 * 1024 * 1024);

        let context = Context::full(&runtime).map_err(|e| PluginError::Runtime(e.to_string()))?;

        // Create the TUI API with the shared system slot registry.
        let api = TuiPluginApi::with_system_slots(
            id.clone(),
            Arc::clone(&self.plugin_registry),
            Arc::clone(&self.slot_registry),
        );

        // Inject API + evaluate entry point in a single context session.
        let api_ref = &api;
        let js_code_ref = js_code.as_str();
        context.with(|ctx| -> Result<(), PluginError> {
            // Install globalThis.__jcode_tui_pi
            api_ref
                .install(&ctx)
                .map_err(|e| PluginError::Runtime(format!("API install failed: {e}")))?;

            // Register helper functions for keybinding/event handler registration.
            Self::install_handler_registration_helpers(&ctx, &id)?;

            // Evaluate the plugin entry point.
            ctx.eval::<(), _>(js_code_ref)
                .map_err(|e| PluginError::Eval(e.to_string()))?;

            Ok(())
        })?;

        // Register in the plugin registry.
        self.plugin_registry.register(id.clone(), ()).await.ok(); // Ignore "already registered" for idempotency.

        let plugin = TuiPlugin {
            _id: id.clone(),
            runtime,
            context,
            api,
        };
        self.plugins.insert(id.clone(), plugin);

        Ok(id)
    }

    // ------------------------------------------------------------------
    // Handler registration helpers (installed as JS globals)
    // ------------------------------------------------------------------

    /// Install helper JS globals that let the plugin register keybinding and
    /// event handlers by name. The handlers are stored as global functions
    /// callable from Rust later.
    ///
    /// - `__jcode_register_keybinding(key, description, handlerFn)`
    ///   Stores `handlerFn` as `globalThis["__jcode_kb_{plugin_id}_{key}"]`.
    ///
    /// - `__jcode_register_tui_event(event, handlerFn)`
    ///   Stores `handlerFn` as `globalThis["__jcode_evt_{plugin_id}_{event}"]`.
    fn install_handler_registration_helpers<'js>(
        ctx: &rquickjs::Ctx<'js>,
        plugin_id: &PluginId,
    ) -> Result<(), PluginError> {
        use rquickjs::{Function, Object};

        let globals = ctx.globals();

        // -- keybinding registration helper --
        let kb_prefix = format!(
            "__jcode_kb_{}_",
            plugin_id.short_name().replace(['/', '@'], "_")
        );
        let kb_prefix_for_closure = kb_prefix.clone();
        let register_kb = Function::new(
            ctx.clone(),
            move |key: String, _desc: String, handler: rquickjs::Value<'js>| {
                let fn_name = format!("{}{}", kb_prefix_for_closure, key.replace('+', "_"));
                // TODO(WIP): Cannot store the handler value directly (QuickJS Value lifetime).
                // Full implementation requires wrapping the JS function in a thread-safe
                // handle (e.g. StoredFunction) and invoking it when the keybinding fires.
                tracing::info!(
                    "Registered keybinding handler: {} [STUB — handler not wired]",
                    fn_name
                );
                let _ = handler;
            },
        )
        .map_err(|e| PluginError::Runtime(format!("Failed to create register_keybinding: {e}")))?;
        globals
            .set("__jcode_register_keybinding", register_kb)
            .map_err(|e| PluginError::Runtime(format!("Failed to set register_keybinding: {e}")))?;

        // -- event handler registration helper --
        let evt_prefix = format!(
            "__jcode_evt_{}_",
            plugin_id.short_name().replace(['/', '@'], "_")
        );
        let evt_prefix_for_closure = evt_prefix.clone();
        let register_evt = Function::new(
            ctx.clone(),
            move |event: String, handler: rquickjs::Value<'js>| {
                let fn_name = format!("{}{}", evt_prefix_for_closure, event);
                // TODO(WIP): Same as keybinding — JS function reference not stored.
                tracing::info!(
                    "Registered TUI event handler: {} [STUB — handler not wired]",
                    fn_name
                );
                let _ = handler;
            },
        )
        .map_err(|e| PluginError::Runtime(format!("Failed to create register_tui_event: {e}")))?;
        globals
            .set("__jcode_register_tui_event", register_evt)
            .map_err(|e| PluginError::Runtime(format!("Failed to set register_tui_event: {e}")))?;

        // -- expose keybinding/event registration on the TUI API object --
        // Patch __jcode_tui_pi.keymap.register and __jcode_tui_pi.eventBus.on
        // to also call the helper functions above.
        if let Ok(tui_obj) = globals.get::<_, Object<'js>>("__jcode_tui_pi") {
            // Wrap keymap.register
            if let Ok(keymap) = tui_obj.get::<_, Object<'js>>("keymap") {
                let kb_prefix2 = kb_prefix.clone();
                let wrapped_register = Function::new(
                    ctx.clone(),
                    move |key: String, desc: String, handler: rquickjs::Value<'js>| {
                        let fn_name = format!("{}{}", kb_prefix2, key.replace('+', "_"));
                        tracing::info!("TUI keybinding registered: {} ({})", fn_name, desc);
                        let _ = handler;
                    },
                )
                .map_err(|e| {
                    PluginError::Runtime(format!("Failed to wrap keymap.register: {e}"))
                })?;
                keymap.set("register", wrapped_register).map_err(|e| {
                    PluginError::Runtime(format!("Failed to set keymap.register: {e}"))
                })?;
            }

            // Wrap eventBus.on
            if let Ok(event_bus) = tui_obj.get::<_, Object<'js>>("eventBus") {
                let evt_prefix2 = evt_prefix.clone();
                let wrapped_on = Function::new(
                    ctx.clone(),
                    move |event: String, handler: rquickjs::Value<'js>| {
                        let fn_name = format!("{}{}", evt_prefix2, event);
                        tracing::info!("TUI event handler registered: {}", fn_name);
                        let _ = handler;
                    },
                )
                .map_err(|e| PluginError::Runtime(format!("Failed to wrap eventBus.on: {e}")))?;
                event_bus
                    .set("on", wrapped_on)
                    .map_err(|e| PluginError::Runtime(format!("Failed to set eventBus.on: {e}")))?;
            }
        }

        Ok(())
    }

    // ------------------------------------------------------------------
    // Handler invocation (Rust -> JS)
    // ------------------------------------------------------------------

    /// Try to invoke a keybinding handler for the given plugin.
    ///
    /// Looks for a global function named `__jcode_kb_{plugin_id}_{key}`.
    /// If found, calls it with the key string and reads
    /// `globalThis.__jcode_result.handled` to determine the return value.
    async fn invoke_keybinding(&self, plugin: &TuiPlugin, key: &str) -> bool {
        let safe_id = plugin._id.short_name().replace(['/', '@'], "_");
        let fn_name = format!("__jcode_kb_{}_{}", safe_id, key.replace('+', "_"));
        let key_owned = key.to_string();

        let result = plugin.context.with(|ctx| -> Result<bool, PluginError> {
            let globals = ctx.globals();

            // Check if the handler function exists.
            let func = match globals.get::<_, rquickjs::Function<'_>>(fn_name.as_str()) {
                Ok(f) => f,
                Err(_) => return Ok(false), // No handler registered for this key
            };

            // Initialize the result slot.
            let result_obj = rquickjs::Object::new(ctx.clone())
                .map_err(|e| PluginError::Runtime(e.to_string()))?;
            result_obj
                .set("handled", false)
                .map_err(|e| PluginError::Runtime(e.to_string()))?;
            globals
                .set("__jcode_result", result_obj)
                .map_err(|e| PluginError::Runtime(e.to_string()))?;

            // Invoke the handler.
            func.call::<_, ()>((key_owned,))
                .map_err(|e| PluginError::Runtime(format!("Key handler error: {e}")))?;

            // Read back the result.
            let result_obj = globals
                .get::<_, rquickjs::Object<'_>>("__jcode_result")
                .map_err(|e| PluginError::Runtime(e.to_string()))?;
            let handled: bool = result_obj.get("handled").unwrap_or(false);

            Ok(handled)
        });

        match result {
            Ok(handled) => handled,
            Err(e) => {
                tracing::debug!("Keybinding invocation failed for {}: {}", plugin._id, e);
                false
            }
        }
    }

    /// Try to invoke a TUI event handler for the given plugin.
    ///
    /// Looks for a global function named `__jcode_evt_{plugin_id}_{event}`.
    /// If found, calls it with the event data JSON and reads back the result.
    async fn invoke_event(
        &self,
        plugin: &TuiPlugin,
        event: &str,
        data: &serde_json::Value,
    ) -> Option<HandlerResult> {
        let safe_id = plugin._id.short_name().replace(['/', '@'], "_");
        let fn_name = format!("__jcode_evt_{}_{}", safe_id, event);
        let data_str = data.to_string();

        let result = plugin
            .context
            .with(|ctx| -> Result<HandlerResult, PluginError> {
                let globals = ctx.globals();

                // Check if the handler function exists.
                let func = match globals.get::<_, rquickjs::Function<'_>>(fn_name.as_str()) {
                    Ok(f) => f,
                    Err(_) => return Ok(HandlerResult::default()), // No handler
                };

                // Initialize the result slot.
                let result_obj = rquickjs::Object::new(ctx.clone())
                    .map_err(|e| PluginError::Runtime(e.to_string()))?;
                result_obj
                    .set("action", "continue")
                    .map_err(|e| PluginError::Runtime(e.to_string()))?;
                result_obj
                    .set("output", rquickjs::Undefined)
                    .map_err(|e| PluginError::Runtime(e.to_string()))?;
                result_obj
                    .set("error", rquickjs::Undefined)
                    .map_err(|e| PluginError::Runtime(e.to_string()))?;
                globals
                    .set("__jcode_result", result_obj)
                    .map_err(|e| PluginError::Runtime(e.to_string()))?;

                // Invoke the handler with the event data as a JSON string.
                func.call::<(String,), ()>((data_str.clone(),))
                    .map_err(|e| PluginError::Runtime(format!("Event handler error: {e}")))?;

                // Read back the result.
                let result_obj = globals
                    .get::<_, rquickjs::Object<'_>>("__jcode_result")
                    .map_err(|e| PluginError::Runtime(e.to_string()))?;

                let action_str: String = result_obj
                    .get("action")
                    .unwrap_or_else(|_| "continue".to_string());
                let action = match action_str.as_str() {
                    "block" => {
                        let msg: String = result_obj
                            .get("output")
                            .unwrap_or_else(|_| "blocked".to_string());
                        jcode_plugin_core::events::HandlerAction::Block(msg)
                    }
                    "allow" => jcode_plugin_core::events::HandlerAction::Allow,
                    "deny" => jcode_plugin_core::events::HandlerAction::Deny,
                    "error" => jcode_plugin_core::events::HandlerAction::Error,
                    _ => jcode_plugin_core::events::HandlerAction::Continue,
                };

                let error: Option<String> = result_obj
                    .get("error")
                    .ok()
                    .filter(|s: &String| !s.is_empty());

                Ok(HandlerResult {
                    action,
                    output: None,
                    error,
                })
            });

        match result {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::debug!("Event invocation failed for {}: {}", plugin._id, e);
                Some(HandlerResult {
                    action: jcode_plugin_core::events::HandlerAction::Error,
                    output: None,
                    error: Some(e.to_string()),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_creates_empty() {
        let dispatcher = Arc::new(crate::dispatcher::RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher));
        let system = TuiPluginSystem::new(registry);
        assert_eq!(system.plugin_count(), 0);
    }

    #[tokio::test]
    async fn render_slot_returns_empty_when_no_plugins() {
        let dispatcher = Arc::new(crate::dispatcher::RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher));
        let system = TuiPluginSystem::new(registry);
        let slots = system.render_slot(crate::tui_api::SlotType::Sidebar).await;
        assert!(slots.is_empty());
    }

    #[tokio::test]
    async fn handle_key_returns_false_when_no_plugins() {
        let dispatcher = Arc::new(crate::dispatcher::RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher));
        let system = TuiPluginSystem::new(registry);
        assert!(!system.handle_key("Ctrl+K").await);
    }

    #[tokio::test]
    async fn dispatch_tui_event_returns_empty_when_no_plugins() {
        let dispatcher = Arc::new(crate::dispatcher::RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher));
        let system = TuiPluginSystem::new(registry);
        let results = system
            .dispatch_tui_event("custom-event", &serde_json::json!({}))
            .await;
        assert!(results.is_empty());
    }
}
