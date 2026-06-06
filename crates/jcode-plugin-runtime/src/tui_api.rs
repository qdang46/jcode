use crate::registry::PluginRegistry;
use jcode_plugin_core::types::PluginId;
use rquickjs::{Ctx, Function, Object, Value};
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// SlotType / SlotContent
// ---------------------------------------------------------------------------

/// Predefined UI slots a TUI plugin can fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotType {
    Sidebar,
    StatusBar,
    Overlay,
    Header,
    Footer,
}

impl SlotType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sidebar => "Sidebar",
            Self::StatusBar => "StatusBar",
            Self::Overlay => "Overlay",
            Self::Header => "Header",
            Self::Footer => "Footer",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "Sidebar" => Some(Self::Sidebar),
            "StatusBar" => Some(Self::StatusBar),
            "Overlay" => Some(Self::Overlay),
            "Header" => Some(Self::Header),
            "Footer" => Some(Self::Footer),
            _ => None,
        }
    }
}

/// Content that can be rendered in a UI slot.
#[derive(Debug, Clone)]
pub enum SlotContent {
    Text { body: String },
    Box { title: String, body: String },
    List { items: Vec<String> },
    Empty,
}

impl SlotContent {
    /// Render to a plain string suitable for TUI display.
    pub fn render(&self) -> String {
        match self {
            Self::Text { body } => body.clone(),
            Self::Box { title, body } => format!("[{title}]\n{body}"),
            Self::List { items } => items
                .iter()
                .enumerate()
                .map(|(i, item)| format!("{}. {}", i + 1, item))
                .collect::<Vec<_>>()
                .join("\n"),
            Self::Empty => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// TuiPluginApi
// ---------------------------------------------------------------------------

/// Shared slot registry: maps `"{plugin_id}:{SlotType}"` to content.
/// Used by `TuiPluginSystem` to aggregate slots across all plugins.
pub type SlotRegistry = tokio::sync::RwLock<HashMap<String, SlotContent>>;

/// Provides the TUI-specific API surface exposed to JS plugins as
/// `globalThis.__jcode_tui_pi`.
///
/// Sub-APIs: route, keymap, ui, slot, theme, kv, eventBus.
pub struct TuiPluginApi {
    plugin_id: PluginId,
    registry: Arc<PluginRegistry>,
    /// In-memory slot store (slot_key -> content).
    slots: Arc<tokio::sync::RwLock<HashMap<String, SlotContent>>>,
    /// System-level slot registry shared across all TUI plugins.
    /// When set, slot fill/clear operations propagate here.
    system_slots: Option<Arc<SlotRegistry>>,
    /// In-memory KV store scoped to this plugin.
    kv_store: Arc<std::sync::RwLock<HashMap<String, String>>>,
}

impl TuiPluginApi {
    pub fn new(plugin_id: PluginId, registry: Arc<PluginRegistry>) -> Self {
        Self {
            plugin_id,
            registry,
            slots: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            system_slots: None,
            kv_store: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Create a new API instance with a shared system-level slot registry.
    pub fn with_system_slots(
        plugin_id: PluginId,
        registry: Arc<PluginRegistry>,
        system_slots: Arc<SlotRegistry>,
    ) -> Self {
        Self {
            plugin_id,
            registry,
            slots: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            system_slots: Some(system_slots),
            kv_store: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Install the `__jcode_tui_pi` global on the QuickJS context.
    pub fn install<'js>(&self, ctx: &Ctx<'js>) -> Result<(), rquickjs::Error> {
        let tui = Object::new(ctx.clone())?;

        tui.set("route", self.make_route_api(ctx)?)?;
        tui.set("keymap", self.make_keymap_api(ctx)?)?;
        tui.set("ui", self.make_ui_api(ctx)?)?;
        tui.set("slot", self.make_slot_api(ctx)?)?;
        tui.set("theme", self.make_theme_api(ctx)?)?;
        tui.set("kv", self.make_kv_api(ctx)?)?;
        tui.set("eventBus", self.make_event_bus_api(ctx)?)?;

        ctx.globals().set("__jcode_tui_pi", tui)?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // RouteApi  -  register custom views
    // ------------------------------------------------------------------

    fn make_route_api<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let api = Object::new(ctx.clone())?;
        let registry = Arc::clone(&self.registry);
        let plugin_id = self.plugin_id.clone();

        // register(path, handler)
        let reg_id = plugin_id.clone();
        let reg_registry = Arc::clone(&registry);
        api.set(
            "register",
            Function::new(ctx.clone(), move |path: String, handler: Value<'js>| {
                tracing::info!(
                    "Plugin {} registered route: {} (handler type: {:?})",
                    reg_id,
                    path,
                    handler.type_of()
                );
                let _ = &reg_registry; // keep alive; actual dispatch deferred
            }),
        )?;

        // unregister(path)
        let unreg_id = plugin_id.clone();
        let unreg_registry = Arc::clone(&registry);
        api.set(
            "unregister",
            Function::new(ctx.clone(), move |path: String| {
                tracing::info!("Plugin {} unregistered route: {}", unreg_id, path);
                let _ = &unreg_registry;
            }),
        )?;

        // navigate(path)
        let nav_id = plugin_id.clone();
        api.set(
            "navigate",
            Function::new(ctx.clone(), move |path: String| {
                tracing::info!("Plugin {} requests navigate: {}", nav_id, path);
            }),
        )?;

        Ok(api)
    }

    // ------------------------------------------------------------------
    // KeymapApi  -  register keybindings
    // ------------------------------------------------------------------

    fn make_keymap_api<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let api = Object::new(ctx.clone())?;
        let plugin_id = self.plugin_id.clone();

        // register(key, description, handler)
        let reg_id = plugin_id.clone();
        api.set(
            "register",
            Function::new(
                ctx.clone(),
                move |key: String, description: String, handler: Value<'js>| {
                    tracing::info!(
                        "Plugin {} registered keybinding: {} - \"{}\" (handler type: {:?})",
                        reg_id,
                        key,
                        description,
                        handler.type_of()
                    );
                },
            ),
        )?;

        // unregister(key)
        let unreg_id = plugin_id.clone();
        api.set(
            "unregister",
            Function::new(ctx.clone(), move |key: String| {
                tracing::info!("Plugin {} unregistered keybinding: {}", unreg_id, key);
            }),
        )?;

        // list() -> array of { key, description }
        api.set(
            "list",
            Function::new(ctx.clone(), || -> Vec<String> {
                // Stub: real implementation would enumerate registered keybindings.
                Vec::new()
            }),
        )?;

        Ok(api)
    }

    // ------------------------------------------------------------------
    // UiApi  -  render text / box / list
    // ------------------------------------------------------------------

    fn make_ui_api<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let api = Object::new(ctx.clone())?;
        let plugin_id = self.plugin_id.clone();

        // text(body)
        let text_id = plugin_id.clone();
        api.set(
            "text",
            Function::new(ctx.clone(), move |body: String| -> String {
                tracing::debug!("Plugin {} renders text", text_id);
                body
            }),
        )?;

        // box(title, body)
        let box_id = plugin_id.clone();
        api.set(
            "box",
            Function::new(ctx.clone(), move |title: String, body: String| -> String {
                tracing::debug!("Plugin {} renders box: {}", box_id, title);
                let content = SlotContent::Box { title, body };
                content.render()
            }),
        )?;

        // list(items)
        let list_id = plugin_id.clone();
        api.set(
            "list",
            Function::new(ctx.clone(), move |items: Vec<String>| -> String {
                tracing::debug!("Plugin {} renders list ({} items)", list_id, items.len());
                let content = SlotContent::List { items };
                content.render()
            }),
        )?;

        Ok(api)
    }

    // ------------------------------------------------------------------
    // SlotApi  -  fill predefined slots
    // ------------------------------------------------------------------

    fn make_slot_api<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let api = Object::new(ctx.clone())?;
        let plugin_id = self.plugin_id.clone();
        let slots = Arc::clone(&self.slots);

        // fill(slotName, contentObj)
        let fill_id = plugin_id.clone();
        let fill_slots = Arc::clone(&slots);
        let fill_system = self.system_slots.clone();
        api.set(
            "fill",
            Function::new(
                ctx.clone(),
                move |slot_name: String, content: Object<'js>| {
                    let slot_type = match SlotType::from_name(&slot_name) {
                        Some(s) => s,
                        None => {
                            tracing::warn!(
                                "Plugin {} tried to fill unknown slot: {}",
                                fill_id,
                                slot_name
                            );
                            return;
                        }
                    };

                    let kind: String = content.get("kind").unwrap_or_default();
                    let slot_content = match kind.as_str() {
                        "text" => {
                            let body: String = content.get("body").unwrap_or_default();
                            SlotContent::Text { body }
                        }
                        "box" => {
                            let title: String = content.get("title").unwrap_or_default();
                            let body: String = content.get("body").unwrap_or_default();
                            SlotContent::Box { title, body }
                        }
                        "list" => {
                            let items: Vec<String> = content.get("items").unwrap_or_default();
                            SlotContent::List { items }
                        }
                        _ => SlotContent::Empty,
                    };

                    tracing::info!(
                        "Plugin {} filled slot {} with {} content",
                        fill_id,
                        slot_type.as_str(),
                        kind
                    );

                    // Store the content (blocking on the RwLock in a sync closure is fine
                    // because this runs on the QuickJS thread).
                    let key = format!("{}:{}", fill_id, slot_type.as_str());
                    let fill_slots2 = Arc::clone(&fill_slots);
                    // We cannot .await inside a sync Function, so spawn a brief task.
                    let content_for_plugin = slot_content.clone();
                    tokio::spawn(async move {
                        fill_slots2.write().await.insert(key, content_for_plugin);
                    });

                    // Propagate to system-level slot registry if present.
                    if let Some(ref sys) = fill_system {
                        let sys_key = format!("{}:{}", fill_id, slot_type.as_str());
                        let sys_slots = Arc::clone(sys);
                        tokio::spawn(async move {
                            sys_slots.write().await.insert(sys_key, slot_content);
                        });
                    }
                },
            ),
        )?;

        // clear(slotName)
        let clear_id = plugin_id.clone();
        let clear_slots = Arc::clone(&slots);
        let clear_system = self.system_slots.clone();
        api.set(
            "clear",
            Function::new(ctx.clone(), move |slot_name: String| {
                tracing::info!("Plugin {} cleared slot {}", clear_id, slot_name);
                let key = format!("{}:{}", clear_id, slot_name);
                let clear_slots2 = Arc::clone(&clear_slots);
                tokio::spawn(async move {
                    clear_slots2.write().await.remove(&key);
                });

                // Propagate removal to system-level slot registry.
                if let Some(ref sys) = clear_system {
                    let sys_key = format!("{}:{}", clear_id, slot_name);
                    let sys_slots = Arc::clone(sys);
                    tokio::spawn(async move {
                        sys_slots.write().await.remove(&sys_key);
                    });
                }
            }),
        )?;

        // list() -> array of slot names
        api.set(
            "list",
            Function::new(ctx.clone(), || -> Vec<String> {
                SlotType::iter().map(|s| s.as_str().to_string()).collect()
            }),
        )?;

        Ok(api)
    }

    // ------------------------------------------------------------------
    // ThemeApi  -  read / write colors
    // ------------------------------------------------------------------

    fn make_theme_api<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let api = Object::new(ctx.clone())?;
        let plugin_id = self.plugin_id.clone();

        // getColor(name) -> string (hex)
        let get_id = plugin_id.clone();
        api.set(
            "getColor",
            Function::new(ctx.clone(), move |name: String| -> String {
                tracing::debug!("Plugin {} reads theme color: {}", get_id, name);
                // Stub: return sensible defaults.
                match name.as_str() {
                    "bg" => "#1e1e2e".to_string(),
                    "fg" => "#cdd6f4".to_string(),
                    "accent" => "#89b4fa".to_string(),
                    "error" => "#f38ba8".to_string(),
                    "warning" => "#fab387".to_string(),
                    "success" => "#a6e3a1".to_string(),
                    _ => "#cdd6f4".to_string(),
                }
            }),
        )?;

        // setColor(name, hex)
        let set_id = plugin_id.clone();
        api.set(
            "setColor",
            Function::new(ctx.clone(), move |name: String, hex: String| {
                tracing::info!("Plugin {} sets theme color: {} = {}", set_id, name, hex);
            }),
        )?;

        // getAll() -> object of name->hex
        api.set(
            "getAll",
            Function::new(ctx.clone(), || -> HashMap<String, String> {
                // Stub: return default palette.
                let mut map = HashMap::new();
                map.insert("bg".to_string(), "#1e1e2e".to_string());
                map.insert("fg".to_string(), "#cdd6f4".to_string());
                map.insert("accent".to_string(), "#89b4fa".to_string());
                map.insert("error".to_string(), "#f38ba8".to_string());
                map.insert("warning".to_string(), "#fab387".to_string());
                map.insert("success".to_string(), "#a6e3a1".to_string());
                map
            }),
        )?;

        Ok(api)
    }

    // ------------------------------------------------------------------
    // KvApi  -  persistent key-value storage scoped to plugin
    // ------------------------------------------------------------------

    fn make_kv_api<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let api = Object::new(ctx.clone())?;
        let plugin_id = self.plugin_id.clone();
        let kv_store = Arc::clone(&self.kv_store);

        // get(key) -> string
        let get_id = plugin_id.clone();
        let get_store = Arc::clone(&kv_store);
        api.set(
            "get",
            Function::new(ctx.clone(), move |key: String| -> String {
                tracing::debug!("Plugin {} kv.get({})", get_id, key);
                let key = format!("{}:{}", get_id, key);
                let store = Arc::clone(&get_store);
                store.read().unwrap().get(&key).cloned().unwrap_or_default()
            }),
        )?;

        // set(key, value)
        let set_id = plugin_id.clone();
        let set_store = Arc::clone(&kv_store);
        api.set(
            "set",
            Function::new(ctx.clone(), move |key: String, value: String| {
                tracing::debug!("Plugin {} kv.set({}, ...)", set_id, key);
                let key = format!("{}:{}", set_id, key);
                let store = Arc::clone(&set_store);
                store.write().unwrap().insert(key, value);
            }),
        )?;

        // delete(key)
        let del_id = plugin_id.clone();
        let del_store = Arc::clone(&kv_store);
        api.set(
            "delete",
            Function::new(ctx.clone(), move |key: String| {
                tracing::debug!("Plugin {} kv.delete({})", del_id, key);
                let key = format!("{}:{}", del_id, key);
                let store = Arc::clone(&del_store);
                store.write().unwrap().remove(&key);
            }),
        )?;

        // list() -> array of keys (without plugin prefix)
        let list_id = plugin_id.clone();
        let list_store = Arc::clone(&kv_store);
        api.set(
            "list",
            Function::new(ctx.clone(), move || -> Vec<String> {
                let prefix = format!("{}:", list_id);
                let store = Arc::clone(&list_store);
                store
                    .read()
                    .unwrap()
                    .keys()
                    .filter(|k| k.starts_with(&prefix))
                    .map(|k| k[prefix.len()..].to_string())
                    .collect()
            }),
        )?;

        Ok(api)
    }

    // ------------------------------------------------------------------
    // EventBusApi  -  inter-plugin events
    // ------------------------------------------------------------------

    fn make_event_bus_api<'js>(&self, ctx: &Ctx<'js>) -> Result<Object<'js>, rquickjs::Error> {
        let api = Object::new(ctx.clone())?;
        let plugin_id = self.plugin_id.clone();
        let registry = Arc::clone(&self.registry);

        // emit(event, data)
        let emit_id = plugin_id.clone();
        let emit_registry = Arc::clone(&registry);
        api.set(
            "emit",
            Function::new(ctx.clone(), move |event: String, data: Value<'js>| {
                tracing::info!(
                    "Plugin {} emits event: {} (data type: {:?})",
                    emit_id,
                    event,
                    data.type_of()
                );
                let _ = &emit_registry;
            }),
        )?;

        // on(event, handler)
        let on_id = plugin_id.clone();
        let on_registry = Arc::clone(&registry);
        api.set(
            "on",
            Function::new(ctx.clone(), move |event: String, handler: Value<'js>| {
                tracing::info!(
                    "Plugin {} subscribes to event: {} (handler type: {:?})",
                    on_id,
                    event,
                    handler.type_of()
                );
                let _ = &on_registry;
            }),
        )?;

        // off(event)
        let off_id = plugin_id.clone();
        api.set(
            "off",
            Function::new(ctx.clone(), move |event: String| {
                tracing::info!("Plugin {} unsubscribes from event: {}", off_id, event);
            }),
        )?;

        Ok(api)
    }
}

// ---------------------------------------------------------------------------
// Helpers on SlotType
// ---------------------------------------------------------------------------

impl SlotType {
    /// Iterate over all variants.
    pub fn iter() -> impl Iterator<Item = SlotType> {
        [
            SlotType::Sidebar,
            SlotType::StatusBar,
            SlotType::Overlay,
            SlotType::Header,
            SlotType::Footer,
        ]
        .into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_type_roundtrip() {
        for slot in SlotType::iter() {
            let s = slot.as_str();
            assert_eq!(SlotType::from_name(s), Some(slot));
        }
    }

    #[test]
    fn slot_type_unknown() {
        assert_eq!(SlotType::from_name("Nonexistent"), None);
    }

    #[test]
    fn slot_content_render_text() {
        let c = SlotContent::Text {
            body: "hello".into(),
        };
        assert_eq!(c.render(), "hello");
    }

    #[test]
    fn slot_content_render_box() {
        let c = SlotContent::Box {
            title: "T".into(),
            body: "B".into(),
        };
        assert_eq!(c.render(), "[T]\nB");
    }

    #[test]
    fn slot_content_render_list() {
        let c = SlotContent::List {
            items: vec!["a".into(), "b".into(), "c".into()],
        };
        assert_eq!(c.render(), "1. a\n2. b\n3. c");
    }

    #[test]
    fn slot_content_render_empty() {
        let c = SlotContent::Empty;
        assert_eq!(c.render(), "");
    }
}
