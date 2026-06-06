//! Workflow execution engine.
//!
//! Bridges the keyword system with the agent runtime. Called from the turn loop
//! to execute active workflows and produce actions (spawn agents, inject reminders, etc.).

use super::{WorkflowAction, WorkflowContext};
use crate::registry::WorkflowKind;
use crate::state::ModeState;
use crate::task_size::TaskSize;

/// Truncate a string to at most `max_chars` Unicode scalar values
/// (i.e. characters), respecting UTF-8 boundaries.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    // Walk char indices and stop at the max_chars-th character.
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
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
    task_size: TaskSize,
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

        // Heavy workflows are suppressed for Simple tasks (e.g. one-line requests)
        // so we don't burn tokens on a multi-agent workflow for a trivial fix.
        if handler.should_suppress_for_task_size(task_size) {
            continue;
        }

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

/// Spawn actions whose execution was deferred to the caller.
///
/// `SpawnAgent` and `SpawnParallel` need access to the agent runtime
/// (provider, tool registry, etc.) which lives in `jcode-app-core`, not in
/// `jcode-keywords`. `apply_actions` records the spawn in metadata and
/// returns these so the caller can dispatch them via `SubagentTool`.
#[derive(Debug, Clone)]
pub struct DeferredSpawn {
    /// Index of the active mode that produced the spawn.
    pub mode_index: usize,
    /// The workflow kind that requested the spawn.
    pub kind: WorkflowKind,
    /// The action to dispatch.
    pub action: WorkflowAction,
}

/// Apply workflow actions to mode state (metadata persistence, mode deactivation).
///
/// This is the key function that persists `ContinueWithMetadata` and `Complete` actions.
/// Returns `(summaries, deferred_spawns)`. Spawn actions are recorded in
/// metadata so we do not loop, and surfaced to the caller for execution.
pub fn apply_actions(
    mode_state: &mut ModeState,
    actions: &[(usize, WorkflowKind, WorkflowAction)],
) -> (Vec<String>, Vec<DeferredSpawn>) {
    let mut summaries = Vec::new();
    let mut to_remove = Vec::new();
    let mut deferred_spawns = Vec::new();

    for (idx, kind, action) in actions {
        match action {
            WorkflowAction::ContinueWithMetadata { metadata, reminder } => {
                if let Some(mode) = mode_state.active_modes.get_mut(*idx) {
                    // Merge new metadata into existing (don't discard)
                    for (k, v) in metadata {
                        mode.metadata.insert(k.clone(), v.clone());
                    }
                    summaries.push(format!(
                        "{}: updated metadata, reminder: {}",
                        kind,
                        truncate_str(reminder, 50)
                    ));
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
                summaries.push(format!(
                    "{}: inject reminder — {}",
                    kind,
                    truncate_str(r, 50)
                ));
            }
            WorkflowAction::SpawnAgent { description, .. } => {
                if let Some(mode) = mode_state.active_modes.get_mut(*idx) {
                    mode.metadata
                        .insert(format!("{}_spawned", kind), "true".to_string());
                }
                summaries.push(format!(
                    "{}: spawn agent deferred — {} (caller must dispatch via SubagentTool)",
                    kind, description
                ));
                deferred_spawns.push(DeferredSpawn {
                    mode_index: *idx,
                    kind: *kind,
                    action: action.clone(),
                });
            }
            WorkflowAction::SpawnParallel(specs) => {
                if let Some(mode) = mode_state.active_modes.get_mut(*idx) {
                    mode.metadata
                        .insert(format!("{}_spawned", kind), "true".to_string());
                }
                summaries.push(format!(
                    "{}: spawn {} agents deferred (caller must dispatch via SubagentTool)",
                    kind,
                    specs.len()
                ));
                for spec in specs {
                    deferred_spawns.push(DeferredSpawn {
                        mode_index: *idx,
                        kind: *kind,
                        action: WorkflowAction::SpawnAgent {
                            description: spec.description.clone(),
                            prompt: spec.prompt.clone(),
                            system_prompt: spec.system_prompt.clone(),
                            max_turns: spec.max_turns,
                        },
                    });
                }
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
    (summaries, deferred_spawns)
}

/// Result of a turn's keyword processing.
pub struct TurnResult {
    /// Prompt section to inject into the system prompt's dynamic part.
    /// `None` means no active workflow (or empty input).
    pub keyword_prompt: Option<String>,
    /// Mode conflicts (TDD + ultrawork, etc.) detected among the now-active
    /// modes. Callers are expected to surface these to logs/UI.
    pub conflicts: Vec<crate::conflict::Conflict>,
}

/// One-shot keyword processing for a turn.
///
/// This is the canonical entry point used by both the agent runtime and the
/// TUI: it runs the full detect -> update -> process-response -> execute
/// pipeline against the latest user input, persists state to disk, and
/// returns the workflow prompt section to inject into the system prompt.
///
/// `keyword_prompt` is `None` if the input is empty or no workflow is active.
/// `conflicts` is always computed when there is input and at least one
/// active mode.
pub fn process_turn(
    latest_input: &str,
    last_assistant: Option<&str>,
    working_dir: Option<&std::path::Path>,
    session_id: &str,
) -> TurnResult {
    if latest_input.is_empty() {
        return TurnResult {
            keyword_prompt: None,
            conflicts: Vec::new(),
        };
    }

    let detections = crate::detector::detect_keywords(latest_input);
    let mut mode_state = crate::state::update_modes(&detections, working_dir);

    // Process PREVIOUS turn's LLM response (phase transitions, completion)
    if let Some(prev) = last_assistant {
        let response_actions = process_turn_response(&mode_state, prev);
        if !response_actions.is_empty() {
            let _ = apply_actions(&mut mode_state, &response_actions);
        }
    }

    // Execute active workflows for THIS turn (heavy ones suppress on simple input)
    let task_size = crate::task_size::classify(latest_input);
    let actions = execute_active_workflows(
        &mode_state,
        latest_input,
        working_dir,
        session_id,
        task_size,
    );
    if !actions.is_empty() {
        let _ = apply_actions(&mut mode_state, &actions);
    }

    // Detect conflicts among the now-active modes (TDD + ultrawork, etc.)
    let active_kinds: Vec<crate::registry::WorkflowKind> =
        mode_state.active_modes.iter().map(|m| m.workflow).collect();
    let conflicts = crate::conflict::check_conflicts(&active_kinds);

    // Persist state
    crate::state::save_state(&mode_state, working_dir);

    let prompt = build_workflow_prompt(&mode_state);
    TurnResult {
        keyword_prompt: if prompt.is_empty() {
            None
        } else {
            Some(prompt)
        },
        conflicts,
    }
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
        let remaining = active_mode
            .turn_limit
            .saturating_sub(active_mode.turn_count);
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
        let actions =
            execute_active_workflows(&state, "hello", None, "test-session", TaskSize::Medium);
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
        assert_eq!(
            state.active_modes[0].metadata.get("tdd_phase").unwrap(),
            "green"
        );
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
        let (_summaries, deferred) = apply_actions(&mut state, &actions);
        assert!(deferred.is_empty());
        assert_eq!(state.active_modes.len(), 1);
        assert_eq!(state.active_modes[0].workflow, WorkflowKind::Ultrathink);
    }

    #[test]
    fn apply_actions_defers_spawn_actions() {
        let mut state = ModeState {
            active_modes: vec![ActiveMode {
                workflow: WorkflowKind::CodeReview,
                activated_at: "2026-01-01T00:00:00Z".to_string(),
                turn_count: 0,
                turn_limit: 10,
                metadata: HashMap::new(),
            }],
            updated_at: None,
        };
        let actions = vec![(
            0,
            WorkflowKind::CodeReview,
            WorkflowAction::SpawnAgent {
                description: "test agent".to_string(),
                prompt: "do thing".to_string(),
                system_prompt: "you are a tester".to_string(),
                max_turns: 5,
            },
        )];
        let (_summaries, deferred) = apply_actions(&mut state, &actions);
        assert_eq!(deferred.len(), 1);
        // Metadata flag is set so we do not loop
        assert_eq!(
            state.active_modes[0].metadata.get("code-review_spawned"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn truncate_str_respects_char_boundaries() {
        // 50 chars of CJK = 150 bytes; must keep all 50 chars, not truncate to 50 bytes.
        let s: String = "中".repeat(100);
        let out = truncate_str(&s, 50);
        assert_eq!(out.chars().count(), 50);
        assert!(out.chars().all(|c| c == '中'));
    }

    #[test]
    fn truncate_str_short_input_passes_through() {
        assert_eq!(truncate_str("hello", 50), "hello");
    }
}
