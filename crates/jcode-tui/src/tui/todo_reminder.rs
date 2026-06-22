//! TodoReminderState — tracks when to remind the model to update todos.
//!
//! Pattern: oh-my-pi `todo-reminder.ts`.
//!
//! The model often goes many tool calls without updating the todo list.
//! This component detects drift by counting tool calls and elapsed time
//! since the last todo update, then signals when a reminder is warranted.

use std::time::{Duration, Instant};
use crate::todo::TodoItem;

/// Tracks the timing and call count needed to decide whether to remind.
#[derive(Debug, Clone)]
pub struct TodoReminderState {
    pub last_todo_update: Option<Instant>,
    pub tool_calls_at_last_update: u64,
    pub tool_calls_since_start: u64,
    pub reminded_at: Option<Instant>,
}

impl TodoReminderState {
    pub fn new() -> Self {
        Self {
            last_todo_update: None,
            tool_calls_at_last_update: 0,
            tool_calls_since_start: 0,
            reminded_at: None,
        }
    }

    pub fn record_todo_update(&mut self) {
        self.last_todo_update = Some(Instant::now());
        self.tool_calls_at_last_update = self.tool_calls_since_start;
        self.reminded_at = None; // reset cooldown
    }

    pub fn record_tool_call(&mut self) {
        self.tool_calls_since_start += 1;
    }
}

/// Returns true if a reminder should be injected into the ambient prompt.
///
/// Conditions (all must be met):
/// 1. There are open (pending / in_progress) tasks
/// 2. It has been at least 60s since the last reminder (cooldown)
/// 3. Either ≥5 tool calls since last todo update, or ≥10 minutes elapsed
pub fn should_remind(state: &TodoReminderState, todos: &[TodoItem]) -> bool {
    if todos.is_empty() {
        return false;
    }
    let open_count = todos
        .iter()
        .filter(|t| matches!(t.status.as_str(), "pending" | "in_progress"))
        .count();
    if open_count == 0 {
        return false;
    }
    // Cooldown: don't re-remind within 60s
    if let Some(last) = state.reminded_at {
        if last.elapsed() < Duration::from_secs(60) {
            return false;
        }
    }
    let calls_since = state
        .tool_calls_since_start
        .saturating_sub(state.tool_calls_at_last_update);
    let time_since = state
        .last_todo_update
        .map(|t| t.elapsed())
        .unwrap_or(Duration::from_secs(u64::MAX));
    calls_since >= 5 || time_since >= Duration::from_secs(600)
}

/// Format a reminder message for ambient prompt injection.
pub fn render_reminder(todos: &[TodoItem]) -> String {
    let open: Vec<&TodoItem> = todos
        .iter()
        .filter(|t| matches!(t.status.as_str(), "pending" | "in_progress"))
        .collect();
    if open.is_empty() {
        return String::new();
    }
    let list = open
        .iter()
        .take(3)
        .map(|t| format!("- [{}] {}", status_label(&t.status), t.content))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "[Todo reminder] You have {} open task(s). Consider updating the todo list:\n{list}",
        open.len()
    )
}

fn status_label(status: &str) -> &'static str {
    match status {
        "completed" => "x",
        "in_progress" => ">",
        "pending" => " ",
        "blocked" => "!",
        _ => " ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(content: &str, status: &str) -> TodoItem {
        TodoItem {
            content: content.into(),
            status: status.into(),
            ..Default::default()
        }
    }

    #[test]
    fn no_remind_when_empty() {
        let state = TodoReminderState::new();
        assert!(!should_remind(&state, &[]));
    }

    #[test]
    fn no_remind_when_all_done() {
        let state = TodoReminderState::new();
        let todos = vec![item("x", "completed")];
        assert!(!should_remind(&state, &todos));
    }

    #[test]
    fn remind_after_5_tool_calls() {
        let mut state = TodoReminderState::new();
        state.record_todo_update();
        for _ in 0..5 {
            state.record_tool_call();
        }
        let todos = vec![item("x", "pending")];
        assert!(should_remind(&state, &todos));
    }

    #[test]
    fn cooldown_prevents_spam() {
        let mut state = TodoReminderState::new();
        state.reminded_at = Some(Instant::now());
        let todos = vec![item("x", "pending")];
        assert!(!should_remind(&state, &todos));
    }

    #[test]
    fn render_empty_when_no_open() {
        let todos = vec![item("x", "completed")];
        assert!(render_reminder(&todos).is_empty());
    }

    #[test]
    fn render_lists_open_tasks() {
        let todos = vec![
            item("task a", "pending"),
            item("task b", "in_progress"),
        ];
        let msg = render_reminder(&todos);
        assert!(msg.contains("2 open"));
        assert!(msg.contains("task a"));
        assert!(msg.contains("task b"));
    }
}
