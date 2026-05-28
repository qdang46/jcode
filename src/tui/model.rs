//! FrankenTUI Model for jcode
//!
//! This module defines the central Model type that replaces the current App struct
//! in the Elm/Bubbletea architecture used by frankentui.
//!
//! The Model owns all state that affects rendering. Business logic state remains
//! in the App struct which is managed separately.

use ftui_runtime::{Cmd, Frame, Model};
use std::time::Instant;

// ===== Message Enum =====
// Msg variants mirror terminal events and app-level actions

#[derive(Debug, Clone)]
pub enum Msg {
    // Input events (from Event::Key)
    KeyChar(char),
    KeyEnter,
    KeyCtrlC,
    KeyCtrlD,
    KeyCtrlL,
    KeyCtrlU,
    KeyAltEnter,
    KeyShiftTab,
    KeyTab,
    KeyEsc,
    KeyUp,
    KeyDown,
    KeyLeft,
    KeyRight,
    KeyHome,
    KeyEnd,
    KeyPageUp,
    KeyPageDown,
    KeyBackspace,
    KeyDelete,
    // Mouse events
    MouseClick { row: u16, col: u16, button: u8 },
    MouseScrollUp,
    MouseScrollDown,
    // Clipboard/Paste
    Paste(String),
    // Application-level actions
    Submit,
    Cancel,
    ToggleSessionPicker,
    ToggleLoginPicker,
    ToggleSidePanel,
    ToggleDiffMode,
    TogglePlanMode,
    ScrollUp,
    ScrollDown,
    ScrollToTop,
    ScrollToBottom,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    // Window events
    Resize { width: u16, height: u16 },
    // Messages from agent
    AppendStreamingChunk(String),
    MessageEnd,
    StreamStart,
    StreamError(String),
    // Remote events
    RemoteSessionListUpdated(Vec<String>),
    RemoteModelSwitch(String),
    // Quit
    Quit,
    // Tick (for subscriptions)
    Tick,
}

// ===== Model =====

#[derive(Debug)]
pub struct Model {
    // --- Display state (what's visible on screen) ---
    pub messages: Vec<crate::tui::DisplayMessage>,
    pub messages_version: u64,
    pub input: String,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
    pub auto_scroll_paused: bool,

    // --- Processing state ---
    pub is_processing: bool,
    pub streaming_text: String,
    pub status: crate::tui::ProcessingStatus,

    // --- Provider info ---
    pub provider_name: Option<String>,
    pub provider_model: Option<String>,

    // --- Token/usage ---
    pub streaming_tokens: (u64, u64),
    pub total_session_tokens: Option<(u64, u64)>,
    pub total_cost: f32,

    // --- Output TPS ---
    pub output_tps: Option<f32>,

    // --- Layout/diff ---
    pub diff_mode: crate::config::DiffDisplayMode,
    pub centered: bool,
    pub diagram_mode: crate::config::DiagramDisplayMode,

    // --- Pickers/overlays ---
    pub session_picker_open: bool,
    pub login_picker_open: bool,
    // NOTE: Overlays are managed via TuiState trait (session_picker_overlay,
    // login_picker_overlay, etc.). This tracks which are visible at the view level.
    pub active_overlay: Option<ActiveOverlay>,

    // --- Viewport state ---
    pub viewport_width: u16,
    pub viewport_height: u16,

    // --- Remote mode ---
    pub is_remote: bool,
    pub remote_sessions: Vec<String>,
    pub remote_server_name: Option<String>,
    pub remote_server_icon: Option<String>,

    // --- Terminal info ---
    pub should_quit: bool,
}

/// Active overlay tracking (maps to the various picker/overlay types in jcode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveOverlay {
    Help,
    Changelog,
    SessionPicker,
    LoginPicker,
    AccountPicker,
    UsageOverlay,
}

impl Model {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            messages_version: 0,
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            auto_scroll_paused: false,
            is_processing: false,
            streaming_text: String::new(),
            status: crate::tui::ProcessingStatus::Idle,
            provider_name: None,
            provider_model: None,
            streaming_tokens: (0, 0),
            total_session_tokens: None,
            total_cost: 0.0,
            output_tps: None,
            diff_mode: crate::config::DiffDisplayMode::default(),
            centered: false,
            diagram_mode: crate::config::DiagramDisplayMode::default(),
            session_picker_open: false,
            login_picker_open: false,
            active_overlay: None,
            viewport_width: 0,
            viewport_height: 0,
            is_remote: false,
            remote_sessions: Vec::new(),
            remote_server_name: None,
            remote_server_icon: None,
            should_quit: false,
        }
    }
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    /// Sync model state from the App struct.
    /// Called before each render to pick up latest state.
    pub fn sync_from_app(&mut self, app: &crate::tui::app::App) {
        use crate::tui::TuiState;

        self.messages_version = app.display_messages_version();
        self.scroll_offset = app.scroll_offset();
        self.auto_scroll_paused = app.auto_scroll_paused();
        self.is_processing = app.is_processing();
        self.streaming_text = app.streaming_text().to_string();
        self.status = app.status();
        self.provider_name = Some(app.provider_name());
        self.provider_model = Some(app.provider_model());
        self.streaming_tokens = app.streaming_tokens();
        self.total_session_tokens = app.total_session_tokens();
        self.output_tps = app.output_tps();
        self.diff_mode = app.diff_mode();
        self.is_remote = app.is_remote_mode();
        self.remote_sessions = app.server_sessions();
        self.remote_server_name = app.server_display_name();
        self.remote_server_icon = app.server_display_icon();

        // Sync display messages (cloning the Vec - could be optimized later)
        self.messages = app.display_messages().to_vec();

        // Sync overlay state
        self.session_picker_open = app.session_picker_overlay().is_some();
        self.login_picker_open = app.login_picker_overlay().is_some();

        // Map active overlay from app state
        self.active_overlay = if self.session_picker_open {
            Some(ActiveOverlay::SessionPicker)
        } else if self.login_picker_open {
            Some(ActiveOverlay::LoginPicker)
        } else {
            None
        };
    }
}

impl Model {
    /// Update model state based on a message.
    /// Returns Cmd for side effects.
    pub fn update(&mut self, msg: Msg) -> Cmd<Msg> {
        match msg {
            Msg::KeyChar(c) => {
                self.input.push(c);
                self.cursor_pos = self.input.len();
                Cmd::none()
            }
            Msg::KeyBackspace => {
                if self.cursor_pos > 0 {
                    self.input.remove(self.cursor_pos - 1);
                    self.cursor_pos = self.cursor_pos.saturating_sub(1);
                }
                Cmd::none()
            }
            Msg::KeyEnter => {
                if !self.input.trim().is_empty() {
                    // Submit will be handled by bridging to app logic
                    let _input = self.input.clone();
                    self.input.clear();
                    self.cursor_pos = 0;
                    // Return a command that signals submission
                    // The actual submission happens through app bridge
                    return Cmd::none();
                }
                Cmd::none()
            }
            Msg::Submit => {
                // Called when input is submitted
                Cmd::none()
            }
            Msg::ScrollUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(5);
                self.auto_scroll_paused = true;
                Cmd::none()
            }
            Msg::ScrollDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
                if self.scroll_offset == 0 {
                    self.auto_scroll_paused = false;
                }
                Cmd::none()
            }
            Msg::ScrollToBottom => {
                self.scroll_offset = 0;
                self.auto_scroll_paused = false;
                Cmd::none()
            }
            Msg::ToggleSessionPicker => {
                self.session_picker_open = !self.session_picker_open;
                self.login_picker_open = false;
                self.active_overlay = if self.session_picker_open {
                    Some(ActiveOverlay::SessionPicker)
                } else {
                    None
                };
                Cmd::none()
            }
            Msg::ToggleLoginPicker => {
                self.login_picker_open = !self.login_picker_open;
                self.session_picker_open = false;
                self.active_overlay = if self.login_picker_open {
                    Some(ActiveOverlay::LoginPicker)
                } else {
                    None
                };
                Cmd::none()
            }
            Msg::TogglePlanMode => {
                // Toggle plan mode - handled by app
                Cmd::none()
            }
            Msg::AppendStreamingChunk(text) => {
                self.streaming_text.push_str(&text);
                if !self.auto_scroll_paused {
                    self.scroll_offset = 0;
                }
                Cmd::none()
            }
            Msg::MessageEnd => {
                self.streaming_text.clear();
                self.is_processing = false;
                Cmd::none()
            }
            Msg::StreamStart => {
                self.is_processing = true;
                self.streaming_text.clear();
                Cmd::none()
            }
            Msg::Resize { width, height } => {
                self.viewport_width = width;
                self.viewport_height = height;
                Cmd::none()
            }
            Msg::Quit => {
                self.should_quit = true;
                Cmd::quit()
            }
            _ => Cmd::none(),
        }
    }
}

impl ftui_runtime::Model for Model {
    type Message = Msg;

    fn init(&mut self) -> Cmd<Self::Message> {
        // Return startup commands if needed
        Cmd::none()
    }

    fn update(&mut self, msg: Self::Message) -> Cmd<Self::Message> {
        self.update(msg)
    }

    fn view(&self, _frame: &mut Frame) {
        // STUB: Actual rendering implemented in Phase 4 (bead jcode-4we)
        // For now, empty render - frankentui runtime boots but shows blank screen
    }

    fn subscriptions(&self) -> Vec<Box<dyn ftui_runtime::Subscription<Self::Message>>> {
        // STUB: subscriptions implemented in later beads
        vec![]
    }
}
