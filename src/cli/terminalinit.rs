//! FrankenTUI-compatible TUI initialization
//!
//! This module provides the bridge between jcode's terminal initialization
//! and the frankentui runtime.
//!
//! ## How it works with frankentui
//!
//! Frankentui's `AppBuilder::run()` manages terminal setup internally via the
//! CrosstermEventSource, which handles:
//! - Entering alternate screen mode
//! - Enabling mouse capture
//! - Enabling focus change events
//! - Kitty keyboard enhancement
//!
//! However, we still need to track the state for cleanup and maintain
//! compatibility with jcode's cleanup_tui_runtime pattern.
//!
//! For phase 1.3, the initialization is simplified because frankentui handles
//! most terminal setup internally.

use crate::tui;
use anyhow::Result;

/// TUI Runtime State tracking
///
/// This tracks what terminal modes were enabled so we can properly
/// restore them on cleanup. For frankentui, most of this is handled
/// internally, but we track it for compatibility.
#[derive(Debug, Clone)]
pub struct TuiRuntimeState {
    /// Whether mouse capture was enabled
    pub mouse_capture: bool,
    /// Whether keyboard enhancement was enabled
    pub keyboard_enhanced: bool,
    /// Whether focus change events were enabled
    pub focus_change: bool,
}

/// Initialize the TUI runtime for use with frankentui.
///
/// For frankentui, most terminal initialization happens inside `AppBuilder::run()`.
/// This function does minimal setup and returns the state needed for cleanup.
///
/// The actual terminal setup (alternate screen, mouse capture, etc.) is handled
/// by frankentui's internal CrosstermEventSource when `run()` is called.
pub fn init_tui_runtime() -> Result<TuiRuntimeState> {
    // Check that we're in a terminal
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        anyhow::bail!("jcode TUI requires an interactive terminal (stdin/stdout must be a TTY)");
    }

    // Frankentui handles most terminal setup internally via CrosstermEventSource.
    // We still track the perf policy settings for potential cleanup.
    let perf_policy = crate::perf::tui_policy();

    let mouse_capture = perf_policy.enable_mouse_capture;
    let focus_change = perf_policy.enable_focus_change;
    let keyboard_enhanced = if perf_policy.enable_keyboard_enhancement {
        tui::enable_keyboard_enhancement()
    } else {
        false
    };

    // Enable bracketed paste (used by frankentui)
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)?;

    if focus_change {
        crossterm::execute!(std::io::stdout(), crossterm::event::EnableFocusChange)?;
    }
    if mouse_capture {
        crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;
    }

    Ok(TuiRuntimeState {
        mouse_capture,
        keyboard_enhanced,
        focus_change,
    })
}

/// Clean up the TUI runtime, restoring the terminal to its previous state.
///
/// This is called after frankentui's `run()` completes or on error.
/// Frankentui's CrosstermEventSource handles most cleanup internally, but
/// we may need to do some additional restoration.
pub fn cleanup_tui_runtime(state: &TuiRuntimeState, restore_terminal: bool) {
    if restore_terminal {
        // Frankentui's CrosstermEventSource handles most terminal cleanup internally.
        // But we still need to do some cleanup that frankentui might not cover.
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);

        if state.focus_change {
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableFocusChange);
        }
        if state.mouse_capture {
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
        }
        if state.keyboard_enhanced {
            tui::disable_keyboard_enhancement();
        }

        // Some terminals may need additional defensive resets
        let _ = std::io::stdout().write_all(defensive_terminal_reset_bytes());
        let _ = std::io::stdout().flush();
    }
}

/// Same as cleanup_tui_runtime but also handles the run result for exit code logic.
pub fn cleanup_tui_runtime_for_run_result(
    state: &TuiRuntimeState,
    run_result: &crate::tui::RunResult,
    restore_terminal: bool,
) {
    cleanup_tui_runtime(state, restore_terminal);
}

/// Defensive terminal reset bytes for issue #158.
///
/// These bytes cover terminal state that frankentui might not reset on exit.
fn defensive_terminal_reset_bytes() -> &'static [u8] {
    b"\x1b[r\x1b[?25h\x1b[?2004l"
}
