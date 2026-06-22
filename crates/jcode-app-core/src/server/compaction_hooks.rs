//! Compaction-survival hooks: extract todo state from message log sau khi
//! context bị compact. Source: claude-code v1
//! `src/utils/sessionRestore.ts:extractTodosFromTranscript`.
//!
//! Sau khi compaction, model context mất các tool_use block cũ (kể cả
//! TodoWrite), nhưng state vẫn còn trong log. Scan ngược tìm tool_use
//! block cuối cùng của tool "todo", restore state. Zero in-memory state,
//! fail-safe.

use crate::bus::{Bus, BusEvent, TodoEvent};
use anyhow::Result;

use chrono::Utc;
use jcode_task_types::TodoItem;
use serde_json::Value;

/// Extract most recent todo list from message log.
/// Returns empty Vec when không có todo block nào, hoặc messages invalid.
///
/// `messages` = JSON array of message objects. Mỗi message có shape:
///   { "role": "assistant", "content": [{"type": "tool_use", "name": "todo",
///                                       "input": {"todos": [...]}}] }
pub fn extract_todos_from_transcript(messages: &[Value]) -> Vec<TodoItem> {
    for msg in messages.iter().rev() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant" {
            continue;
        }
        let content = match msg.get("content") {
            Some(c) if c.is_array() => c.as_array().unwrap(),
            _ => continue,
        };
        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let block_name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if block_type == "tool_use" && block_name == "todo" {
                if let Some(input) = block.get("input") {
                    if let Some(todos_raw) = input.get("todos") {
                        return normalize_and_parse(todos_raw);
                    }
                }
            }
        }
    }
    Vec::new()
}

/// Parse todo array từ JSON value. Handle provider quirks:
/// 1. Accept array directly
/// 2. Accept stringified JSON (claude sometimes stringifies tool args)
/// 3. Skip malformed items silently
fn normalize_and_parse(raw: &Value) -> Vec<TodoItem> {
    let arr: Vec<Value> = if let Some(a) = raw.as_array() {
        a.clone()
    } else if let Some(s) = raw.as_str() {
        match serde_json::from_str::<Vec<Value>>(s.trim()) {
            Ok(a) => a,
            Err(_) => return Vec::new(),
        }
    } else {
        return Vec::new();
    };
    arr.into_iter().filter_map(parse_one).collect()
}

fn parse_one(item: Value) -> Option<TodoItem> {
    let content = item.get("content")?.as_str()?.to_string();
    let status_str = item.get("status")?.as_str()?;
    let status = match status_str {
        "pending" => "pending",
        "in_progress" => "in_progress",
        "completed" => "completed",
        "blocked" => "blocked",
        _ => return None,
    };
    Some(TodoItem {
        content,
        status: status.to_string(),
        active_form: item
            .get("active_form")
            .and_then(|v| v.as_str())
            .map(String::from),
        ..Default::default()
    })
}

/// Public entrypoint: extract + persist + broadcast. Called from
/// compaction code path. Returns extracted todos for caller info.
pub fn restore_todos_after_compaction(
    session_id: &str,
    messages: &[Value],
) -> Result<Vec<TodoItem>> {
    let todos = extract_todos_from_transcript(messages);
    if todos.is_empty() {
        return Ok(Vec::new());
    }
    crate::todo::save_todos(session_id, &todos)?;
    Bus::global().publish(BusEvent::TodoUpdated(TodoEvent {
        session_id: session_id.to_string(),
        todos: todos.clone(),
        at: Utc::now(),
    }));
    Ok(todos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn todo_block(todos: Value) -> Value {
        json!({
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "name": "todo",
                "input": {"todos": todos}
            }]
        })
    }

    #[test]
    fn extracts_last_todo_block() {
        let messages = vec![
            json!({"role": "user", "content": [{"type": "text", "text": "do stuff"}]}),
            todo_block(json!([{"content": "old task", "status": "pending"}])),
            json!({"role": "user", "content": [{"type": "tool_result", "content": "ok"}]}),
            todo_block(json!([
                {"content": "task A", "status": "completed"},
                {"content": "task B", "status": "in_progress"}
            ])),
        ];
        let todos = extract_todos_from_transcript(&messages);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].content, "task A");
        assert_eq!(todos[1].status, "in_progress");
    }

    #[test]
    fn normalizes_stringified_json() {
        let raw = json!("[{\"content\":\"x\",\"status\":\"pending\"}]");
        let parsed = normalize_and_parse(&raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].content, "x");
    }

    #[test]
    fn empty_when_no_todo_block() {
        let messages = vec![json!({"role": "user", "content": []})];
        assert!(extract_todos_from_transcript(&messages).is_empty());
    }

    #[test]
    fn ignores_user_messages() {
        let messages = vec![json!({
            "role": "user",
            "content": [{"type": "tool_use", "name": "todo",
                         "input": {"todos": [{"content": "x", "status": "pending"}]}}]
        })];
        assert!(extract_todos_from_transcript(&messages).is_empty());
    }

    #[test]
    fn skips_malformed_items_gracefully() {
        let raw = json!([
            {"content": "valid", "status": "pending"},
            {"content": "missing status"},
            {"status": "pending"},
            {"content": "bad status", "status": "frozen"},
        ]);
        let parsed = normalize_and_parse(&raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].content, "valid");
    }

    #[test]
    fn handles_active_form() {
        let raw = json!([{
            "content": "Run tests",
            "status": "in_progress",
            "active_form": "Running tests"
        }]);
        let parsed = normalize_and_parse(&raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].active_form.as_deref(), Some("Running tests"));
    }

    #[test]
    fn returns_first_match_scanning_reverse() {
        let messages = vec![
            todo_block(json!([{"content": "newest", "status": "completed"}])),
            todo_block(json!([{"content": "oldest", "status": "pending"}])),
        ];
        let todos = extract_todos_from_transcript(&messages);
        // Reverse scan → newest first
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "newest");
    }
}
