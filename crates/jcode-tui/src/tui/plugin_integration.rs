//! Integration layer between the jcode TUI and the plugin runtime.
//!
//! This module bridges [`TuiPluginSystem`] into the synchronous TUI draw loop
//! and the async event loop. It provides:
//!
//! - Initialization of the plugin system at startup
//! - Synchronous slot reads for the draw path (status bar, sidebar)
//! - Async key-event forwarding before normal TUI key handling
//! - Async protocol-event forwarding from `ServerEvent::PluginNotification`

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyModifiers};
use jcode_plugin_runtime::dispatcher::RcuDispatcher;
use jcode_plugin_runtime::registry::PluginRegistry;
use jcode_plugin_runtime::tui_api::{SlotContent, SlotRegistry, SlotType};
use jcode_plugin_runtime::tui_system::TuiPluginSystem;

// ---------------------------------------------------------------------------
// PluginTuiBridge
// ---------------------------------------------------------------------------

/// Bridge between the TUI application and the plugin runtime.
///
/// Owns the [`TuiPluginSystem`] and provides both synchronous (draw-path) and
/// async (event-loop) access to plugin state.
pub struct PluginTuiBridge {
    /// The live plugin system (kept for key/event dispatch).
    system: Arc<TuiPluginSystem>,
    /// Shared slot registry for synchronous reads from the draw path.
    slot_registry: Arc<SlotRegistry>,
}

impl PluginTuiBridge {
    /// Initialize the plugin system: create a registry, discover and load all
    /// TUI-kind plugins, and return the bridge.
    pub async fn new() -> Self {
        let dispatcher = Arc::new(RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher));
        let mut system = TuiPluginSystem::new(Arc::clone(&registry));
        if let Err(e) = system.load_all().await {
            crate::logging::warn(&format!("Failed to load TUI plugins: {}", e));
        }
        let slot_registry = Arc::clone(system.slot_registry());
        let system = Arc::new(system);
        crate::logging::info(&format!(
            "TUI plugin system initialized ({} plugin(s))",
            system.plugin_count()
        ));
        Self {
            system,
            slot_registry,
        }
    }

    // -- Synchronous draw-path API ------------------------------------------

    /// Read slot content synchronously (non-blocking) from the shared registry.
    /// Returns an empty vec when the lock is contended or no plugins filled the
    /// slot.
    pub fn read_slots(&self, slot_type: SlotType) -> Vec<SlotContent> {
        let Ok(registry) = self.slot_registry.try_read() else {
            return Vec::new();
        };
        let prefix = format!(":{}", slot_type.as_str());
        registry
            .iter()
            .filter(|(k, _)| k.ends_with(&prefix))
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Whether any plugin has filled the given slot.
    pub fn has_slot_content(&self, slot_type: SlotType) -> bool {
        let Ok(registry) = self.slot_registry.try_read() else {
            return false;
        };
        let prefix = format!(":{}", slot_type.as_str());
        registry.keys().any(|k| k.ends_with(&prefix))
    }

    // -- Async event-loop API -----------------------------------------------

    /// Forward a key event to all loaded plugins. Returns `true` if any plugin
    /// consumed the key.
    pub async fn handle_key(&self, key: &str) -> bool {
        self.system.handle_key(key).await
    }

    /// Forward a protocol event (e.g. `PluginNotification`) to all loaded TUI
    /// plugins.
    pub async fn dispatch_event(&self, event: &str, data: &serde_json::Value) {
        let _ = self.system.dispatch_tui_event(event, data).await;
    }

    /// Number of loaded TUI plugins.
    pub fn plugin_count(&self) -> usize {
        self.system.plugin_count()
    }
}

// ---------------------------------------------------------------------------
// Key formatting
// ---------------------------------------------------------------------------

/// Format a crossterm key event into the canonical `"Mod+Key"` string expected
/// by the plugin keybinding system (e.g. `"Ctrl+K"`, `"Alt+Enter"`).
///
/// Returns `None` for key codes that have no standard string representation
/// (e.g. `KeyCode::Null`).
pub(crate) fn format_plugin_key(code: KeyCode, modifiers: KeyModifiers) -> Option<String> {
    let key_part = match code {
        KeyCode::Char(c) => {
            let upper = c.to_ascii_uppercase();
            return Some(if modifiers.contains(KeyModifiers::CONTROL) {
                format!("Ctrl+{}", upper)
            } else if modifiers.contains(KeyModifiers::ALT) {
                format!("Alt+{}", upper)
            } else {
                upper.to_string()
            });
        }
        KeyCode::Enter => "Enter",
        KeyCode::Esc => "Escape",
        KeyCode::Tab => "Tab",
        KeyCode::BackTab => "BackTab",
        KeyCode::Backspace => "Backspace",
        KeyCode::Delete => "Delete",
        KeyCode::Insert => "Insert",
        KeyCode::Home => "Home",
        KeyCode::End => "End",
        KeyCode::PageUp => "PageUp",
        KeyCode::PageDown => "PageDown",
        KeyCode::Up => "Up",
        KeyCode::Down => "Down",
        KeyCode::Left => "Left",
        KeyCode::Right => "Right",
        KeyCode::F(n) => return Some(format!("F{}", n)),
        _ => return None,
    };

    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }
    parts.push(key_part);
    Some(parts.join("+"))
}

// ---------------------------------------------------------------------------
// Draw-path helpers
// ---------------------------------------------------------------------------

/// Render plugin `StatusBar` slot content into the given area.
///
/// Each plugin's slot content is rendered as a single ratatui [`Line`]. If no
/// plugin has filled the `StatusBar` slot this is a no-op.
pub(crate) fn draw_status_bar_slots(
    frame: &mut ratatui::Frame,
    bridge: &PluginTuiBridge,
    area: ratatui::layout::Rect,
) {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};

    let slots = bridge.read_slots(SlotType::StatusBar);
    if slots.is_empty() {
        return;
    }

    let lines: Vec<Line<'static>> = slots
        .iter()
        .map(|s| {
            Line::from(Span::styled(
                s.render(),
                Style::default().fg(Color::DarkGray),
            ))
        })
        .collect();

    let paragraph = ratatui::widgets::Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Render plugin `Sidebar` slot content into the given area.
///
/// Content is wrapped in a bordered block titled "Plugins". If no plugin has
/// filled the `Sidebar` slot this is a no-op.
pub(crate) fn draw_sidebar_slots(
    frame: &mut ratatui::Frame,
    bridge: &PluginTuiBridge,
    area: ratatui::layout::Rect,
) {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph};

    let slots = bridge.read_slots(SlotType::Sidebar);
    if slots.is_empty() {
        return;
    }

    let lines: Vec<Line<'static>> = slots
        .iter()
        .map(|s| {
            Line::from(Span::styled(
                s.render(),
                Style::default().fg(Color::DarkGray),
            ))
        })
        .collect();

    let block = Block::default()
        .title("Plugins")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
