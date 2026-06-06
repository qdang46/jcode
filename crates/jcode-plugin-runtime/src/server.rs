use crate::audit::AuditTrail;
use crate::dispatcher::RcuDispatcher;
use crate::loader::PluginLoader;
use crate::registry::PluginRegistry;
use crate::runtime::{RuntimeConfig, RuntimeManager};
use crate::transpiler::Transpiler;
use jcode_plugin_core::PluginEvent;
use jcode_plugin_core::config::{DiscoveryPaths, PluginConfig};
use jcode_plugin_core::events::{EventInput, HandlerAction};
use jcode_plugin_core::types::PluginId;
use std::sync::Arc;

pub static DISABLE_ALL_PLUGINS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub static SKIP_HOOKS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static FORCE_DENY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_force_deny() -> bool {
    FORCE_DENY.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn check_kill_switches() {
    use std::sync::atomic::Ordering;
    if std::env::var("JCODE_DISABLE_PLUGINS").is_ok() {
        DISABLE_ALL_PLUGINS.store(true, Ordering::SeqCst);
    }
    if std::env::var("JCODE_SKIP_PLUGINS").is_ok() {
        SKIP_HOOKS.store(true, Ordering::SeqCst);
    }
    if std::env::var("JCODE_TEAM_WORKER").is_ok() {
        FORCE_DENY.store(true, Ordering::SeqCst);
    }
}

pub struct PluginSystem {
    pub dispatcher: Arc<RcuDispatcher>,
    pub registry: Arc<PluginRegistry>,
    pub runtime: Arc<RuntimeManager>,
    pub loader: PluginLoader,
    pub _transpiler: Arc<Transpiler>,
    pub audit_trail: AuditTrail,
}

impl PluginSystem {
    pub async fn initialize(config: &PluginConfig) -> Result<Self, jcode_plugin_core::PluginError> {
        check_kill_switches();

        if DISABLE_ALL_PLUGINS.load(std::sync::atomic::Ordering::SeqCst) {
            tracing::info!("Plugins disabled via JCODE_DISABLE_PLUGINS");
        }

        let rt_config = RuntimeConfig::default();
        let runtime = Arc::new(RuntimeManager::new(rt_config)?);
        let dispatcher = Arc::new(RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(Arc::clone(&dispatcher)));
        let transpiler = Arc::new(Transpiler::new());
        let discovery = DiscoveryPaths::default();

        let loader = PluginLoader::new(
            discovery,
            config.clone(),
            Arc::clone(&registry),
            Arc::clone(&runtime),
        );

        let loaded = loader.load_all().await?;
        tracing::info!("Loaded {} server plugin(s)", loaded.len());

        Ok(Self {
            dispatcher,
            registry,
            runtime,
            loader,
            _transpiler: transpiler,
            audit_trail: AuditTrail::new(1000),
        })
    }

    pub async fn dispatch_event(
        &self,
        event: PluginEvent,
        input: jcode_plugin_core::events::EventInput,
        output: Option<jcode_plugin_core::events::EventOutput>,
    ) -> Vec<(PluginId, jcode_plugin_core::events::HandlerResult)> {
        if SKIP_HOOKS.load(std::sync::atomic::Ordering::SeqCst) {
            return Vec::new();
        }
        self.dispatcher.dispatch(event, input, output).await
    }

    pub async fn execute_tool(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> Result<String, String> {
        use std::sync::atomic::Ordering;

        if DISABLE_ALL_PLUGINS.load(Ordering::SeqCst) {
            return Err("Plugins are disabled via JCODE_DISABLE_PLUGINS".to_string());
        }

        if FORCE_DENY.load(Ordering::SeqCst) {
            return Err("Plugin tool execution denied via JCODE_TEAM_WORKER".to_string());
        }

        if SKIP_HOOKS.load(Ordering::SeqCst) {
            return Ok(format!(
                "Plugin tool '{tool_name}' executed (hooks skipped)"
            ));
        }

        let event = PluginEvent::PreToolUse;
        let event_input = EventInput::PreToolUse {
            tool_name: tool_name.to_string(),
            tool_input: input.clone(),
            session_id: String::new(),
        };

        let results = self.dispatcher.dispatch(event, event_input, None).await;

        for (_id, result) in &results {
            if let HandlerAction::Block(reason) = &result.action {
                return Err(format!("Plugin blocked tool execution: {reason}"));
            }
        }

        Ok(format!("Plugin tool '{tool_name}' executed successfully"))
    }

    pub fn has_handler(&self, event: PluginEvent) -> bool {
        if SKIP_HOOKS.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        self.dispatcher.has_handler(event)
    }

    pub async fn list_plugins(&self) -> Vec<(PluginId, String)> {
        self.registry.list().await
    }

    pub async fn install(&self, source: &str) -> Result<(), jcode_plugin_core::PluginError> {
        use jcode_plugin_core::config::PluginSource;

        let source = if source.starts_with('/') || source.starts_with('.') {
            PluginSource::File {
                path: source.to_string(),
            }
        } else {
            PluginSource::Npm {
                package: source.to_string(),
                version: None,
            }
        };

        let id = self.loader.load_one(&source).await?;
        tracing::info!("Plugin installed: {id}");
        Ok(())
    }

    pub async fn uninstall_by_id(
        &self,
        id: &PluginId,
    ) -> Result<(), jcode_plugin_core::PluginError> {
        self.registry.unregister(id).await;
        tracing::info!("Plugin uninstalled: {id}");
        Ok(())
    }

    pub async fn uninstall(&self, id_str: &str) -> Result<(), jcode_plugin_core::PluginError> {
        let id = PluginId::from(id_str.to_string());
        self.uninstall_by_id(&id).await
    }

    /// TODO(WIP): Re-enable a previously disabled plugin.
    /// Currently only commits the dispatcher but does not re-register handlers
    /// that were removed during disable. Full implementation should store the
    /// plugin's handler registrations and replay them on enable.
    pub async fn enable_plugin(&self, id_str: &str) -> Result<(), jcode_plugin_core::PluginError> {
        let id = PluginId::from(id_str.to_string());
        self.dispatcher.commit();
        tracing::info!("Plugin enabled: {id} [STUB — handlers not re-registered]");
        Ok(())
    }

    pub async fn disable_plugin(&self, id_str: &str) -> Result<(), jcode_plugin_core::PluginError> {
        let id = PluginId::from(id_str.to_string());
        self.dispatcher.unregister_plugin(&id);
        tracing::info!("Plugin disabled: {id}");
        Ok(())
    }

    pub fn audit_trail(&self) -> &AuditTrail {
        &self.audit_trail
    }
}
