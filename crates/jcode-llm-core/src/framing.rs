use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Describes how the response stream is framed on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    /// Server-Sent Events (`text/event-stream`).
    Sse,
    /// AWS Event Stream binary framing (used by Bedrock).
    AwsEventStream,
    /// Binary messages over WebSocket.
    WebSocketBinary,
}

/// A single Server-Sent Events frame.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SseFrame {
    /// The SSE event type (field name `event`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// The SSE data payload (field name `data`).
    #[serde(default)]
    pub data: String,
    /// The SSE event ID (field name `id`), used for reconnection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl SseFrame {
    /// Parse a single SSE frame from raw bytes.
    ///
    /// Lines are split on `\n`, and recognised `event:`, `data:`, and `id:` fields
    /// are extracted.  A line with only `data:` (empty payload) is treated as a
    /// valid frame with an empty `data` string.
    pub fn parse(raw: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(raw).ok()?;
        let mut event = None;
        let mut data = String::new();
        let mut id = None;

        for line in text.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("data:") {
                data.push_str(value.trim());
            } else if line.starts_with("data:") && line.len() == 5 {
                // "data:" with nothing after it
            } else if let Some(value) = line.strip_prefix("id:") {
                id = Some(value.trim().to_string());
            }
        }

        // An SSE frame without any data field is not useful
        if data.is_empty() {
            return None;
        }

        Some(SseFrame { event, data, id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_frame_parse_basic() {
        let bytes = b"event: completion\ndata: Hello, world!\n\n";
        let frame = SseFrame::parse(bytes).unwrap();
        assert_eq!(frame.event.as_deref(), Some("completion"));
        assert_eq!(frame.data, "Hello, world!");
        assert_eq!(frame.id, None);
    }

    #[test]
    fn test_sse_frame_parse_with_id() {
        let bytes = b"id: 42\nevent: ping\ndata: {}\n\n";
        let frame = SseFrame::parse(bytes).unwrap();
        assert_eq!(frame.event.as_deref(), Some("ping"));
        assert_eq!(frame.id.as_deref(), Some("42"));
    }

    #[test]
    fn test_sse_frame_empty_data_returns_none() {
        let bytes = b"event: heartbeat\n\n";
        assert!(SseFrame::parse(bytes).is_none());
    }

    #[test]
    fn test_sse_frame_multiline_data() {
        let bytes = b"data: line1\ndata: line2\n\n";
        let frame = SseFrame::parse(bytes).unwrap();
        assert_eq!(frame.data, "line1line2");
    }
}
