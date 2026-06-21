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
// Request body types (OpenAI Chat Completions API JSON shape)
// ---------------------------------------------------------------------------

/// The top-level request body sent to POST /v1/chat/completions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatBody {
    pub model: String,
    pub messages: Vec<OpenAIChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAIChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub stream: bool,
}

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAIChatMessage {
    /// User message with string content.
    UserString { role: String, content: String },
    /// User message with structured content parts.
    UserParts {
        role: String,
        content: Vec<OpenAIChatContentPart>,
    },
    /// Assistant message with optional tool_calls.
    Assistant {
        role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<OpenAIToolCall>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
    },
    /// System message.
    System { role: String, content: String },
    /// Tool result message.
    Tool {
        role: String,
        tool_call_id: String,
        content: String,
    },
}

impl OpenAIChatMessage {
    #[expect(dead_code)]
    fn role(&self) -> &str {
        match self {
            OpenAIChatMessage::UserString { role, .. } => role,
            OpenAIChatMessage::UserParts { role, .. } => role,
            OpenAIChatMessage::Assistant { role, .. } => role,
            OpenAIChatMessage::System { role, .. } => role,
            OpenAIChatMessage::Tool { role, .. } => role,
        }
    }
}

/// A content part inside a user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAIChatContentPart {
    Text { text: String },
    ImageUrl { image_url: OpenAIImageUrl },
}

/// Image URL reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A tool call in the assistant response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OpenAIFunctionCall,
}

/// A function call inside a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// A tool definition in OpenAI format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunctionDef,
}

/// Function definition inside a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

// ---------------------------------------------------------------------------
// SSE event types (delta variants)
// ---------------------------------------------------------------------------

/// Top-level chunk from the Chat Completions streaming API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    #[serde(default)]
    pub choices: Vec<OpenAIChatChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIChatUsage>,
}

/// A single choice in a streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatChoice {
    pub index: u64,
    pub delta: OpenAIChatDelta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// The delta content for a streaming choice.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAIChatDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIChatDeltaToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// A streaming delta tool call (may be partial).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatDeltaToolCall {
    pub index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<OpenAIChatDeltaFunction>,
}

/// Function delta inside a streaming tool call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAIChatDeltaFunction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

/// Usage reported in the final (non-empty) streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAIChatUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<OpenAIChatCompletionDetails>,
}

/// Breakdown of completion tokens (reasoning, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenAIChatCompletionDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// Protocol events for the Chat Completions API
// ---------------------------------------------------------------------------

/// Events emitted by the OpenAI Chat Completions SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAIChatEvent {
    /// First chunk received (contains role and possibly content).
    Start { id: String, model: String },
    /// Text content delta.
    TextDelta { delta: String },
    /// Reasoning content delta.
    ReasoningDelta { delta: String },
    /// A new tool call was initiated (index, id, name known).
    ToolCallStart {
        index: u64,
        id: String,
        name: String,
    },
    /// Tool call arguments delta.
    ToolCallArgumentsDelta { index: u64, delta: String },
    /// A tool call completed (arguments fully received).
    ToolCallEnd { index: u64, id: String },
    /// The final chunk with usage info and finish reason.
    Finish {
        finish_reason: Option<String>,
        usage: Option<OpenAIChatUsage>,
    },
    /// Error event.
    Error { message: String },
}

// ---------------------------------------------------------------------------
// Protocol state
// ---------------------------------------------------------------------------

/// Opaque state carried across `step()` calls for Chat Completions.
#[derive(Debug, Clone, Default)]
pub struct OpenAIChatState {
    /// Accumulated SSE data buffer.
    buffer: String,
    /// Accumulated usage.
    accumulated_usage: Option<Usage>,
    /// Whether we have seen the final chunk (stream done).
    done: bool,
    /// Whether we have emitted at least one event.
    started: bool,
    /// In-progress tool calls indexed by their streaming index.
    /// Maps index -> (id, name, accumulated_arguments).
    pending_tool_calls: HashMap<u64, (Option<String>, Option<String>, String)>,
}

// ---------------------------------------------------------------------------
// Helper: map ToolChoice to OpenAI tool_choice value
// ---------------------------------------------------------------------------

fn map_tool_choice(tc: &ToolChoice) -> Option<Value> {
    match tc {
        ToolChoice::Auto => Some(Value::String("auto".to_string())),
        ToolChoice::Any => Some(Value::String("required".to_string())),
        ToolChoice::None => None,
        ToolChoice::Specific { name } => Some(serde_json::json!({
            "type": "function",
            "function": { "name": name }
        })),
    }
}

// ---------------------------------------------------------------------------
// Helper: convert ContentPart to OpenAI chat content parts
// ---------------------------------------------------------------------------

fn content_part_to_openai_chat(part: &ContentPart) -> Option<OpenAIChatContentPart> {
    match part {
        ContentPart::Text { text } => Some(OpenAIChatContentPart::Text { text: text.clone() }),
        ContentPart::Media { media_type, data } => {
            let url = if data.starts_with("data:") {
                data.clone()
            } else {
                format!("data:{};base64,{}", media_type, data)
            };
            Some(OpenAIChatContentPart::ImageUrl {
                image_url: OpenAIImageUrl { url, detail: None },
            })
        }
        // ToolCall, ToolResult, Reasoning are handled at the message level,
        // not inside user content parts.
        ContentPart::ToolCall { .. }
        | ContentPart::ToolResult { .. }
        | ContentPart::Reasoning { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// Protocol implementation
// ---------------------------------------------------------------------------

/// OpenAI Chat Completions API protocol implementation.
///
/// Covers:
/// - Request body construction from `LlmRequest`
/// - SSE event stream decoding via `step()`
/// - Text, reasoning, and tool call deltas with parallel tool call accumulation
pub struct OpenAiChatProtocol;

#[async_trait]
impl Protocol for OpenAiChatProtocol {
    type Body = OpenAIChatBody;
    type Event = OpenAIChatEvent;
    type State = OpenAIChatState;

    fn body_from_request(&self, request: &LlmRequest) -> Result<(Self::Body, Self::State), String> {
        // --- model ---
        let model = request.model.id.clone();

        // --- messages ---
        let messages: Vec<OpenAIChatMessage> = request
            .messages
            .iter()
            .map(|msg| {
                let role = msg.role.clone();

                // Check if there are any tool calls or tool results in the content.
                let _has_tool_calls = msg
                    .content
                    .iter()
                    .any(|p| matches!(p, ContentPart::ToolCall { .. }));
                let has_tool_results = msg
                    .content
                    .iter()
                    .any(|p| matches!(p, ContentPart::ToolResult { .. }));

                if role == "assistant" {
                    // Build tool_calls from ToolCall content parts.
                    let tool_calls: Vec<OpenAIToolCall> = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::ToolCall { id, name, input } => Some(OpenAIToolCall {
                                id: id.clone(),
                                call_type: "function".to_string(),
                                function: OpenAIFunctionCall {
                                    name: name.clone(),
                                    arguments: serde_json::to_string(input).unwrap_or_default(),
                                },
                            }),
                            _ => None,
                        })
                        .collect();

                    // Extract text content (skipping reasoning parts for the text string).
                    let text_content: String = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    let content = if text_content.is_empty() && !tool_calls.is_empty() {
                        None
                    } else {
                        Some(text_content)
                    };

                    return OpenAIChatMessage::Assistant {
                        role,
                        content,
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls)
                        },
                        reasoning_content: None,
                    };
                }

                if has_tool_results {
                    // Tool messages: each ToolResult becomes a separate tool message.
                    let tool_part = msg.content.iter().find_map(|p| match p {
                        ContentPart::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => Some((tool_use_id, content, is_error)),
                        _ => None,
                    });

                    if let Some((tool_use_id, content, _is_error)) = tool_part {
                        return OpenAIChatMessage::Tool {
                            role,
                            tool_call_id: tool_use_id.clone(),
                            content: content.clone(),
                        };
                    }
                }

                if role == "system" {
                    let text: String = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    return OpenAIChatMessage::System {
                        role,
                        content: text,
                    };
                }

                // Default: build content parts.
                let text_parts: Vec<&str> = msg
                    .content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();

                let media_parts: Vec<&ContentPart> = msg
                    .content
                    .iter()
                    .filter(|p| matches!(p, ContentPart::Media { .. }))
                    .collect();

                if media_parts.is_empty() {
                    // Simple string content.
                    OpenAIChatMessage::UserString {
                        role,
                        content: text_parts.join(""),
                    }
                } else {
                    // Structured content with images.
                    let mut parts: Vec<OpenAIChatContentPart> = Vec::new();
                    for part in &msg.content {
                        if let Some(converted) = content_part_to_openai_chat(part) {
                            parts.push(converted);
                        }
                    }
                    OpenAIChatMessage::UserParts {
                        role,
                        content: parts,
                    }
                }
            })
            .collect();

        // --- tools ---
        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| OpenAIChatTool {
                    tool_type: "function".to_string(),
                    function: OpenAIFunctionDef {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.input_schema.clone(),
                        strict: None,
                    },
                })
                .collect()
        });

        // --- tool_choice ---
        let tool_choice = request.tool_choice.as_ref().and_then(map_tool_choice);

        // --- generation params ---
        let max_tokens = request
            .generation_params
            .as_ref()
            .and_then(|p| p.max_tokens);

        let temperature = request
            .generation_params
            .as_ref()
            .and_then(|p| p.temperature);

        // --- stream ---
        let stream = request.stream;

        let body = OpenAIChatBody {
            model,
            messages,
            tools,
            tool_choice,
            max_tokens,
            temperature,
            stream,
        };

        let state = OpenAIChatState::default();

        Ok((body, state))
    }

    async fn step(&self, state: &mut Self::State, chunk: Option<&[u8]>) -> StepOutput<Self::Event> {
        // If already done, report it.
        if state.done {
            let usage = state.accumulated_usage.clone();
            return StepOutput::Done {
                reason: None,
                usage,
            };
        }

        // Append incoming data to the buffer.
        if let Some(data) = chunk {
            let text = std::str::from_utf8(data).unwrap_or("");
            state.buffer.push_str(text);
        }

        // Try to extract complete SSE frames from the buffer.
        let mut events: Vec<OpenAIChatEvent> = Vec::new();

        while let Some(pos) = state.buffer.find("\n\n").map(|pos| pos + 2) {
            // Find the next double-newline that delimits an SSE frame.
            let frame_end = pos;

            let raw_frame = state.buffer[..frame_end].to_string();
            state.buffer.drain(..frame_end);

            let parsed = SseFrame::parse(raw_frame.as_bytes());
            let sse_event = match parsed {
                Some(f) => f,
                None => continue,
            };

            let data_str = sse_event.data.as_str();

            // Check for the [DONE] sentinel.
            if data_str.trim() == "[DONE]" {
                state.done = true;
                events.push(OpenAIChatEvent::Finish {
                    finish_reason: None,
                    usage: None,
                });
                break;
            }

            // Try to parse as a Chat Completions chunk.
            let chunk_data: OpenAIChatChunk = match serde_json::from_str(data_str) {
                Ok(c) => c,
                Err(_) => {
                    // Skip unparseable lines (e.g. empty data frames).
                    continue;
                }
            };

            // Emit start event on first chunk.
            if !state.started {
                state.started = true;
                events.push(OpenAIChatEvent::Start {
                    id: chunk_data.id.clone(),
                    model: chunk_data.model.clone(),
                });
            }

            // Process each choice.
            for choice in &chunk_data.choices {
                let delta = &choice.delta;

                // Text content delta.
                if let Some(text) = &delta.content
                    && !text.is_empty()
                {
                    events.push(OpenAIChatEvent::TextDelta {
                        delta: text.clone(),
                    });
                }

                // Reasoning content delta (non-standard, used by some providers).
                if let Some(reasoning) = &delta.reasoning_content
                    && !reasoning.is_empty()
                {
                    events.push(OpenAIChatEvent::ReasoningDelta {
                        delta: reasoning.clone(),
                    });
                }

                // Tool call deltas (may be multiple in one chunk, indexed).
                if let Some(tool_call_deltas) = &delta.tool_calls {
                    for tc_delta in tool_call_deltas {
                        let idx = tc_delta.index;

                        // Get or create the pending tool call entry.
                        let entry = state
                            .pending_tool_calls
                            .entry(idx)
                            .or_insert_with(|| (None, None, String::new()));

                        // If this delta has an id, it's the start of a tool call.
                        if let Some(tc_id) = &tc_delta.id {
                            let was_new = entry.0.is_none();
                            entry.0 = Some(tc_id.clone());
                            if was_new {
                                // Emit tool call start; name may come in a later chunk.
                                events.push(OpenAIChatEvent::ToolCallStart {
                                    index: idx,
                                    id: tc_id.clone(),
                                    name: String::new(),
                                });
                            }
                        }

                        // If this delta has a name, update it.
                        if let Some(func) = &tc_delta.function {
                            if let Some(name) = &func.name {
                                entry.1 = Some(name.clone());
                                // Re-send start with name if we already emitted without one.
                                if let Some(ref tc_id) = entry.0 {
                                    events.push(OpenAIChatEvent::ToolCallStart {
                                        index: idx,
                                        id: tc_id.clone(),
                                        name: name.clone(),
                                    });
                                }
                            }

                            // Accumulate arguments.
                            if let Some(args_delta) = &func.arguments
                                && !args_delta.is_empty()
                            {
                                entry.2.push_str(args_delta);
                                events.push(OpenAIChatEvent::ToolCallArgumentsDelta {
                                    index: idx,
                                    delta: args_delta.clone(),
                                });
                            }
                        }
                    }
                }

                // Check finish reason — this signals the end of this choice.
                if let Some(finish) = &choice.finish_reason {
                    // Finalize any pending tool calls before finish.
                    let mut finalized_indices: Vec<u64> = Vec::new();
                    for (&idx, (id, _, _)) in &state.pending_tool_calls {
                        if let Some(tc_id) = id {
                            events.push(OpenAIChatEvent::ToolCallEnd {
                                index: idx,
                                id: tc_id.clone(),
                            });
                        }
                        finalized_indices.push(idx);
                    }
                    for idx in finalized_indices {
                        state.pending_tool_calls.remove(&idx);
                    }

                    // Build usage from chunk usage or estimate.
                    let chat_usage = chunk_data.usage.clone();
                    let core_usage = chat_usage.as_ref().map(|u| Usage {
                        input_tokens: u.prompt_tokens,
                        output_tokens: u.completion_tokens,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                        total_tokens: u.total_tokens,
                        breakdown: u.completion_tokens_details.as_ref().map(|d| {
                            jcode_llm_core::schema::UsageBreakdown {
                                audio_input_tokens: None,
                                reasoning_tokens: d.reasoning_tokens,
                            }
                        }),
                    });

                    if let Some(ref u) = core_usage {
                        state.accumulated_usage = Some(u.clone());
                    }

                    state.done = true;
                    events.push(OpenAIChatEvent::Finish {
                        finish_reason: Some(finish.clone()),
                        usage: chat_usage,
                    });
                }
            }

            if state.done {
                break;
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

/// Build a fully-specified `PreparedRoute` for the OpenAI Chat Completions API.
///
/// - Endpoint: `POST https://api.openai.com/v1/chat/completions`
/// - Auth: `Authorization: Bearer` header (value from `auth` map key `"api_key"`)
/// - Framing: SSE
/// - Transport: HTTP
pub fn chat_route() -> PreparedRoute {
    let mut auth = HashMap::new();
    auth.insert(
        "Authorization".to_string(),
        "Bearer ${OPENAI_API_KEY}".to_string(),
    );

    PreparedRoute {
        id: "openai-chat".to_string(),
        provider: jcode_llm_core::schema::ModelRef {
            provider_id: "openai".into(),
            id: String::new(),
            variant: None,
        },
        protocol: "openai-chat-2024-01-01".to_string(),
        endpoint: Endpoint {
            base_url: "https://api.openai.com".to_string(),
            path: PathSpec::Static("/v1/chat/completions".to_string()),
            query: None,
        },
        auth,
        framing: Framing::Sse,
        transport: Transport::Http,
        defaults: HashMap::new(),
        body_overlay: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_llm_core::schema::*;

    #[test]
    fn test_body_from_request_basic() {
        let protocol = OpenAiChatProtocol;
        let req = LlmRequest {
            model: ModelRef::parse("openai/gpt-4o").unwrap(),
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
        assert_eq!(body.model, "gpt-4o");
        assert_eq!(body.messages.len(), 1);
        assert_eq!(body.messages[0].role(), "user");
        assert!(body.stream);
        assert_eq!(body.max_tokens, Some(512));
        assert_eq!(body.temperature, Some(0.7));
    }

    #[test]
    fn test_body_from_request_with_tools() {
        let protocol = OpenAiChatProtocol;
        let req = LlmRequest {
            model: ModelRef::parse("openai/gpt-4o").unwrap(),
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
        assert_eq!(body.model, "gpt-4o");
        assert!(body.tools.is_some());
        assert_eq!(body.tools.as_ref().unwrap().len(), 1);
        assert_eq!(body.tools.as_ref().unwrap()[0].function.name, "search");
        assert!(body.tool_choice.is_some());
        assert!(!body.stream);
        assert_eq!(body.max_tokens, Some(4096));
    }

    #[test]
    fn test_body_from_request_with_assistant_tool_calls() {
        let protocol = OpenAiChatProtocol;
        let req = LlmRequest {
            model: ModelRef::parse("openai/gpt-4o").unwrap(),
            messages: vec![
                Message {
                    role: "user".into(),
                    content: vec![ContentPart::Text {
                        text: "What is the weather?".into(),
                    }],
                },
                Message {
                    role: "assistant".into(),
                    content: vec![ContentPart::ToolCall {
                        id: "call_123".into(),
                        name: "get_weather".into(),
                        input: serde_json::json!({"location": "San Francisco"}),
                    }],
                },
                Message {
                    role: "user".into(),
                    content: vec![ContentPart::ToolResult {
                        tool_use_id: "call_123".into(),
                        content: "Sunny".into(),
                        is_error: None,
                    }],
                },
            ],
            system: None,
            tools: None,
            tool_choice: None,
            generation_params: None,
            stream: false,
            route_id: None,
        };

        let (body, _state) = protocol.body_from_request(&req).unwrap();
        assert_eq!(body.messages.len(), 3);

        // Check assistant message has tool_calls.
        match &body.messages[1] {
            OpenAIChatMessage::Assistant {
                tool_calls,
                content,
                ..
            } => {
                assert!(tool_calls.is_some());
                assert_eq!(tool_calls.as_ref().unwrap().len(), 1);
                assert_eq!(tool_calls.as_ref().unwrap()[0].id, "call_123");
                assert_eq!(tool_calls.as_ref().unwrap()[0].function.name, "get_weather");
                assert!(content.is_none());
            }
            other => panic!("expected Assistant message, got {:?}", other),
        }

        // Check tool result message.
        match &body.messages[2] {
            OpenAIChatMessage::Tool {
                tool_call_id,
                content,
                ..
            } => {
                assert_eq!(tool_call_id, "call_123");
                assert_eq!(content, "Sunny");
            }
            other => panic!("expected Tool message, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_step_text_delta() {
        let protocol = OpenAiChatProtocol;
        let mut state = OpenAIChatState::default();

        let frames = concat!(
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello\"}}]}\n",
            "\n",
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"}}]}\n",
            "\n",
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n",
            "\n",
        );

        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert!(!evts.is_empty(), "expected at least one event");

                // First event should be Start.
                let start = evts
                    .iter()
                    .find(|e| matches!(e, OpenAIChatEvent::Start { .. }));
                assert!(start.is_some(), "expected Start event");

                // Should have text deltas.
                let text_deltas: Vec<&OpenAIChatEvent> = evts
                    .iter()
                    .filter(|e| matches!(e, OpenAIChatEvent::TextDelta { .. }))
                    .collect();
                assert!(!text_deltas.is_empty(), "expected TextDelta events");

                // Should finish.
                let finish = evts
                    .iter()
                    .find(|e| matches!(e, OpenAIChatEvent::Finish { .. }));
                assert!(finish.is_some(), "expected Finish event");
            }
            other => panic!("expected Events, got {:?}", other),
        }

        assert!(state.done, "state should be done");
    }

    #[tokio::test]
    async fn test_step_tool_call_streaming() {
        let protocol = OpenAiChatProtocol;
        let mut state = OpenAIChatState::default();

        let frames = concat!(
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null,\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"search\",\"arguments\":\"\"}}]}}]}\n",
            "\n",
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"q\\\":\\\"hello\\\"}\"}}]}}]}\n",
            "\n",
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":8,\"total_tokens\":18}}\n",
            "\n",
        );

        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert!(!evts.is_empty(), "expected at least one event");

                let tool_starts: Vec<&OpenAIChatEvent> = evts
                    .iter()
                    .filter(|e| matches!(e, OpenAIChatEvent::ToolCallStart { .. }))
                    .collect();
                assert!(!tool_starts.is_empty(), "expected ToolCallStart events");

                let tool_args: Vec<&OpenAIChatEvent> = evts
                    .iter()
                    .filter(|e| matches!(e, OpenAIChatEvent::ToolCallArgumentsDelta { .. }))
                    .collect();
                assert!(
                    !tool_args.is_empty(),
                    "expected ToolCallArgumentsDelta events"
                );

                let tool_ends: Vec<&OpenAIChatEvent> = evts
                    .iter()
                    .filter(|e| matches!(e, OpenAIChatEvent::ToolCallEnd { .. }))
                    .collect();
                assert!(!tool_ends.is_empty(), "expected ToolCallEnd events");

                let finish = evts
                    .iter()
                    .find(|e| matches!(e, OpenAIChatEvent::Finish { .. }));
                assert!(finish.is_some(), "expected Finish event");
            }
            other => panic!("expected Events, got {:?}", other),
        }

        assert!(state.done, "state should be done");
    }

    #[tokio::test]
    async fn test_step_need_more() {
        let protocol = OpenAiChatProtocol;
        let mut state = OpenAIChatState::default();

        // Incomplete frame — no double-newline.
        let chunk = "data: {\"id\":\"chatcmpl_123\"";
        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        assert!(matches!(result, StepOutput::NeedMore));
    }

    #[tokio::test]
    async fn test_step_done_sentinel() {
        let protocol = OpenAiChatProtocol;
        let mut state = OpenAIChatState::default();

        let chunk = "data: [DONE]\n\n";
        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert_eq!(evts.len(), 1);
                assert!(matches!(evts[0], OpenAIChatEvent::Finish { .. }));
            }
            other => panic!("expected Events, got {:?}", other),
        }
        assert!(state.done);
    }

    #[tokio::test]
    async fn test_step_parallel_tool_calls() {
        let protocol = OpenAiChatProtocol;
        let mut state = OpenAIChatState::default();

        let frames = concat!(
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null,\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"search\",\"arguments\":\"\"}},{\"index\":1,\"id\":\"call_2\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"\"}}]}}]}\n",
            "\n",
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"q\\\":\\\"test\\\"}\"}},{\"index\":1,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"/tmp\\\"}\"}}]}}]}\n",
            "\n",
            "data: {\"id\":\"chatcmpl_123\",\"object\":\"chat.completion.chunk\",\"created\":1700000000,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n",
            "\n",
        );

        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                let tool_starts: Vec<&OpenAIChatEvent> = evts
                    .iter()
                    .filter(|e| matches!(e, OpenAIChatEvent::ToolCallStart { .. }))
                    .collect();
                // Expect at least 2 tool call starts (one per parallel call).
                assert!(
                    tool_starts.len() >= 2,
                    "expected at least 2 ToolCallStart events, got {}",
                    tool_starts.len()
                );

                let tool_ends: Vec<&OpenAIChatEvent> = evts
                    .iter()
                    .filter(|e| matches!(e, OpenAIChatEvent::ToolCallEnd { .. }))
                    .collect();
                // Expect at least 2 tool call ends.
                assert!(
                    tool_ends.len() >= 2,
                    "expected at least 2 ToolCallEnd events, got {}",
                    tool_ends.len()
                );
            }
            other => panic!("expected Events, got {:?}", other),
        }

        assert!(state.done);
    }

    #[test]
    fn test_map_tool_choice() {
        assert_eq!(
            map_tool_choice(&ToolChoice::Auto),
            Some(Value::String("auto".to_string()))
        );
        assert_eq!(
            map_tool_choice(&ToolChoice::Any),
            Some(Value::String("required".to_string()))
        );
        assert!(map_tool_choice(&ToolChoice::None).is_none());
        let specific = map_tool_choice(&ToolChoice::Specific { name: "foo".into() });
        assert!(specific.is_some());
        assert_eq!(
            specific
                .unwrap()
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str()),
            Some("foo")
        );
    }

    #[test]
    fn test_chat_route_has_correct_values() {
        let r = chat_route();
        assert_eq!(r.protocol, "openai-chat-2024-01-01");
        assert_eq!(r.endpoint.base_url, "https://api.openai.com");
        assert_eq!(r.framing, Framing::Sse);
        assert_eq!(r.transport, Transport::Http);
        assert!(r.auth.contains_key("Authorization"));
    }
}
