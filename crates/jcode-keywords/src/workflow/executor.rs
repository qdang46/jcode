//! Workflow execution engine.
//!
//! Bridges the keyword system with the agent runtime. Called from the turn loop
//! to execute active workflows and produce actions (spawn agents, inject reminders, etc.).

use super::{WorkflowAction, WorkflowContext};
use crate::registry::WorkflowKind;
use crate::state::ModeState;

/// Truncate a string to `max_chars` characters, respecting UTF-8 boundaries.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        return s;
    }
    // Find the last valid char boundary at or before max_chars
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Execute all active workflows for the current turn.
///
/// Returns actions paired with the index of the active mode that produced them.
/// The caller is responsible for persisting metadata from `ContinueWithMetadata`.
pub fn execute_active_workflows(
    mode_state: &ModeState,
    user_input: &str,
    working_dir: Option<&std::path::Path>,
    session_id: &str,
) -> Vec<(usize, WorkflowKind, WorkflowAction)> {
    let mut actions = Vec::new();

    for (i, active_mode) in mode_state.active_modes.iter().enumerate() {
        // Skip cancel — it's handled by state::update_modes()
        if active_mode.workflow == WorkflowKind::Cancel {
            continue;
        }

        let Some(handler) = crate::workflow::get_handler(active_mode.workflow) else {
            continue;
        };

        let ctx = WorkflowContext {
            user_input,
            working_dir: working_dir.map(|p| p),
            session_id,
            mode_state,
            metadata: &active_mode.metadata,
        };

        let action = handler.execute(&ctx);
        actions.push((i, active_mode.workflow, action));
    }

    actions
}

/// Process the LLM's response through all active workflow handlers.
///
/// Returns actions paired with the index of the active mode that produced them.
pub fn process_turn_response(
    mode_state: &ModeState,
    response: &str,
) -> Vec<(usize, WorkflowKind, WorkflowAction)> {
    let mut actions = Vec::new();

    for (i, active_mode) in mode_state.active_modes.iter().enumerate() {
        if active_mode.workflow == WorkflowKind::Cancel {
            continue;
        }

        let Some(handler) = crate::workflow::get_handler(active_mode.workflow) else {
            continue;
        };

        let action = handler.on_turn_complete(response, &active_mode.metadata);
        actions.push((i, active_mode.workflow, action));
    }

    actions
}

/// Apply workflow actions to mode state (metadata persistence, mode deactivation).
///
/// This is the key function that persists `ContinueWithMetadata` and `Complete` actions.
/// Returns a summary of what changed.
pub fn apply_actions(
    mode_state: &mut ModeState,
    actions: &[(usize, WorkflowKind, WorkflowAction)],
) -> Vec<String> {
    let mut summaries = Vec::new();
    let mut to_remove = Vec::new();

    for (idx, kind, action) in actions {
        match action {
            WorkflowAction::ContinueWithMetadata { metadata, reminder } => {
                if let Some(mode) = mode_state.active_modes.get_mut(*idx) {
                    // Merge new metadata into existing (don't discard)
                    for (k, v) in metadata {
                        mode.metadata.insert(k.clone(), v.clone());
                    }
                    summaries.push(format!("{}: updated metadata, reminder: {}", kind, truncate_str(reminder, 50)));
                }
            }
            WorkflowAction::Complete(msg) => {
                to_remove.push(*idx);
                summaries.push(format!("{}: completed — {}", kind, msg));
            }
            WorkflowAction::Error(msg) => {
                to_remove.push(*idx);
                summaries.push(format!("{}: error — {}", kind, msg));
            }
            WorkflowAction::InjectReminder(r) => {
                summaries.push(format!("{}: inject reminder — {}", kind, truncate_str(r, 50)));
            }
            WorkflowAction::SpawnAgent { description, .. } => {
                if let Some(mode) = mode_state.active_modes.get_mut(*idx) {
                    mode.metadata.insert(format!("{}_spawned", kind), "true".to_string());
                }
                summaries.push(format!("{}: spawn agent — {}", kind, description));
            }
            WorkflowAction::SpawnParallel(specs) => {
                if let Some(mode) = mode_state.active_modes.get_mut(*idx) {
                    mode.metadata.insert(format!("{}_spawned", kind), "true".to_string());
                }
                summaries.push(format!("{}: spawn {} agents", kind, specs.len()));
            }
            WorkflowAction::AskUser(q) => {
                summaries.push(format!("{}: ask user — {}", kind, truncate_str(q, 50)));
            }
            WorkflowAction::Continue => {}
        }
    }

    // Remove completed/errored modes (reverse order to preserve indices)
    to_remove.sort_unstable();
    to_remove.dedup();
    for idx in to_remove.into_iter().rev() {
        if idx < mode_state.active_modes.len() {
            mode_state.active_modes.remove(idx);
        }
    }

    mode_state.updated_at = Some(chrono::Utc::now().to_rfc3339());
    summaries
}

/// Build the combined workflow prompt injection for all active modes.
///
/// This is the text that gets injected into the system prompt's dynamic_part.
pub fn build_workflow_prompt(mode_state: &ModeState) -> String {
    if mode_state.active_modes.is_empty() {
        return String::new();
    }

    let mut sections = Vec::new();
    sections.push("# Active Workflow Modes\n".to_string());

    for active_mode in &mode_state.active_modes {
        let Some(handler) = crate::workflow::get_handler(active_mode.workflow) else {
            continue;
        };

        let prompt = handler.build_prompt();
        let remaining = active_mode.turn_limit.saturating_sub(active_mode.turn_count);
        sections.push(format!(
            "## {} ({} turns remaining)\n\n{}\n",
            active_mode.workflow, remaining, prompt
        ));
    }

    sections.join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ActiveMode;
    use std::collections::HashMap;

    #[test]
    fn execute_empty_state() {
        let state = ModeState::default();
        let actions = execute_active_workflows(&state, "hello", None, "test-session");
        assert!(actions.is_empty());
    }

    #[test]
    fn process_empty_state() {
        let state = ModeState::default();
        let actions = process_turn_response(&state, "hello");
        assert!(actions.is_empty());
    }

    #[test]
    fn build_workflow_prompt_empty() {
        let state = ModeState::default();
        assert!(build_workflow_prompt(&state).is_empty());
    }

    #[test]
    fn build_workflow_prompt_with_active_mode() {
        let state = ModeState {
            active_modes: vec![ActiveMode {
                workflow: WorkflowKind::Ultrathink,
                activated_at: "2026-01-01T00:00:00Z".to_string(),
                turn_count: 0,
                turn_limit: 10,
                metadata: HashMap::new(),
            }],
            updated_at: None,
        };
        let prompt = build_workflow_prompt(&state);
        assert!(prompt.contains("ultrathink"));
        assert!(prompt.contains("10 turns remaining"));
    }

    #[test]
    fn apply_actions_persists_metadata() {
        let mut state = ModeState {
            active_modes: vec![ActiveMode {
                workflow: WorkflowKind::Tdd,
                activated_at: "2026-01-01T00:00:00Z".to_string(),
                turn_count: 0,
                turn_limit: 10,
                metadata: HashMap::new(),
            }],
            updated_at: None,
        };
        let mut new_meta = HashMap::new();
        new_meta.insert("tdd_phase".to_string(), "green".to_string());
        let actions = vec![(
            0,
            WorkflowKind::Tdd,
            WorkflowAction::ContinueWithMetadata {
                reminder: "test".to_string(),
                metadata: new_meta,
            },
        )];
        apply_actions(&mut state, &actions);
        assert_eq!(state.active_modes[0].metadata.get("tdd_phase").unwrap(), "green");
    }

    #[test]
    fn apply_actions_removes_completed() {
        let mut state = ModeState {
            active_modes: vec![
                ActiveMode {
                    workflow: WorkflowKind::Tdd,
                    activated_at: "2026-01-01T00:00:00Z".to_string(),
                    turn_count: 0,
                    turn_limit: 10,
                    metadata: HashMap::new(),
                },
                ActiveMode {
                    workflow: WorkflowKind::Ultrathink,
                    activated_at: "2026-01-01T00:00:00Z".to_string(),
                    turn_count: 0,
                    turn_limit: 10,
                    metadata: HashMap::new(),
                },
            ],
            updated_at: None,
        };
        let actions = vec![(
            0,
            WorkflowKind::Tdd,
            WorkflowAction::Complete("done".to_string()),
        )];
        apply_actions(&mut state, &actions);
        assert_eq!(state.active_modes.len(), 1);
        assert_eq!(state.active_modes[0].workflow, WorkflowKind::Ultrathink);
    }
}
