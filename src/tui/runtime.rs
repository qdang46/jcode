//! FrankenTUI Runtime Driver for jcode
//!
//! This module provides the bridge between jcode's App and the frankentui runtime.
//!
//! ## Architecture Decision: Runtime-on-App Approach
//!
//! We use **Option A — Runtime-on-App approach** where:
//! 1. `App::run_frankentui()` creates an `AppWrapper` that holds the `App`
//! 2. The `AppWrapper` implements `ftui_runtime::Model` trait
//! 3. frankentui's `AppBuilder::run()` takes ownership and runs to completion
//! 4. `RunResult` is captured via a shared `Arc<Mutex<Option<RunResult>>>` that
//!    survives the move into the runtime
//!
//! This approach preserves all existing `RunResult` fields while using frankentui's
//! synchronous runtime loop.
//!
//! ## Why not Option B (Wrapper struct owning the runtime)?
//!
//! Option B would require creating a new `FrankenTuiRuntime` struct that owns the Program.
//! However, this would require significant refactoring of how the async/sync boundary
//! works. Option A keeps the App unchanged and just wraps it for the frankentui runtime.

use ftui::{App, Cmd};
use ftui_render::frame::Frame;
use ftui_runtime::MouseCapturePolicy;
use std::sync::{Arc, Mutex};

use crate::tui::app::App as AppCore;
use crate::tui::{ProcessingStatus, RunResult, TuiState};

// ===== AppWrapper =====

/// Wrapper that adapts jcode's `App` to frankentui's `Model` trait.
///
/// This struct holds:
/// - `app`: The jcode App, wrapped in Arc<Mutex<>> for shared access
/// - `result_ref`: Reference to the shared result capture location
/// - `display`: Display-only state synced from App before each view()
///
/// The `result_ref` is an Arc<Mutex<Option<RunResult>>> that SURVIVES the move
/// into AppBuilder::run() because we keep a clone of the Arc in the caller.
pub struct AppWrapper {
    /// Shared ownership of the App for sync access during view/shutdown
    app: Arc<Mutex<AppCore>>,
    /// Reference to shared result capture - survives the move into run()
    result_ref: Arc<Mutex<Option<RunResult>>>,
    /// Display model state (synced from App before view)
    display: DisplayState,
}

/// Display-only state that gets synced from App before each view() call.
/// This is a subset of what the real view will need.
#[derive(Debug, Default)]
struct DisplayState {
    messages_version: u64,
    scroll_offset: usize,
    auto_scroll_paused: bool,
    is_processing: bool,
    streaming_text: String,
    status: ProcessingStatus,
    provider_name: Option<String>,
    provider_model: Option<String>,
}

impl AppWrapper {
    /// Create a new AppWrapper wrapping the given App.
    ///
    /// `result` is a shared location where RunResult will be stored during shutdown.
    /// This Arc survives the move into AppBuilder::run() because we keep a clone.
    pub fn new(app: AppCore, result: Arc<Mutex<Option<RunResult>>>) -> Self {
        Self {
            app: Arc::new(Mutex::new(app)),
            result_ref: result,
            display: DisplayState::default(),
        }
    }

    /// Sync display state from the App into our display struct.
    /// Called before each view() render.
    fn sync_from_app(&mut self) {
        if let Ok(app) = self.app.lock() {
            self.display.messages_version = app.display_messages_version();
            self.display.scroll_offset = app.scroll_offset();
            self.display.auto_scroll_paused = app.auto_scroll_paused();
            self.display.is_processing = app.is_processing();
            self.display.streaming_text = app.streaming_text().to_string();
            self.display.status = app.status().clone();
            self.display.provider_name = Some(app.provider_name().to_string());
            self.display.provider_model = Some(app.provider_model());
        }
    }

    /// Extract RunResult from App and store it in the shared result location.
    /// Called during on_shutdown.
    fn capture_result(&self) {
        if let Ok(mut app) = self.app.lock() {
            let result = RunResult {
                reload_session: app.reload_requested().take(),
                rebuild_session: app.rebuild_requested().take(),
                update_session: app.update_requested().take(),
                restart_session: app.restart_requested().take(),
                exit_code: app.requested_exit_code().take(),
                session_id: Some(app.session_id().to_string()),
            };
            if let Ok(mut r) = self.result_ref.lock() {
                *r = Some(result);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppMsg;

impl From<ftui::Event> for AppMsg {
    fn from(_: ftui::Event) -> Self {
        AppMsg
    }
}

impl ftui::Model for AppWrapper {
    type Message = AppMsg;

    fn init(&mut self) -> Cmd<Self::Message> {
        // STUB: No startup commands needed for phase 1.3
        Cmd::none()
    }

    fn update(&mut self, _msg: Self::Message) -> Cmd<Self::Message> {
        // STUB: The real update logic lives in App::run() which runs the
        // synchronous event loop that frankentui drives. For phase 1.3, we
        // don't yet bridge the update cycle - view() is stubbed anyway.
        // Full update bridging comes in later beads.
        Cmd::none()
    }

    fn view(&self, _frame: &mut Frame) {
        // STUB: Actual rendering implemented in Phase 4 (bead jcode-4we)
        // For now, empty render - frankentui runtime boots but shows blank screen.
        // The display state is synced via sync_from_app() before each view call,
        // ready for when the real view() implementation lands.
    }

    fn subscriptions(&self) -> Vec<Box<dyn ftui_runtime::Subscription<Self::Message>>> {
        // STUB: Subscriptions (ticks, async events) come in later beads
        vec![]
    }

    fn on_shutdown(&mut self) -> Cmd<Self::Message> {
        // Capture the RunResult from App state before shutdown
        self.capture_result();
        Cmd::none()
    }
}

// ===== FrankenTUI Runtime =====

/// Configuration for the frankenTUI runtime.
#[derive(Debug, Clone)]
pub struct FrankenTuiConfig {
    /// Whether to capture mouse events
    pub mouse_capture: bool,
    /// Whether keyboard enhancement is enabled
    pub keyboard_enhanced: bool,
    /// Whether focus change events are enabled
    pub focus_change: bool,
    /// Whether to run in fullscreen mode
    pub fullscreen: bool,
}

impl Default for FrankenTuiConfig {
    fn default() -> Self {
        Self {
            mouse_capture: true,
            keyboard_enhanced: true,
            focus_change: true,
            fullscreen: true,
        }
    }
}

/// Run the TUI using the frankentui runtime, returning RunResult.
///
/// This function:
/// 1. Creates an Arc<Mutex<Option<RunResult>>> to capture the result
/// 2. Creates an AppWrapper that wraps the App and references the result Arc
/// 3. Configures the frankentui AppBuilder with appropriate settings
/// 4. Runs the frankentui runtime (blocking)
/// 5. Returns the captured RunResult from the shared Arc
///
/// Note: frankentui's run() is synchronous and takes ownership of the model.
/// The Arc<Mutex<Option<RunResult>>> SURVIVES the move because we hold a clone
/// in this function and can access it after run() returns.
pub fn run_frankentui(app: AppCore, config: FrankenTuiConfig) -> std::io::Result<RunResult> {
    // Shared location for capturing RunResult - survives the move into run()
    let result: Arc<Mutex<Option<RunResult>>> = Arc::new(Mutex::new(None));

    // Create the wrapper with reference to the shared result
    let wrapper = AppWrapper::new(app, Arc::clone(&result));

    // Build the AppBuilder with configuration
    let builder = if config.fullscreen {
        App::fullscreen(wrapper)
    } else {
        App::new(wrapper)
    };

    let builder = builder.with_mouse_capture_policy(if config.mouse_capture {
        MouseCapturePolicy::On
    } else {
        MouseCapturePolicy::Off
    });

    // Run the frankentui runtime (blocking)
    let run_result = builder.run();

    // Extract the captured RunResult from the shared Arc
    let captured = result
        .lock()
        .ok()
        .and_then(|mut r| r.take())
        .unwrap_or_else(|| RunResult {
            reload_session: None,
            rebuild_session: None,
            update_session: None,
            restart_session: None,
            exit_code: None,
            session_id: None,
        });

    match run_result {
        Ok(()) => Ok(captured),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_franken_tui_config_default() {
        let config = FrankenTuiConfig::default();
        assert!(config.mouse_capture);
        assert!(config.keyboard_enhanced);
        assert!(config.focus_change);
        assert!(config.fullscreen);
    }
}
