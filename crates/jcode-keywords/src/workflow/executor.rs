//! Workflow execution engine.
//!
//! Bridges the keyword system with the agent runtime. Called from the turn loop
//! to execute active workflows and produce actions (spawn agents, inject reminders, etc.).

use super::{SpawnSpec, WorkflowAction, WorkflowContext};
use crate::registry::WorkflowKind;
use crate::state::ModeState;

/// Execute all active workflows for the current turn.
///
/// Called from `build_system_prompt_split` or the turn loop. Returns the
/// combined actions from all active workflow handlers.
pub fn execute_active_workflows(
    mode_state: &ModeState,
    user_input: &str,
    working_dir: Option<&std::path::Path>,
    session_id: &str,
) -> Vec<(WorkflowKind, WorkflowAction)> {
    let mut actions = Vec::new();

    for active_mode in &mode_state.active_modes {
        let Some(handler) = crate::workflow::get_handler(active_mode.workflow) else {
            continue;
        };

        let ctx = WorkflowContext {
            user_input: user_input.to_string(),
            working_dir: working_dir.map(|p| p.to_path_buf()),
            session_id: session_id.to_string(),
            mode_state: mode_state.clone(),
            metadata: active_mode.metadata.clone(),
        };

        let action = handler.execute(&ctx);
        actions.push((active_mode.workflow, action));
    }

    actions
}

/// Process the LLM's response through all active workflow handlers.
///
/// Called after each turn completes. Handlers can inspect the response
/// and decide whether to continue, complete, or ask for more input.
pub fn process_turn_response(
    mode_state: &ModeState,
    response: &str,
) -> Vec<(WorkflowKind, WorkflowAction)> {
    let mut actions = Vec::new();

    for active_mode in &mode_state.active_modes {
        let Some(handler) = crate::workflow::get_handler(active_mode.workflow) else {
            continue;
        };

        let action = handler.on_turn_complete(response, &active_mode.metadata);
        actions.push((active_mode.workflow, action));
    }

    actions
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
    sections.push("The user has activated the following workflows:\n".to_string());

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

/// Create a SpawnSpec for a workflow sub-agent.
pub fn make_spawn_spec(
    description: &str,
    prompt: &str,
    system_prompt: &str,
    max_turns: u32,
) -> SpawnSpec {
    SpawnSpec {
        description: description.to_string(),
        prompt: prompt.to_string(),
        system_prompt: system_prompt.to_string(),
        max_turns,
    }
}

/// Build a system prompt for a workflow sub-agent.
pub fn build_subagent_system_prompt(workflow: WorkflowKind, base_instructions: &str) -> String {
    let handler_prompt = crate::workflow::get_handler(workflow)
        .map(|h| h.build_prompt())
        .unwrap_or_default();

    format!(
        "{}\n\n{}\n\nYou are a specialized sub-agent executing a workflow step. \
         Focus on completing your assigned task efficiently. \
         Report your results clearly and concisely.",
        handler_prompt, base_instructions
    )
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
}
