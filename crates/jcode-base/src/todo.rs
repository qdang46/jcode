//! Session-local todo persistence (file-backed JSON store).

pub use jcode_task_types::TodoItem;

use anyhow::Result;
use std::path::PathBuf;

use crate::bus::{Bus, BusEvent, TodoEvent};
use crate::storage::{self, read_json, write_json_fast};

fn todo_path(session_id: &str) -> Result<PathBuf> {
    let base = storage::jcode_dir()?;
    Ok(base.join("todos").join(format!("{}.json", session_id)))
}

/// Load todos for a session from disk.
pub fn load_todos(session_id: &str) -> Result<Vec<TodoItem>> {
    let path = todo_path(session_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    read_json(&path).or_else(|_| Ok(Vec::new()))
}

/// Check if any todos exist for a session.
pub fn todos_exist(session_id: &str) -> Result<bool> {
    Ok(todo_path(session_id)?.exists())
}

/// Save todos for a session to disk + broadcast TodoUpdated event.
///
/// Returns `Ok(true)` if a verification nudge should be injected into the
/// tool result (model just closed 3+ tasks without a verification step).
/// Caller (TodoTool) reads this and appends reminder text to its output.
///
/// Returns `Ok(false)` when no nudge is warranted, or when the save completed
/// but the verification check did not fire.
pub fn save_todos(session_id: &str, todos: &[TodoItem]) -> Result<bool> {
    let path = todo_path(session_id)?;
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    // Load previous state to compute verification nudge delta.
    let previous = load_todos(session_id).unwrap_or_default();
    let nudge = needs_verification_nudge(&previous, todos);
    write_json_fast(&path, todos)?;
    // Broadcast update to subscribers (TUI panel, metrics, etc.).
    Bus::global().publish(BusEvent::TodoUpdated(TodoEvent {
        session_id: session_id.to_string(),
        todos: todos.to_vec(),
        at: chrono::Utc::now(),
    }));
    Ok(nudge)
}

/// Detect close-out 3+ tasks không có verification step.
/// Source: claude-code v1 verificationNudgeNeeded.
///
/// Returns true khi model vừa complete ≥3 task (chưa complete trước đó)
/// và không có task nào chứa "verif" (case-insensitive). Khi false,
/// tool không cần inject reminder.
///
/// `previous` = state trước save; `updated` = state sau save.
pub fn needs_verification_nudge(previous: &[TodoItem], updated: &[TodoItem]) -> bool {
    use std::collections::HashSet;
    let was_completed: HashSet<&str> = previous
        .iter()
        .filter(|t| t.status == "completed")
        .map(|t| t.content.as_str())
        .collect();
    let newly_completed: Vec<&TodoItem> = updated
        .iter()
        .filter(|t| t.status == "completed" && !was_completed.contains(t.content.as_str()))
        .collect();
    if newly_completed.len() < 3 {
        return false;
    }
    !newly_completed.iter().any(|t| {
        t.content.to_ascii_lowercase().contains("verif")
            || t.active_form
                .as_deref()
                .map(|s| s.to_ascii_lowercase().contains("verif"))
                .unwrap_or(false)
    })
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
    fn triggers_at_3_no_verif() {
        let prev = vec![];
        let updated = vec![
            item("write code", "completed"),
            item("run lint", "completed"),
            item("commit", "completed"),
        ];
        assert!(needs_verification_nudge(&prev, &updated));
    }

    #[test]
    fn skipped_when_verif_present() {
        let prev = vec![];
        let updated = vec![
            item("write code", "completed"),
            item("run tests", "completed"),
            item("verify build", "completed"),
        ];
        assert!(!needs_verification_nudge(&prev, &updated));
    }

    #[test]
    fn counts_only_newly_completed() {
        let prev = vec![item("old done", "completed")];
        let updated = vec![
            item("old done", "completed"),
            item("new a", "completed"),
            item("new b", "completed"),
        ];
        assert!(!needs_verification_nudge(&prev, &updated));
    }

    #[test]
    fn below_threshold() {
        let prev = vec![];
        let updated = vec![item("a", "completed"), item("b", "completed")];
        assert!(!needs_verification_nudge(&prev, &updated));
    }

    #[test]
    fn case_insensitive_verif() {
        let prev = vec![];
        let updated = vec![
            item("Run VERIFICATION", "completed"),
            item("b", "completed"),
            item("c", "completed"),
        ];
        assert!(!needs_verification_nudge(&prev, &updated));
    }

    #[test]
    fn active_form_counts_as_verif() {
        let prev = vec![];
        let mut i1 = item("a", "completed");
        i1.active_form = Some("Verifying x".into());
        let updated = vec![i1, item("b", "completed"), item("c", "completed")];
        assert!(!needs_verification_nudge(&prev, &updated));
    }
}
