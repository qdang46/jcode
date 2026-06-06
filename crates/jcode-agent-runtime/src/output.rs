//! How an agent's output is delivered back to its parent.
//!
//! Adapted from Codebuff's `outputMode` field. Three modes cover the
//! useful cases:
//!
//! - `LastMessage`: parent receives only the agent's final assistant turn.
//!   Default. Good for "research-and-summarize" agents like file-picker.
//! - `AllMessages`: parent receives the full child message history
//!   (text + tool calls + tool results). Good for editor-like agents
//!   that need to expose their full edit trace.
//! - `StructuredOutput`: agent must call `set_output` with a JSON value
//!   that conforms to `output_schema`. Good for judge agents, lessons
//!   extractors, structured planners.

use serde::{Deserialize, Serialize};

/// Output delivery mode for a sub-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// Parent receives only the final assistant turn. (Default.)
    #[default]
    LastMessage,
    /// Parent receives the full message history of the child agent.
    AllMessages,
    /// Agent must produce a JSON object conforming to its `output_schema`.
    /// Validated on `set_output` tool call.
    StructuredOutput,
}

impl OutputMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputMode::LastMessage => "last_message",
            OutputMode::AllMessages => "all_messages",
            OutputMode::StructuredOutput => "structured_output",
        }
    }

    pub fn parse(s: &str) -> Option<OutputMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "last_message" | "lastmessage" | "last" => Some(OutputMode::LastMessage),
            "all_messages" | "allmessages" | "all" => Some(OutputMode::AllMessages),
            "structured_output" | "structured" | "json" => Some(OutputMode::StructuredOutput),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_aliases() {
        assert_eq!(
            OutputMode::parse("last_message"),
            Some(OutputMode::LastMessage)
        );
        assert_eq!(OutputMode::parse("all"), Some(OutputMode::AllMessages));
        assert_eq!(
            OutputMode::parse("structured"),
            Some(OutputMode::StructuredOutput)
        );
        assert_eq!(OutputMode::parse("nonsense"), None);
    }

    #[test]
    fn default_is_last_message() {
        assert_eq!(OutputMode::default(), OutputMode::LastMessage);
    }

    #[test]
    fn serde_uses_snake_case() {
        let s = serde_json::to_string(&OutputMode::StructuredOutput).unwrap();
        assert_eq!(s, "\"structured_output\"");
    }
}
