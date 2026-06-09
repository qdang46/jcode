//! Memory Extraction Service
//!
//! Uses the forked agent pattern to run a memory extraction subagent
//! that shares the parent's prompt cache. Triggered by stop hooks
//! after each complete turn.
//!
//! ## Flow
//! 1. Agent stop hooks fire after a complete turn
//! 2. If memory extraction is enabled and due (enough new messages):
//!    a. Check if parent already wrote memories (skip if so)
//!    b. Build extraction prompt from recent messages + existing memory scan
//!    c. Call fork runner with `ForkPermissionMode::MemoryExtraction`
//!    d. Advance cursor past processed messages
//!    e. Fork's output is written to memory directory
//!
//! ## Cursor-based Processing
//! Tracks `last_processed_index` to avoid re-extracting from already-processed
//! messages. The cursor is persisted per-session so it survives restarts.
//!
//! ## Why Skip When Parent Wrote Memories?
//! The main agent's system prompt includes full save instructions. When the
//! main agent writes memories itself (via Write/Edit tool calls targeting
//! the memory directory), the forked extraction is redundant. We detect this
//! via `has_memory_writes_since()` and skip the fork, advancing the cursor.

use jcode_message_types::{ContentBlock, Message};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};

/// Cursor tracking for incremental message processing.
///
/// Tracks how far through the message list we've processed.
/// Uses a simple message-count-based index since jcode messages
/// do not carry UUIDs.
///
/// Persisted per-session so extraction progress survives restarts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractionCursor {
    /// Index of the last processed message in the messages list.
    /// 0 means no messages processed yet.
    pub last_processed_index: usize,

    /// Session ID this cursor belongs to.
    pub session_id: String,

    /// When the last extraction ran.
    pub last_extracted_at: Option<DateTime<Utc>>,
}

impl ExtractionCursor {
    pub fn new(session_id: &str) -> Self {
        Self {
            last_processed_index: 0,
            session_id: session_id.to_string(),
            last_extracted_at: None,
        }
    }
}

/// Configuration for the memory extraction fork.
///
/// Stored in `config.toml` under `[forked_agent.memory_extraction]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryExtractionConfig {
    /// Enable automatic memory extraction.
    pub enabled: bool,

    /// Memory directory path (relative to working dir).
    /// Example: ".jcode/memory"
    pub memory_dir: PathBuf,

    /// Max turns for the extraction agent.
    /// Each turn is one API call + tool execution.
    /// CCB default: 3 (1 read turn + 2 write turns).
    pub max_turns: u32,

    /// Max output tokens per turn.
    pub max_output_tokens: u32,

    /// Minimum number of new (unprocessed) messages before triggering extraction.
    /// Avoids running extraction on every single turn.
    pub min_new_messages: usize,

    /// The extraction prompt template variant to use.
    /// "auto" -> auto-only (single user scope)
    /// "combined" -> auto + team (multi-scope)
    pub prompt_variant: ExtractionPromptVariant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExtractionPromptVariant {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "combined")]
    Combined,
}

impl Default for MemoryExtractionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            memory_dir: PathBuf::from(".jcode/memory"),
            max_turns: 3,
            max_output_tokens: 4096,
            min_new_messages: 5,
            prompt_variant: ExtractionPromptVariant::Auto,
        }
    }
}

/// Check if the parent agent already wrote memories in recent messages.
///
/// Scans messages starting from `since_index` for tool calls targeting
/// the memory directory. If found, the fork is redundant and should skip.
///
/// CCB reference: `hasMemoryWritesSince()` in extractMemories.ts
pub fn has_memory_writes_since(
    messages: &[Message],
    since_index: usize,
    memory_dir: &Path,
) -> bool {
    for (i, message) in messages.iter().enumerate() {
        if i < since_index {
            continue;
        }
        if message.role != jcode_message_types::Role::Assistant {
            continue;
        }
        // Check if any tool call targets the memory directory
        for block in &message.content {
            if let ContentBlock::ToolUse { name, input, .. } = block {
                if matches!(
                    name.as_str(),
                    "write" | "edit" | "create" | "file_write" | "file_edit"
                ) {
                    if let Some(path) = input
                        .get("file_path")
                        .or_else(|| input.get("path"))
                        .and_then(|p| p.as_str())
                    {
                        // Normalize to prevent ../ path traversal bypass
                        let p = normalize_path_for_extraction(&PathBuf::from(path));
                        if p.starts_with(memory_dir) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Count model-visible messages from `since_index` onward.
///
/// Model-visible = user + assistant messages (excludes progress-only).
fn count_model_visible_messages_since(messages: &[Message], since_index: usize) -> usize {
    messages
        .iter()
        .skip(since_index)
        .filter(|m| {
            matches!(
                m.role,
                jcode_message_types::Role::User | jcode_message_types::Role::Assistant
            )
        })
        .count()
}

/// Normalize a path by resolving "." and ".." components without filesystem access.
/// Prevents path traversal attacks via "../" segments.
fn normalize_path_for_extraction(path: &PathBuf) -> PathBuf {
    use std::path::Component;
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                components.pop();
            }
            Component::CurDir => {}
            other => {
                components.push(other.as_os_str().to_os_string());
            }
        }
    }
    let mut result = PathBuf::new();
    for c in components {
        result.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_message_types::{ContentBlock, Message, Role};

    fn create_message(text: &str, role: Role) -> Message {
        Message {
            role,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
                cache_control: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        }
    }

    #[test]
    fn test_has_memory_writes_since() {
        let memory_dir = PathBuf::from(".jcode/memory");

        // Assistant message with a write tool call to memory dir
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call1".to_string(),
                name: "write".to_string(),
                input: serde_json::json!({"file_path": ".jcode/memory/user_role.md"}),
                thought_signature: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        }];

        assert!(has_memory_writes_since(&messages, 0, &memory_dir));

        // Without memory writes
        let messages_no_write = vec![Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call2".to_string(),
                name: "read".to_string(),
                input: serde_json::json!({"file_path": "src/main.rs"}),
                thought_signature: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        }];
        assert!(!has_memory_writes_since(&messages_no_write, 0, &memory_dir));
    }

    #[test]
    fn test_count_model_visible_messages_since() {
        let messages = vec![
            create_message("msg1", Role::User),
            create_message("response1", Role::Assistant),
            create_message("msg2", Role::User),
            create_message("response2", Role::Assistant),
        ];

        assert_eq!(count_model_visible_messages_since(&messages, 0), 4);
        assert_eq!(count_model_visible_messages_since(&messages, 2), 2);
        assert_eq!(count_model_visible_messages_since(&messages, 4), 0);
    }
}
