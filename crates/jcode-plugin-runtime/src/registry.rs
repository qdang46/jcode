use crate::dispatcher::RcuDispatcher;
use crate::types::HandlerSlot;
use jcode_plugin_core::PluginEvent;
use jcode_plugin_core::types::PluginId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct PluginRegistry {
    plugins: RwLock<HashMap<PluginId, PluginRegistration>>,
    dispatcher: Arc<RcuDispatcher>,
    _js_tools: RwLock<HashMap<String, (PluginId, String)>>,
}

struct PluginRegistration {
    _id: PluginId,
    _state: PluginState,
    _tools: Vec<String>,
}

#[allow(dead_code)]
enum PluginState {
    Active,
    Error(String),
    Disabled,
}

#[derive(Default)]
pub struct JsToolRegistry {
    tools: RwLock<HashMap<String, JsToolEntry>>,
}

#[allow(dead_code)]
struct JsToolEntry {
    plugin_id: PluginId,
    description: String,
}

impl JsToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, id: PluginId, name: String, _description: String) {
        self.tools.write().await.insert(
            name.clone(),
            JsToolEntry {
                plugin_id: id,
                description: _description,
            },
        );
        tracing::info!("Plugin tool registered: {name}");
    }

    pub async fn unregister_plugin(&self, id: &PluginId) {
        let mut tools = self.tools.write().await;
        tools.retain(|_, entry| &entry.plugin_id != id);
    }

    pub async fn tool_count(&self) -> usize {
        self.tools.read().await.len()
    }
}

impl PluginRegistry {
    pub fn new(dispatcher: Arc<RcuDispatcher>) -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            dispatcher,
            _js_tools: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(
        &self,
        id: PluginId,
        _context: (),
    ) -> Result<(), jcode_plugin_core::PluginError> {
        let mut plugins = self.plugins.write().await;
        if plugins.contains_key(&id) {
            return Err(jcode_plugin_core::PluginError::Other(format!(
                "Plugin already registered: {id}"
            )));
        }
        plugins.insert(
            id.clone(),
            PluginRegistration {
                _id: id.clone(),
                _state: PluginState::Active,
                _tools: Vec::new(),
            },
        );
        tracing::info!("Plugin registered: {id}");
        Ok(())
    }

    pub async fn unregister(&self, id: &PluginId) {
        let mut plugins = self.plugins.write().await;
        plugins.remove(id);
        self.dispatcher.unregister_plugin(id);
        tracing::info!("Plugin unregistered: {id}");
    }

    pub async fn get_state(&self, id: &PluginId) -> Option<String> {
        let plugins = self.plugins.read().await;
        plugins.get(id).map(|p| {
            match p._state {
                PluginState::Active => "active",
                PluginState::Error(_) => "error",
                PluginState::Disabled => "disabled",
            }
            .to_string()
        })
    }

    pub fn register_handler(&self, event: PluginEvent, id: PluginId, slot: HandlerSlot) {
        self.dispatcher.register(event, id, slot);
    }

    /// TODO(WIP): Register a tool exposed by a JS plugin.
    /// Currently a no-op — the JS tool handle needs to be wrapped in a `Tool`
    /// implementation that bridges calls into the QuickJS context. This requires
    /// creating a `PluginTool` adapter that serializes input to JSON, invokes the
    /// JS function, and deserializes the output.
    pub fn register_js_tool(&self, _id: PluginId, _name: String, _handle: rquickjs::Object) {
        tracing::warn!("register_js_tool called but not yet implemented [STUB]");
    }

    pub async fn plugin_count(&self) -> usize {
        self.plugins.read().await.len()
    }

    pub async fn list(&self) -> Vec<(PluginId, String)> {
        let plugins = self.plugins.read().await;
        plugins
            .iter()
            .map(|(id, reg)| {
                let state = match &reg._state {
                    PluginState::Active => "active",
                    PluginState::Error(_) => "error",
                    PluginState::Disabled => "disabled",
                };
                (id.clone(), state.to_string())
            })
            .collect()
    }
}
