//! Session-local todo persistence (file-backed JSON store).

pub use jcode_task_types::TodoItem;

use anyhow::Result;
use std::path::PathBuf;

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

/// Save todos for a session to disk.
pub fn save_todos(session_id: &str, todos: &[TodoItem]) -> Result<()> {
    let path = todo_path(session_id)?;
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    write_json_fast(&path, todos)
}
