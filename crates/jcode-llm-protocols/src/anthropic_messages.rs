use async_trait::async_trait;
use jcode_llm_core::endpoint::{Endpoint, PathSpec};
use jcode_llm_core::framing::{Framing, SseFrame};
use jcode_llm_core::protocol::{Protocol, StepOutput};
use jcode_llm_core::route::PreparedRoute;
use jcode_llm_core::schema::{ContentPart, LlmRequest, ToolChoice, Usage};
use jcode_llm_core::transport::Transport;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Request body types (Anthropic Messages API JSON shape)
// ---------------------------------------------------------------------------

/// The top-level request body sent to POST /v1/messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicBody {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<AnthropicToolChoice>,
    pub max_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub stream: bool,
}

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Vec<AnthropicContent>,
}

/// A content block inside a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

/// Image source block (only base64 currently).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// A tool definition in Anthropic format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// How the model should pick tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

// ---------------------------------------------------------------------------
// SSE event types returned by the Anthropic Messages API
// ---------------------------------------------------------------------------

/// Events produced by the Anthropic streaming API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum AnthropicEvent {
    /// Initial message metadata (usage, id, model, stop_reason, stop_sequence).
    MessageStart {
        #[serde(rename = "message")]
        _message: AnthropicMessageStartInfo,
    },
    /// A new content block started (text, tool_use, or thinking).
    ContentBlockStart {
        index: u64,
        content_block: AnthropicContentBlockStart,
    },
    /// A delta inside an existing content block.
    ContentBlockDelta {
        index: u64,
        delta: AnthropicContentDelta,
    },
    /// A content block finished with an optional stop reason.
    ContentBlockStop { index: u64 },
    /// Top-level message delta (stop_reason, stop_sequence, usage).
    MessageDelta {
        delta: AnthropicMessageDeltaInfo,
        usage: AnthropicUsage,
    },
    /// Streaming finished.
    MessageStop,
    /// Ping event.
    Ping,
    /// Error event.
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        error: AnthropicErrorBody,
    },
}

/// Metadata inside `message_start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessageStartInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub role: String,
    pub model: String,
    pub content: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

/// The content block variant inside `content_block_start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentBlockStart {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    Thinking {
        thinking: String,
    },
}

/// A delta inside `content_block_delta`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicContentDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    ThinkingDelta { thinking: String },
}

/// Delta info in `message_delta`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessageDeltaInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Usage object in Anthropic streaming events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
}

/// Error body in Anthropic SSE error events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnthropicErrorBody {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Protocol state
// ---------------------------------------------------------------------------

/// Opaque state carried across `step()` calls.
#[derive(Debug, Clone)]
pub struct AnthropicState {
    /// Accumulated SSE data buffer (newline-delimited lines).
    buffer: String,
    /// Accumulated usage across the stream.
    accumulated_usage: Usage,
    /// Whether we have seen `message_stop` (terminal).
    done: bool,
    /// Whether we have seen `message_start`.
    started: bool,
    /// In-progress tool call input JSON being accumulated across deltas.
    pending_tool_json: Option<(String, String)>,
    /// In-progress thinking text being accumulated.
    pending_thinking: bool,
}

impl Default for AnthropicState {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            accumulated_usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                total_tokens: 0,
                breakdown: None,
            },
            done: false,
            started: false,
            pending_tool_json: None,
            pending_thinking: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: map our ToolChoice into the Anthropic variant
// ---------------------------------------------------------------------------

fn map_anthropic_tool_choice(tc: &ToolChoice) -> Option<AnthropicToolChoice> {
    match tc {
        ToolChoice::Auto => Some(AnthropicToolChoice::Auto),
        ToolChoice::Any => Some(AnthropicToolChoice::Any),
        ToolChoice::None => None,
        ToolChoice::Specific { name } => Some(AnthropicToolChoice::Tool { name: name.clone() }),
    }
}

// ---------------------------------------------------------------------------
// Helper: convert ContentPart to AnthropicContent
// ---------------------------------------------------------------------------

fn content_part_to_anthropic(part: &ContentPart) -> AnthropicContent {
    match part {
        ContentPart::Text { text } => AnthropicContent::Text { text: text.clone() },
        ContentPart::Media { media_type, data } => AnthropicContent::Image {
            source: AnthropicImageSource {
                source_type: "base64".to_string(),
                media_type: media_type.clone(),
                data: data.clone(),
            },
        },
        ContentPart::ToolCall { id, name, input } => AnthropicContent::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        },
        ContentPart::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => AnthropicContent::ToolResult {
            tool_use_id: tool_use_id.clone(),
            content: content.clone(),
            is_error: *is_error,
        },
        ContentPart::Reasoning { text } => AnthropicContent::Thinking {
            thinking: text.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// Protocol implementation
// ---------------------------------------------------------------------------

/// Anthropic Messages API protocol implementation.
///
/// Covers:
/// - Request body construction from `LlmRequest`
/// - SSE event stream decoding via `step()`
/// - Tool use, thinking, and text deltas
pub struct AnthropicMessagesProtocol;

#[async_trait]
impl Protocol for AnthropicMessagesProtocol {
    type Body = AnthropicBody;
    type Event = AnthropicEvent;
    type State = AnthropicState;

    fn body_from_request(&self, request: &LlmRequest) -> Result<(Self::Body, Self::State), String> {
        // --- model ---
        let model = request.model.id.clone();

        // --- system ---
        let system = request.system.clone();

        // --- messages ---
        let messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .map(|msg| AnthropicMessage {
                role: msg.role.clone(),
                content: msg.content.iter().map(content_part_to_anthropic).collect(),
            })
            .collect();

        // --- tools ---
        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| AnthropicTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.input_schema.clone(),
                })
                .collect()
        });

        // --- tool_choice ---
        let tool_choice = request
            .tool_choice
            .as_ref()
            .and_then(map_anthropic_tool_choice);

        // --- generation params ---
        let max_tokens = request
            .generation_params
            .as_ref()
            .and_then(|p| p.max_tokens)
            .unwrap_or(4096);

        let temperature = request
            .generation_params
            .as_ref()
            .and_then(|p| p.temperature);

        // --- stream ---
        let stream = request.stream;

        let body = AnthropicBody {
            model,
            system,
            messages,
            tools,
            tool_choice,
            max_tokens,
            temperature,
            stream,
        };

        let state = AnthropicState::default();

        Ok((body, state))
    }

    async fn step(&self, state: &mut Self::State, chunk: Option<&[u8]>) -> StepOutput<Self::Event> {
        // If already done, report it.
        if state.done {
            return StepOutput::Done {
                reason: None,
                usage: Some(state.accumulated_usage.clone()),
            };
        }

        // Append incoming data to the buffer.
        if let Some(data) = chunk {
            let text = std::str::from_utf8(data).unwrap_or("");
            state.buffer.push_str(text);
        }

        // Try to extract complete SSE frames from the buffer.
        let mut events: Vec<AnthropicEvent> = Vec::new();

        loop {
            // Find the next double-newline that delimits an SSE frame.
            let frame_end = state.buffer.find("\n\n").map(|pos| pos + 2);
            let frame_end = match frame_end {
                Some(pos) => pos,
                None => break, // need more data
            };

            let raw_frame = state.buffer[..frame_end].to_string();
            state.buffer.drain(..frame_end);

            let parsed = SseFrame::parse(raw_frame.as_bytes());
            let sse_event = match parsed {
                Some(f) => f,
                None => continue,
            };

            // Only process event type "completion" (Anthropic uses this type,
            // but also data-only frames). If there's no event type, still try.
            // Anthropic SSE frames have event: completion and then data: {...}.

            let event_name = sse_event.event.as_deref().unwrap_or("data");
            if event_name != "completion" && event_name != "data" && event_name != "ping" {
                continue;
            }

            let data_str = sse_event.data.as_str();

            // Try to parse as an AnthropicEvent.
            match serde_json::from_str::<AnthropicEvent>(data_str) {
                Ok(evt) => match &evt {
                    AnthropicEvent::MessageStart { _message } => {
                        state.started = true;
                        // Initialize usage from the message start info.
                        let usage = &_message.usage;
                        state.accumulated_usage.input_tokens = usage.input_tokens;
                        state.accumulated_usage.output_tokens = usage.output_tokens;
                        state.accumulated_usage.cache_read_input_tokens =
                            usage.cache_read_input_tokens.unwrap_or(0);
                        state.accumulated_usage.cache_creation_input_tokens =
                            usage.cache_creation_input_tokens.unwrap_or(0);
                        state.accumulated_usage.total_tokens =
                            usage.input_tokens + usage.output_tokens;
                        events.push(evt);
                    }
                    AnthropicEvent::ContentBlockStart {
                        index: _,
                        content_block,
                    } => {
                        // Track pending state for eventual stop.
                        match content_block {
                            AnthropicContentBlockStart::ToolUse { id, name, .. } => {
                                state.pending_tool_json = Some((id.clone(), name.clone()));
                            }
                            AnthropicContentBlockStart::Thinking { .. } => {
                                state.pending_thinking = true;
                            }
                            AnthropicContentBlockStart::Text { .. } => {}
                        }
                        events.push(evt);
                    }
                    AnthropicEvent::ContentBlockDelta { index: _, delta } => {
                        // Accumulate tool call input JSON across deltas.
                        if let AnthropicContentDelta::InputJsonDelta { partial_json } = delta
                            && let Some((ref _id, ref mut json_acc)) = state.pending_tool_json
                        {
                            json_acc.push_str(partial_json);
                        }
                        events.push(evt);
                    }
                    AnthropicEvent::ContentBlockStop { index: _ } => {
                        // Finalize any pending tool call.
                        if let Some((_id, _name)) = state.pending_tool_json.take() {
                            // The tool call start was already emitted; we just
                            // emit the raw event (the upstream translator will
                            // see the ContentBlockStart + deltas + ContentBlockStop).
                        }
                        state.pending_thinking = false;
                        events.push(evt);
                    }
                    AnthropicEvent::MessageDelta { delta: _, usage } => {
                        // Merge delta usage into accumulated usage.
                        state.accumulated_usage.output_tokens = state
                            .accumulated_usage
                            .output_tokens
                            .max(usage.output_tokens);
                        state.accumulated_usage.cache_read_input_tokens =
                            usage.cache_read_input_tokens.unwrap_or(0);
                        state.accumulated_usage.cache_creation_input_tokens =
                            usage.cache_creation_input_tokens.unwrap_or(0);
                        state.accumulated_usage.total_tokens = state
                            .accumulated_usage
                            .input_tokens
                            .saturating_add(usage.output_tokens);
                        events.push(evt);
                    }
                    AnthropicEvent::MessageStop => {
                        state.done = true;
                        events.push(AnthropicEvent::MessageStop);
                    }
                    AnthropicEvent::Ping => {
                        // Ignore ping events.
                    }
                    AnthropicEvent::Error { .. } => {
                        events.push(evt);
                    }
                },
                Err(e) => {
                    // If we can't parse this SSE data line, log and continue.
                    // This can happen with partial/incomplete frames.
                    eprintln!(
                        "[anthropic_messages] failed to parse SSE data: {} raw={}",
                        e, data_str
                    );
                }
            }
        }

        if events.is_empty() && !state.done {
            return StepOutput::NeedMore;
        }

        StepOutput::Events(events)
    }
}

// ---------------------------------------------------------------------------
// Route factory
// ---------------------------------------------------------------------------

/// Build a fully-specified `PreparedRoute` for the Anthropic Messages API.
///
/// - Endpoint: `POST https://api.anthropic.com/v1/messages`
/// - Auth: `x-api-key` header (value provided in `auth` map under key `"api_key"`)
/// - Headers: `anthropic-version: 2023-06-01`
/// - Framing: SSE
/// - Transport: HTTP
pub fn route() -> PreparedRoute {
    let mut auth = HashMap::new();
    auth.insert("x-api-key".to_string(), "${ANTHROPIC_API_KEY}".to_string());

    PreparedRoute {
        id: "anthropic-messages".to_string(),
        provider: jcode_llm_core::schema::ModelRef {
            provider_id: "anthropic".into(),
            id: String::new(), // caller should fill this in
            variant: None,
        },
        protocol: "anthropic-messages-2023-01-01".to_string(),
        endpoint: Endpoint {
            base_url: "https://api.anthropic.com".to_string(),
            path: PathSpec::Static("/v1/messages".to_string()),
            query: None,
        },
        auth,
        framing: Framing::Sse,
        transport: Transport::Http,
        defaults: HashMap::new(),
        body_overlay: Some(serde_json::json!({
            "anthropic_version": "2023-06-01"
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_llm_core::schema::*;

    #[test]
    fn test_body_from_request_basic() {
        let protocol = AnthropicMessagesProtocol;
        let req = LlmRequest {
            model: ModelRef::parse("anthropic/claude-sonnet-4-20250514").unwrap(),
            messages: vec![Message {
                role: "user".into(),
                content: vec![ContentPart::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("Be helpful.".into()),
            tools: None,
            tool_choice: None,
            generation_params: Some(GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(512),
                stop_sequences: None,
                top_p: None,
                top_k: None,
                presence_penalty: None,
                frequency_penalty: None,
                seed: None,
            }),
            stream: true,
            route_id: None,
        };

        let (body, _state) = protocol.body_from_request(&req).unwrap();
        assert_eq!(body.model, "claude-sonnet-4-20250514");
        assert_eq!(body.system.as_deref(), Some("Be helpful."));
        assert_eq!(body.messages.len(), 1);
        assert_eq!(body.messages[0].role, "user");
        assert!(body.stream);
        assert_eq!(body.max_tokens, 512);
        assert_eq!(body.temperature, Some(0.7));
    }

    #[test]
    fn test_body_from_request_with_tools() {
        let protocol = AnthropicMessagesProtocol;
        let req = LlmRequest {
            model: ModelRef::parse("anthropic/claude-sonnet-4-20250514").unwrap(),
            messages: vec![Message {
                role: "user".into(),
                content: vec![ContentPart::Text {
                    text: "Use a tool".into(),
                }],
            }],
            system: None,
            tools: Some(vec![ToolDefinition {
                name: "search".into(),
                description: "Search the web".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }),
            }]),
            tool_choice: Some(ToolChoice::Specific {
                name: "search".into(),
            }),
            generation_params: Some(GenerationParams {
                max_tokens: Some(4096),
                stop_sequences: None,
                top_p: None,
                top_k: None,
                presence_penalty: None,
                frequency_penalty: None,
                seed: None,
                temperature: None,
            }),
            stream: false,
            route_id: None,
        };

        let (body, _state) = protocol.body_from_request(&req).unwrap();
        assert_eq!(body.model, "claude-sonnet-4-20250514");
        assert!(body.tools.is_some());
        assert_eq!(body.tools.as_ref().unwrap().len(), 1);
        assert_eq!(body.tools.as_ref().unwrap()[0].name, "search");
        match body.tool_choice.as_ref().unwrap() {
            AnthropicToolChoice::Tool { name } => assert_eq!(name, "search"),
            _ => panic!("expected Tool variant"),
        }
        assert!(!body.stream);
        assert_eq!(body.max_tokens, 4096);
    }

    #[test]
    fn test_map_tool_choice() {
        assert!(map_anthropic_tool_choice(&ToolChoice::Auto).is_some());
        assert!(map_anthropic_tool_choice(&ToolChoice::Any).is_some());
        assert!(map_anthropic_tool_choice(&ToolChoice::None).is_none());
        let specific = map_anthropic_tool_choice(&ToolChoice::Specific { name: "foo".into() });
        assert!(matches!(specific, Some(AnthropicToolChoice::Tool { .. })));
    }

    #[tokio::test]
    async fn test_step_message_start() {
        let protocol = AnthropicMessagesProtocol;
        let mut state = AnthropicState::default();

        // Simulate an SSE frame with message_start
        let chunk = "event: completion\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_123\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet-4-20250514\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n";

        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert_eq!(evts.len(), 1);
                assert!(matches!(evts[0], AnthropicEvent::MessageStart { .. }));
                assert_eq!(state.accumulated_usage.input_tokens, 10);
                assert!(state.started);
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_step_text_delta() {
        let protocol = AnthropicMessagesProtocol;
        let mut state = AnthropicState::default();

        // Simulate a text content block start + delta + block stop + message stop
        let frames = concat!(
            "event: completion\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n",
            "\n",
            "event: completion\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n",
            "\n",
            "event: completion\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
            "\n",
            "event: completion\n",
            "data: {\"type\":\"message_stop\"}\n",
            "\n",
        );

        // Feed all at once
        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;

        // We should get all parsed events (start + delta + stop + message_stop)
        match result {
            StepOutput::Events(evts) => {
                assert!(!evts.is_empty(), "expected at least one event");
                // First event should be content_block_start
                assert!(matches!(evts[0], AnthropicEvent::ContentBlockStart { .. }));
            }
            other => panic!("expected Events, got {:?}", other),
        }

        // After consuming all events, state should be done
        assert!(state.done, "state should be done after message_stop");
    }

    #[tokio::test]
    async fn test_step_tool_use() {
        let protocol = AnthropicMessagesProtocol;
        let mut state = AnthropicState::default();

        // Simulate a tool use block with deltas
        let frames = concat!(
            "event: completion\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"search\",\"input\":{}}}\n",
            "\n",
            "event: completion\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"hello\\\"}\"}}\n",
            "\n",
            "event: completion\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
            "\n",
            "event: completion\n",
            "data: {\"type\":\"message_stop\"}\n",
            "\n",
        );

        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert_eq!(evts.len(), 4);
                assert!(matches!(evts[0], AnthropicEvent::ContentBlockStart { .. }));
                assert!(matches!(evts[1], AnthropicEvent::ContentBlockDelta { .. }));
                assert!(matches!(evts[2], AnthropicEvent::ContentBlockStop { .. }));
                assert!(matches!(evts[3], AnthropicEvent::MessageStop));
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_step_error() {
        let protocol = AnthropicMessagesProtocol;
        let mut state = AnthropicState::default();

        let chunk = "event: completion\ndata: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Bad request\"}}\n\n";

        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert!(matches!(evts[0], AnthropicEvent::Error { .. }));
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_step_need_more() {
        let protocol = AnthropicMessagesProtocol;
        let mut state = AnthropicState::default();

        // Incomplete frame — no double-newline
        let chunk = "event: completion\ndata: {\"type\":\"message_start\"";
        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        assert!(matches!(result, StepOutput::NeedMore));
    }

    #[test]
    fn test_content_part_conversion() {
        let text = ContentPart::Text { text: "hi".into() };
        let anthropic = content_part_to_anthropic(&text);
        assert!(matches!(anthropic, AnthropicContent::Text { .. }));

        let tc = ContentPart::ToolCall {
            id: "tc1".into(),
            name: "search".into(),
            input: serde_json::json!({"q": "test"}),
        };
        let anthropic = content_part_to_anthropic(&tc);
        assert!(matches!(anthropic, AnthropicContent::ToolUse { .. }));

        let tr = ContentPart::ToolResult {
            tool_use_id: "tc1".into(),
            content: "result".into(),
            is_error: None,
        };
        let anthropic = content_part_to_anthropic(&tr);
        assert!(matches!(anthropic, AnthropicContent::ToolResult { .. }));
    }

    #[test]
    fn test_route_has_correct_values() {
        let r = route();
        assert_eq!(r.protocol, "anthropic-messages-2023-01-01");
        assert_eq!(r.endpoint.base_url, "https://api.anthropic.com");
        assert_eq!(r.framing, Framing::Sse);
        assert_eq!(r.transport, Transport::Http);
        assert!(r.auth.contains_key("x-api-key"));
        assert!(r.body_overlay.is_some());
    }
}
