use crate::bridge::PromiseBridge;
use crate::registry::PluginRegistry;
use crate::types::HandlerSlot;
use jcode_plugin_core::PluginEvent;
use jcode_plugin_core::manifest::PluginManifest;
use jcode_plugin_core::security::CapabilityChain;
use jcode_plugin_core::types::PluginId;
use rquickjs::{Ctx, Function, Object, Value};
use std::sync::Arc;

pub struct PluginApiBindings {
    plugin_id: PluginId,
    _manifest: PluginManifest,
    _capability_chain: Arc<CapabilityChain>,
    registry: Arc<PluginRegistry>,
    _bridge: Arc<PromiseBridge>,
}

impl PluginApiBindings {
    pub fn new(
        plugin_id: PluginId,
        manifest: PluginManifest,
        capability_chain: Arc<CapabilityChain>,
        registry: Arc<PluginRegistry>,
        bridge: Arc<PromiseBridge>,
    ) -> Self {
        Self {
            plugin_id,
            _manifest: manifest,
            _capability_chain: capability_chain,
            registry,
            _bridge: bridge,
        }
    }

    pub fn install<'js>(&self, ctx: &Ctx<'js>) -> Result<(), rquickjs::Error> {
        let pi = Object::new(ctx.clone())?;
        pi.set("id", self.plugin_id.to_string())?;
        pi.set("name", self.plugin_id.to_string())?;
        pi.set("version", "0.1.0")?;
        pi.set("on", self.make_on_fn(ctx)?)?;
        pi.set("registerTool", self.make_register_tool_fn(ctx)?)?;
        pi.set("getConfig", self.make_get_config_fn(ctx)?)?;
        pi.set("logger", self.make_logger(ctx)?)?;

        let kv = Object::new(ctx.clone())?;
        kv.set("get", self.make_kv_get_fn(ctx)?)?;
        kv.set("set", self.make_kv_set_fn(ctx)?)?;
        pi.set("kv", kv)?;

        pi.set("sleep", self.make_sleep_fn(ctx)?)?;
        pi.set("uuid", self.make_uuid_fn(ctx)?)?;
        pi.set(
            "cwd",
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
        )?;
        let handlers = Object::new(ctx.clone())?;
        pi.set("_handlers", handlers)?;
        ctx.globals().set("__jcode_pi", pi)?;
        self._bridge.install(ctx)?;
        Ok(())
    }

    fn make_on_fn<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, rquickjs::Error> {
        let registry = Arc::clone(&self.registry);
        let plugin_id = self.plugin_id.clone();

        Function::new(ctx.clone(), move |event: String, _handler: Value<'js>| {
            let event_variant = match event.as_str() {
                "PreToolUse" => PluginEvent::PreToolUse,
                "PostToolUse" => PluginEvent::PostToolUse,
                "PostToolUseFailure" => PluginEvent::PostToolUseFailure,
                "ToolExecutionStart" => PluginEvent::ToolExecutionStart,
                "ToolExecutionEnd" => PluginEvent::ToolExecutionEnd,
                "SessionStart" => PluginEvent::SessionStart,
                "SessionEnd" => PluginEvent::SessionEnd,
                "SessionSwitch" => PluginEvent::SessionSwitch,
                "SessionCompact" => PluginEvent::SessionCompact,
                "SessionBeforeCompact" => PluginEvent::SessionBeforeCompact,
                "SessionShutdown" => PluginEvent::SessionShutdown,
                "PermissionRequest" => PluginEvent::PermissionRequest,
                "PermissionDenied" => PluginEvent::PermissionDenied,
                "AgentStart" => PluginEvent::AgentStart,
                "AgentEnd" => PluginEvent::AgentEnd,
                "TurnStart" => PluginEvent::TurnStart,
                "TurnEnd" => PluginEvent::TurnEnd,
                "MessageStart" => PluginEvent::MessageStart,
                "MessageEnd" => PluginEvent::MessageEnd,
                "PreCompact" => PluginEvent::PreCompact,
                "PostCompact" => PluginEvent::PostCompact,
                "TaskCreated" => PluginEvent::TaskCreated,
                "TaskCompleted" => PluginEvent::TaskCompleted,
                "AutoCompactionStart" => PluginEvent::AutoCompactionStart,
                "UserPromptSubmit" => PluginEvent::UserPromptSubmit,
                "Stop" => PluginEvent::Stop,
                "Notification" => PluginEvent::Notification,
                _ => {
                    tracing::warn!(
                        "Plugin {} registered handler for unknown event: {}",
                        plugin_id,
                        event
                    );
                    return;
                }
            };

            tracing::debug!(
                "Plugin {} registered handler for event: {}",
                plugin_id,
                event
            );

            // Create a Rust handler slot that wraps the JS handler invocation.
            // The actual JS function call happens in the sandbox's call_handler method.
            //
            // TODO(WIP): The JS handler function (`_handler`) is received but not yet
            // wired into the Rust closure. Currently returns HandlerResult::default().
            // Full JS-to-Rust bridge requires storing the JS function reference in a
            // thread-safe handle and invoking it via QuickJS context during dispatch.
            let id = plugin_id.clone();
            let slot = HandlerSlot::Rust(Arc::new(move |_input, _output| {
                let id = id.clone();
                Box::pin(async move {
                    tracing::debug!("Handler invoked for plugin {} (Rust adapter) [STUB]", id);
                    jcode_plugin_core::events::HandlerResult::default()
                })
            }));

            registry.register_handler(event_variant, plugin_id.clone(), slot);
        })
    }

    fn make_register_tool_fn<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, rquickjs::Error> {
        let registry = Arc::clone(&self.registry);
        let id = self.plugin_id.clone();
        Function::new(ctx.clone(), move |tool_def: Object<'js>| {
            let name: String = match tool_def.get("name") {
                Ok(n) => n,
                Err(_) => {
                    tracing::warn!("Plugin {} tried to register tool without name", id);
                    return;
                }
            };
            tracing::info!("Plugin {} registered tool: {}", id, name);
            let id = id.clone();
            registry.register_js_tool(id, name, tool_def);
        })
    }

    fn make_get_config_fn<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, rquickjs::Error> {
        Function::new(ctx.clone(), |_key: String| "")
    }

    fn make_logger<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let logger = Object::new(ctx.clone())?;
        logger.set(
            "info",
            Function::new(ctx.clone(), |msg: String| {
                tracing::info!("[plugin] {}", msg);
            })?,
        )?;
        logger.set(
            "warn",
            Function::new(ctx.clone(), |msg: String| {
                tracing::warn!("[plugin] {}", msg);
            })?,
        )?;
        logger.set(
            "error",
            Function::new(ctx.clone(), |msg: String| {
                tracing::error!("[plugin] {}", msg);
            })?,
        )?;
        logger.set(
            "debug",
            Function::new(ctx.clone(), |msg: String| {
                tracing::debug!("[plugin] {}", msg);
            })?,
        )?;
        Ok(logger)
    }

    fn make_kv_get_fn<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, rquickjs::Error> {
        Function::new(ctx.clone(), |_key: String| "")
    }

    fn make_kv_set_fn<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, rquickjs::Error> {
        Function::new(ctx.clone(), |_key: String, _value: Value<'js>| {})
    }

    fn make_sleep_fn<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, rquickjs::Error> {
        // Cap sleep duration to prevent plugins from blocking the QuickJS thread indefinitely.
        // 5 seconds is generous for plugin-side delays; anything longer should use async timers.
        const MAX_SLEEP_MS: u64 = 5_000;
        Function::new(ctx.clone(), |ms: u64| {
            let capped = ms.min(MAX_SLEEP_MS);
            std::thread::sleep(std::time::Duration::from_millis(capped));
        })
    }

    fn make_uuid_fn<'js>(&self, ctx: &Ctx<'js>) -> Result<Function<'js>, rquickjs::Error> {
        Function::new(ctx.clone(), || uuid::Uuid::new_v4().to_string())
    }
}
