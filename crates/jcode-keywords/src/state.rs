//! Mode state — persistent activation state for keyword-triggered workflows.
//!
//! State is persisted to `.jcode/state/modes.toml` (project-local) or
//! `~/.jcode/state/modes.toml` (global fallback).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::detector::DetectedKeyword;
use crate::registry::WorkflowKind;

/// Default number of turns before a mode auto-deactivates.
const DEFAULT_TURN_LIMIT: u32 = 10;

/// Persistent mode state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModeState {
    /// Currently active modes.
    pub active_modes: Vec<ActiveMode>,
    /// ISO 8601 timestamp of last update.
    pub updated_at: Option<String>,
}

/// A single active mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveMode {
    /// The workflow kind.
    pub workflow: WorkflowKind,
    /// ISO 8601 timestamp when activated.
    pub activated_at: String,
    /// Number of turns since activation. Auto-deactivates at turn limit.
    pub turn_count: u32,
    /// Turn limit before auto-deactivation.
    pub turn_limit: u32,
}

impl ActiveMode {
    /// Check if this mode has expired.
    pub fn is_expired(&self) -> bool {
        self.turn_count >= self.turn_limit
    }
}

/// Update mode state based on detected keywords.
///
/// - Activates new modes from detections
/// - Increments turn count for existing modes
/// - Deactivates expired modes
/// - Cancel clears everything
/// - Persists state to disk
pub fn update_modes(detections: &[DetectedKeyword], working_dir: Option<&Path>) -> ModeState {
    let mut state = load_state(working_dir);

    // Cancel clears everything
    if detections
        .iter()
        .any(|d| d.entry.workflow == WorkflowKind::Cancel)
    {
        state.active_modes.clear();
        state.updated_at = Some(Utc::now().to_rfc3339());
        save_state(&state, working_dir);
        return state;
    }

    // Increment turn counts for existing modes
    for mode in &mut state.active_modes {
        mode.turn_count += 1;
    }

    // Remove expired modes
    state.active_modes.retain(|m| !m.is_expired());

    // Activate new modes from detections
    for detection in detections {
        let workflow = detection.entry.workflow;

        // Skip if already active
        if state.active_modes.iter().any(|m| m.workflow == workflow) {
            continue;
        }

        state.active_modes.push(ActiveMode {
            workflow,
            activated_at: Utc::now().to_rfc3339(),
            turn_count: 0,
            turn_limit: DEFAULT_TURN_LIMIT,
        });
    }

    state.updated_at = Some(Utc::now().to_rfc3339());
    save_state(&state, working_dir);
    state
}

/// Load mode state from disk.
pub fn load_state(working_dir: Option<&Path>) -> ModeState {
    let path = state_path(working_dir);
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    } else {
        ModeState::default()
    }
}

/// Save mode state to disk.
pub fn save_state(state: &ModeState, working_dir: Option<&Path>) {
    let path = state_path(working_dir);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(content) = toml::to_string_pretty(state) {
        let _ = std::fs::write(&path, content);
    }
}

/// Resolve the state file path.
fn state_path(working_dir: Option<&Path>) -> PathBuf {
    // Project-local takes priority
    if let Some(dir) = working_dir {
        return dir.join(".jcode").join("state").join("modes.toml");
    }

    // Global fallback
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".jcode")
        .join("state")
        .join("modes.toml")
}

/// Clear all active modes (used by cancel).
pub fn clear_modes(working_dir: Option<&Path>) {
    let state = ModeState::default();
    save_state(&state, working_dir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_state_missing_file_returns_default() {
        let tmp = TempDir::new().unwrap();
        // Use a subdir that definitely doesn't have .jcode/state/modes.toml
        let state = load_state(Some(tmp.path()));
        assert!(state.active_modes.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let state = ModeState {
            active_modes: vec![ActiveMode {
                workflow: WorkflowKind::Ultrawork,
                activated_at: "2026-01-01T00:00:00Z".to_string(),
                turn_count: 3,
                turn_limit: 10,
            }],
            updated_at: Some("2026-01-01T00:00:00Z".to_string()),
        };
        save_state(&state, Some(tmp.path()));
        let loaded = load_state(Some(tmp.path()));
        assert_eq!(loaded.active_modes.len(), 1);
        assert_eq!(loaded.active_modes[0].workflow, WorkflowKind::Ultrawork);
    }

    #[test]
    fn active_mode_expires() {
        let mode = ActiveMode {
            workflow: WorkflowKind::Ultrawork,
            activated_at: "2026-01-01T00:00:00Z".to_string(),
            turn_count: 10,
            turn_limit: 10,
        };
        assert!(mode.is_expired());
    }

    #[test]
    fn active_mode_not_expired() {
        let mode = ActiveMode {
            workflow: WorkflowKind::Ultrawork,
            activated_at: "2026-01-01T00:00:00Z".to_string(),
            turn_count: 5,
            turn_limit: 10,
        };
        assert!(!mode.is_expired());
    }
}
