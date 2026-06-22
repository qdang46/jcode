//! Todo panel helpers — sticky window selection and rendering primitives.
//!
//! Patterns:
//! - `select_sticky_window`: oh-my-pi `selectStickyTodoWindow`
//! - `marker_for_status`: oh-my-pi TUI todo renderer

use crate::todo::TodoItem;

/// Maximum visible todo items in the sticky panel.
const MAX_VISIBLE: usize = 5;

/// Display mode for the todo panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoPanelMode {
    /// Open tasks exist — show active window.
    Active,
    /// All tasks completed — show trailing context.
    AllCompletedClear,
}

/// Data for rendering the sticky todo panel.
#[derive(Debug, Clone)]
pub struct TodoPanelData {
    pub visible: Vec<TodoItem>,
    pub hidden_open_count: usize,
    pub mode: TodoPanelMode,
}

/// Select a subset of todos for the sticky panel.
///
/// When open tasks exist: show first MAX_VISIBLE open tasks + hidden count.
/// When all done: show last MAX_VISIBLE completed tasks as context.
/// Blocked tasks are treated as not-open.
pub fn select_sticky_window(todos: &[TodoItem]) -> TodoPanelData {
    let open: Vec<TodoItem> = todos
        .iter()
        .filter(|t| matches!(t.status.as_str(), "pending" | "in_progress"))
        .cloned()
        .collect();

    if !open.is_empty() {
        let visible: Vec<TodoItem> = open.iter().take(MAX_VISIBLE).cloned().collect();
        let hidden_open_count = open.len().saturating_sub(visible.len());
        return TodoPanelData {
            visible,
            hidden_open_count,
            mode: TodoPanelMode::Active,
        };
    }

    // All done — show last few completed as context.
    let completed: Vec<TodoItem> = todos
        .iter()
        .filter(|t| t.status == "completed")
        .cloned()
        .collect();
    let visible: Vec<TodoItem> = completed.iter().rev().take(MAX_VISIBLE).cloned().collect();
    TodoPanelData {
        visible,
        hidden_open_count: 0,
        mode: TodoPanelMode::AllCompletedClear,
    }
}

/// Unicode marker for a todo's status.
pub fn marker_for_status(status: &str) -> &'static str {
    match status {
        "completed" => "✓",
        "in_progress" => "→",
        "pending" => "○",
        "blocked" => "⊘",
        _ => "?",
    }
}

/// Short label for a todo's status.
pub fn label_for_status(status: &str) -> &'static str {
    match status {
        "completed" => "completed",
        "in_progress" => "in_progress",
        "pending" => "pending",
        "blocked" => "blocked",
        _ => "?",
    }
}

/// Progress summary: "3/5 completed".
pub fn progress_summary(todos: &[TodoItem]) -> String {
    let total = todos.len();
    if total == 0 {
        return "no todos".into();
    }
    let done = todos.iter().filter(|t| t.status == "completed").count();
    format!("{done}/{total} completed")
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
    fn active_mode_shows_open_first() {
        let todos = vec![
            item("a", "completed"),
            item("b", "pending"),
            item("c", "in_progress"),
            item("d", "pending"),
        ];
        let panel = select_sticky_window(&todos);
        assert_eq!(panel.mode, TodoPanelMode::Active);
        assert_eq!(panel.visible.len(), 3);
        assert_eq!(panel.visible[0].content, "b");
        assert_eq!(panel.visible[1].content, "c");
    }

    #[test]
    fn hidden_count_truncates() {
        let todos: Vec<_> = (0..10)
            .map(|i| item(&format!("t{i}"), "pending"))
            .collect();
        let panel = select_sticky_window(&todos);
        assert_eq!(panel.visible.len(), MAX_VISIBLE);
        assert_eq!(panel.hidden_open_count, 5);
    }

    #[test]
    fn all_done_shows_completed_context() {
        let todos = vec![
            item("first", "completed"),
            item("second", "completed"),
            item("last", "completed"),
        ];
        let panel = select_sticky_window(&todos);
        assert_eq!(panel.mode, TodoPanelMode::AllCompletedClear);
        assert_eq!(panel.visible[0].content, "last");
    }

    #[test]
    fn empty_list() {
        let panel = select_sticky_window(&[]);
        assert_eq!(panel.visible.len(), 0);
        assert_eq!(panel.hidden_open_count, 0);
    }

    #[test]
    fn blocked_is_not_open() {
        let todos = vec![item("blk", "blocked"), item("done", "completed")];
        let panel = select_sticky_window(&todos);
        assert_eq!(panel.mode, TodoPanelMode::AllCompletedClear);
    }

    #[test]
    fn progress_summary_counts() {
        let todos = vec![
            item("a", "completed"),
            item("b", "pending"),
            item("c", "in_progress"),
            item("d", "completed"),
        ];
        assert_eq!(progress_summary(&todos), "2/4 completed");
    }
}
