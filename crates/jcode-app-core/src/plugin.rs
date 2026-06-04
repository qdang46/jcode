use jcode_plugin_core::events::{EventInput, HandlerAction};
use jcode_plugin_core::PluginEvent;
pub use jcode_plugin_runtime::{check_kill_switches, is_force_deny, PluginSystem, DISABLE_ALL_PLUGINS, FORCE_DENY, SKIP_HOOKS};
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

static PLUGIN_SYSTEM: OnceLock<PluginSystem> = OnceLock::new();

pub async fn init_plugins(config: &crate::config::PluginConfig) {
    if PLUGIN_SYSTEM.get().is_some() {
        return;
    }
    crate::logging::info("Initializing plugin system");
    match PluginSystem::initialize(config).await {
        Ok(system) => {
            crate::logging::info("Plugin system initialized successfully");
            let _ = PLUGIN_SYSTEM.set(system);
        }
        Err(e) => {
            crate::logging::warn(&format!("Plugin system initialization failed: {e}"));
        }
    }
}

pub fn plugin_system() -> Option<&'static PluginSystem> {
    PLUGIN_SYSTEM.get()
}

pub fn plugin_count() -> usize {
    plugin_system()
        .map(|sys| sys.dispatcher.plugin_count())
        .unwrap_or(0)
}

pub enum PermissionVerdict {
    Allow,
    Deny,
    Defer,
}

pub async fn check_permission(action: &str, args: &serde_json::Value) -> PermissionVerdict {
    if DISABLE_ALL_PLUGINS.load(Ordering::SeqCst) {
        return PermissionVerdict::Defer;
    }

    if is_force_deny() {
        return PermissionVerdict::Deny;
    }

    let sys = match PLUGIN_SYSTEM.get() {
        Some(s) => s,
        None => return PermissionVerdict::Defer,
    };

    if SKIP_HOOKS.load(Ordering::SeqCst) {
        return PermissionVerdict::Defer;
    }

    let tool_name = args
        .get("tool")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let event = PluginEvent::PermissionRequest;
    let input = EventInput::PermissionRequest {
        action: action.to_string(),
        tool_name,
        target,
        session_id: String::new(),
    };

    let results = sys.dispatch_event(event, input, None).await;

    for (_id, result) in &results {
        match &result.action {
            HandlerAction::Deny => return PermissionVerdict::Deny,
            HandlerAction::Allow => return PermissionVerdict::Allow,
            _ => continue,
        }
    }

    PermissionVerdict::Defer
}
