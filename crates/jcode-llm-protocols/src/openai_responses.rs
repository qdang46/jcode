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
// Request body types (OpenAI Responses API JSON shape)
// ---------------------------------------------------------------------------

/// An input item in the Responses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseInputItem {
    /// A message input (developer, user, assistant, system).
    Message {
        role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Vec<ResponseContentPart>>,
    },
    /// A function call made by the model.
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    /// A function call output (tool result).
    FunctionCallOutput { call_id: String, output: String },
    /// Reasoning item from a previous response.
    Reasoning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<Vec<Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    /// A compaction item to compress the conversation history.
    Compaction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },
}

/// A content part within a message input item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseContentPart {
    /// Input text.
    InputText { text: String },
    /// Input image (base64 data URL).
    InputImage {
        image_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

/// The top-level request body sent to POST /v1/responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIResponsesBody {
    pub model: String,
    pub input: Vec<ResponseInputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAIResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<OpenAIReasoningConfig>,
}

/// Configuration for model reasoning (thinking).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIReasoningConfig {
    #[serde(rename = "type")]
    pub config_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// A tool definition in OpenAI Responses format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIResponsesTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<OpenAIResponsesFunctionDef>,
    /// User ID for web_search tool (provider-executed tool).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_location: Option<Value>,
    /// Search context size for web_search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_context_size: Option<String>,
}

/// Function definition for `type: "function"` tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIResponsesFunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

// ---------------------------------------------------------------------------
// SSE event types for the Responses API
// ---------------------------------------------------------------------------

/// Events produced by the OpenAI Responses SSE stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OpenAIResponsesEvent {
    /// Response object created.
    #[serde(rename = "response.created")]
    ResponseCreated { response: Value },
    /// Text output delta.
    #[serde(rename = "response.output_text.delta")]
    ResponseOutputTextDelta {
        delta: String,
        item_id: String,
        output_index: u64,
        content_index: u64,
    },
    /// Reasoning delta.
    #[serde(rename = "response.reasoning.delta")]
    ResponseReasoningDelta {
        delta: String,
        item_id: String,
        output_index: u64,
    },
    /// A new output item was added (function_call, message, reasoning, etc.).
    #[serde(rename = "response.output_item.added")]
    ResponseOutputItemAdded { item: Value, output_index: u64 },
    /// Function call arguments delta.
    #[serde(rename = "response.function_call_arguments.delta")]
    ResponseFunctionCallArgumentsDelta {
        item_id: String,
        output_index: u64,
        call_id: String,
        delta: String,
    },
    /// Function call arguments done.
    #[serde(rename = "response.function_call_arguments.done")]
    ResponseFunctionCallArgumentsDone {
        item_id: String,
        output_index: u64,
        call_id: String,
        name: String,
        arguments: String,
    },
    /// An output item is done.
    #[serde(rename = "response.output_item.done")]
    ResponseOutputItemDone { item: Value, output_index: u64 },
    /// Response completed.
    #[serde(rename = "response.completed")]
    ResponseCompleted { response: Value },
    /// Response incomplete.
    #[serde(rename = "response.incomplete")]
    ResponseIncomplete { response: Value },
    /// Response failed.
    #[serde(rename = "response.failed")]
    ResponseFailed { response: Value, error: Value },
    /// Error event.
    #[serde(rename = "error")]
    Error { error: Value },
}

// ---------------------------------------------------------------------------
// Protocol state
// ---------------------------------------------------------------------------

/// Opaque state carried across `step()` calls for Responses API.
#[derive(Debug, Clone, Default)]
pub struct OpenAIResponsesState {
    /// Accumulated SSE data buffer.
    buffer: String,
    /// Accumulated usage.
    accumulated_usage: Option<Usage>,
    /// Whether we have seen a terminal event.
    done: bool,
    /// Whether we have seen `response.created`.
    started: bool,
    /// Pending tool calls being accumulated.
    pending_tool_calls: HashMap<String, (u64, String, String, String)>,
}

// ---------------------------------------------------------------------------
// Helper: extract usage from a response value
// ---------------------------------------------------------------------------

fn extract_usage(response: &Value) -> Option<Usage> {
    let usage = response.get("usage")?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(input_tokens + output_tokens);

    let cache_read = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let reasoning_tokens = usage
        .get("output_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|v| v.as_u64());

    Some(Usage {
        input_tokens,
        output_tokens,
        cache_read_input_tokens: cache_read,
        cache_creation_input_tokens: 0,
        total_tokens,
        breakdown: reasoning_tokens.map(|rt| jcode_llm_core::schema::UsageBreakdown {
            audio_input_tokens: None,
            reasoning_tokens: Some(rt),
        }),
    })
}

// ---------------------------------------------------------------------------
// Helper: map ToolChoice to OpenAI tool_choice value
// ---------------------------------------------------------------------------

fn map_tool_choice(tc: &ToolChoice) -> Option<Value> {
    match tc {
        ToolChoice::Auto => None,
        ToolChoice::Any => Some(Value::String("required".to_string())),
        ToolChoice::None => Some(Value::String("none".to_string())),
        ToolChoice::Specific { name } => Some(serde_json::json!({
            "type": "function",
            "name": name
        })),
    }
}

// ---------------------------------------------------------------------------
// Helper: convert ContentPart to ResponseInputItem variants
// ---------------------------------------------------------------------------

#[expect(dead_code)]
fn content_part_to_response_item(part: &ContentPart) -> Option<ResponseInputItem> {
    match part {
        ContentPart::Text { .. } => None, // handled at message level
        ContentPart::Media { media_type, data } => {
            let url = if data.starts_with("data:") {
                data.clone()
            } else {
                format!("data:{};base64,{}", media_type, data)
            };
            Some(ResponseInputItem::Message {
                role: "user".to_string(),
                content: Some(vec![ResponseContentPart::InputImage {
                    image_url: url,
                    detail: None,
                }]),
            })
        }
        ContentPart::ToolCall { id, name, input } => {
            let arguments = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
            Some(ResponseInputItem::FunctionCall {
                call_id: id.clone(),
                name: name.clone(),
                arguments,
            })
        }
        ContentPart::ToolResult {
            tool_use_id,
            content,
            is_error: _,
        } => Some(ResponseInputItem::FunctionCallOutput {
            call_id: tool_use_id.clone(),
            output: content.clone(),
        }),
        ContentPart::Reasoning { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// Protocol implementation
// ---------------------------------------------------------------------------

/// OpenAI Responses API protocol implementation.
///
/// Covers:
/// - Request body construction from `LlmRequest`
/// - SSE event stream decoding via `step()`
/// - Text, reasoning, and tool call deltas
/// - Provider-executed tools (e.g. web_search) via `tool_type`
pub struct OpenAiResponsesProtocol;

#[async_trait]
impl Protocol for OpenAiResponsesProtocol {
    type Body = OpenAIResponsesBody;
    type Event = OpenAIResponsesEvent;
    type State = OpenAIResponsesState;

    fn body_from_request(&self, request: &LlmRequest) -> Result<(Self::Body, Self::State), String> {
        // --- model ---
        let model = request.model.id.clone();

        // --- input items ---
        let mut input: Vec<ResponseInputItem> = Vec::new();

        for msg in &request.messages {
            let role = msg.role.as_str();

            match role {
                "assistant" => {
                    // Split into text content parts and tool calls.
                    let text_parts: String = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    if !text_parts.is_empty() {
                        input.push(ResponseInputItem::Message {
                            role: "assistant".to_string(),
                            content: Some(vec![ResponseContentPart::InputText {
                                text: text_parts,
                            }]),
                        });
                    }

                    // Function calls.
                    for part in &msg.content {
                        if let ContentPart::ToolCall {
                            id,
                            name,
                            input: args,
                        } = part
                        {
                            let arguments =
                                serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
                            input.push(ResponseInputItem::FunctionCall {
                                call_id: id.clone(),
                                name: name.clone(),
                                arguments,
                            });
                        }
                    }

                    // Reasoning parts.
                    for part in &msg.content {
                        if let ContentPart::Reasoning { text } = part {
                            input.push(ResponseInputItem::Reasoning {
                                id: None,
                                summary: Some(vec![
                                    serde_json::json!({"type": "summary_text", "text": text}),
                                ]),
                                encrypted_content: None,
                                status: None,
                            });
                        }
                    }
                }
                "user" => {
                    let text_parts: Vec<String> = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect();

                    let mut content_parts = Vec::new();
                    for part in &msg.content {
                        match part {
                            ContentPart::Text { text } => {
                                content_parts
                                    .push(ResponseContentPart::InputText { text: text.clone() });
                            }
                            ContentPart::Media { media_type, data } => {
                                let url = if data.starts_with("data:") {
                                    data.clone()
                                } else {
                                    format!("data:{};base64,{}", media_type, data)
                                };
                                content_parts.push(ResponseContentPart::InputImage {
                                    image_url: url,
                                    detail: None,
                                });
                            }
                            _ => {}
                        }
                    }

                    // Also handle tool results embedded in user messages.
                    for part in &msg.content {
                        if let ContentPart::ToolResult {
                            tool_use_id,
                            content,
                            is_error: _,
                        } = part
                        {
                            if !content_parts.is_empty() {
                                input.push(ResponseInputItem::Message {
                                    role: "user".to_string(),
                                    content: Some(content_parts.clone()),
                                });
                                content_parts.clear();
                            }
                            input.push(ResponseInputItem::FunctionCallOutput {
                                call_id: tool_use_id.clone(),
                                output: content.clone(),
                            });
                        }
                    }

                    if !content_parts.is_empty() || text_parts.is_empty() {
                        input.push(ResponseInputItem::Message {
                            role: "user".to_string(),
                            content: Some(content_parts),
                        });
                    }
                }
                "system" | "developer" => {
                    let text: String = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    input.push(ResponseInputItem::Message {
                        role: role.to_string(),
                        content: Some(vec![ResponseContentPart::InputText { text }]),
                    });
                }
                _ => {
                    // Fallback: treat as a text-only message of the given role.
                    let text: String = msg
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    input.push(ResponseInputItem::Message {
                        role: role.to_string(),
                        content: Some(vec![ResponseContentPart::InputText { text }]),
                    });
                }
            }
        }

        // --- tools ---
        let tools = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| OpenAIResponsesTool {
                    tool_type: "function".to_string(),
                    name: Some(t.name.clone()),
                    description: Some(t.description.clone()),
                    parameters: Some(t.input_schema.clone()),
                    strict: None,
                    function: Some(OpenAIResponsesFunctionDef {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.input_schema.clone(),
                        strict: None,
                    }),
                    user_location: None,
                    search_context_size: None,
                })
                .collect()
        });

        // --- tool_choice ---
        let tool_choice = request.tool_choice.as_ref().and_then(map_tool_choice);

        // --- generation params ---
        let max_output_tokens = request
            .generation_params
            .as_ref()
            .and_then(|p| p.max_tokens);

        let temperature = request
            .generation_params
            .as_ref()
            .and_then(|p| p.temperature);

        // --- stream ---
        let stream = request.stream;

        // --- reasoning ---
        let reasoning = None;

        let body = OpenAIResponsesBody {
            model,
            input,
            tools,
            tool_choice,
            max_output_tokens,
            temperature,
            stream,
            reasoning,
        };

        let state = OpenAIResponsesState::default();

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
        let mut events: Vec<OpenAIResponsesEvent> = Vec::new();

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

            // Try to parse as a Responses API event.
            let event: OpenAIResponsesEvent = match serde_json::from_str(data_str) {
                Ok(evt) => evt,
                Err(_) => {
                    // Skip unparseable lines.
                    continue;
                }
            };

            match &event {
                OpenAIResponsesEvent::ResponseCreated { response: _ } => {
                    state.started = true;
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseOutputTextDelta {
                    delta: _,
                    item_id: _,
                    output_index: _,
                    content_index: _,
                } => {
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseReasoningDelta {
                    delta: _,
                    item_id: _,
                    output_index: _,
                } => {
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseOutputItemAdded { item, output_index } => {
                    // If this item is a function_call or custom_tool_call,
                    // initialize a pending entry for argument accumulation.
                    if let Some(item_type) = item.get("type").and_then(|v| v.as_str())
                        && matches!(item_type, "function_call" | "custom_tool_call")
                        && let Some(item_id) = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("item_id").and_then(|v| v.as_str()))
                    {
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(item_id)
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let existing_arguments =
                            item.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                        state.pending_tool_calls.insert(
                            item_id.to_string(),
                            (*output_index, call_id, name, existing_arguments.to_string()),
                        );
                    }

                    // Mark provider-executed tools.
                    if let Some(item_type) = item.get("type").and_then(|v| v.as_str())
                        && (item_type == "web_search" || item_type == "user_location")
                    {
                        // Provider-executed tools: the provider handles these
                        // without needing a separate tool call from the model.
                        // We just pass the event through.
                    }
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseFunctionCallArgumentsDelta {
                    item_id,
                    output_index: _,
                    call_id: _,
                    delta,
                } => {
                    // Accumulate arguments.
                    if let Some(entry) = state.pending_tool_calls.get_mut(item_id) {
                        entry.3.push_str(delta);
                    }
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseFunctionCallArgumentsDone {
                    item_id,
                    output_index: _,
                    call_id: _,
                    name: _,
                    arguments: _,
                } => {
                    state.pending_tool_calls.remove(item_id);
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseOutputItemDone {
                    item: _,
                    output_index: _,
                } => {
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseCompleted { response } => {
                    if let Some(usage) = extract_usage(response) {
                        state.accumulated_usage = Some(usage);
                    }
                    state.done = true;
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseIncomplete { response } => {
                    if let Some(usage) = extract_usage(response) {
                        state.accumulated_usage = Some(usage);
                    }
                    state.done = true;
                    events.push(event);
                }
                OpenAIResponsesEvent::ResponseFailed { response, error: _ } => {
                    if let Some(usage) = extract_usage(response) {
                        state.accumulated_usage = Some(usage);
                    }
                    state.done = true;
                    events.push(event);
                }
                OpenAIResponsesEvent::Error { .. } => {
                    events.push(event);
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

/// Build a fully-specified `PreparedRoute` for the OpenAI Responses API.
///
/// - Endpoint: `POST https://api.openai.com/v1/responses`
/// - Auth: `Authorization: Bearer` header (value from `auth` map key `"api_key"`)
/// - Framing: SSE
/// - Transport: HTTP
pub fn responses_route() -> PreparedRoute {
    let mut auth = HashMap::new();
    auth.insert(
        "Authorization".to_string(),
        "Bearer ${OPENAI_API_KEY}".to_string(),
    );

    PreparedRoute {
        id: "openai-responses".to_string(),
        provider: jcode_llm_core::schema::ModelRef {
            provider_id: "openai".into(),
            id: String::new(),
            variant: None,
        },
        protocol: "openai-responses-2025-01-01".to_string(),
        endpoint: Endpoint {
            base_url: "https://api.openai.com".to_string(),
            path: PathSpec::Static("/v1/responses".to_string()),
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
        let protocol = OpenAiResponsesProtocol;
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
        assert!(!body.input.is_empty());
        assert!(body.stream);
        assert_eq!(body.max_output_tokens, Some(512));
        assert_eq!(body.temperature, Some(0.7));
    }

    #[test]
    fn test_body_from_request_with_tools() {
        let protocol = OpenAiResponsesProtocol;
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
        assert!(body.tool_choice.is_some());
        assert!(!body.stream);
        assert_eq!(body.max_output_tokens, Some(4096));
    }

    #[test]
    fn test_body_from_request_with_assistant_tool_calls() {
        let protocol = OpenAiResponsesProtocol;
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
                    content: vec![
                        ContentPart::Text {
                            text: "I'll check:".into(),
                        },
                        ContentPart::ToolCall {
                            id: "call_123".into(),
                            name: "get_weather".into(),
                            input: serde_json::json!({"location": "San Francisco"}),
                        },
                    ],
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

        // Should have: user msg, assistant text, function_call, function_call_output
        assert!(!body.input.is_empty(), "should have input items");

        // Find the function call input item.
        let has_fc = body.input.iter().any(|item| {
            matches!(item, ResponseInputItem::FunctionCall { name, .. } if name == "get_weather")
        });
        assert!(has_fc, "should contain function_call");

        // Find the function call output item.
        let has_fco = body.input.iter().any(|item| {
            matches!(item, ResponseInputItem::FunctionCallOutput { call_id, .. } if call_id == "call_123")
        });
        assert!(has_fco, "should contain function_call_output");
    }

    #[tokio::test]
    async fn test_step_response_created() {
        let protocol = OpenAiResponsesProtocol;
        let mut state = OpenAIResponsesState::default();

        let chunk = "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-4o\",\"output\":[],\"usage\":{\"input_tokens\":10,\"output_tokens\":0,\"total_tokens\":10}}}\n\n";

        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert_eq!(evts.len(), 1);
                assert!(
                    matches!(evts[0], OpenAIResponsesEvent::ResponseCreated { .. }),
                    "expected ResponseCreated, got {:?}",
                    evts[0]
                );
                assert!(state.started);
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_step_text_delta() {
        let protocol = OpenAiResponsesProtocol;
        let mut state = OpenAIResponsesState::default();

        let frames = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-4o\"}}\n",
            "\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n",
            "\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0}\n",
            "\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-4o\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5,\"total_tokens\":15}}}\n",
            "\n",
        );

        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert!(!evts.is_empty(), "expected events");

                // Should have a response.created.
                assert!(
                    evts.iter()
                        .any(|e| matches!(e, OpenAIResponsesEvent::ResponseCreated { .. })),
                    "expected ResponseCreated"
                );

                // Should have text deltas.
                assert!(
                    evts.iter()
                        .any(|e| matches!(e, OpenAIResponsesEvent::ResponseOutputTextDelta { .. })),
                    "expected ResponseOutputTextDelta"
                );

                // Should have response.completed.
                assert!(
                    evts.iter()
                        .any(|e| matches!(e, OpenAIResponsesEvent::ResponseCompleted { .. })),
                    "expected ResponseCompleted"
                );
            }
            other => panic!("expected Events, got {:?}", other),
        }

        assert!(state.done, "state should be done");
    }

    #[tokio::test]
    async fn test_step_function_call() {
        let protocol = OpenAiResponsesProtocol;
        let mut state = OpenAIResponsesState::default();

        let frames = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-4o\"}}\n",
            "\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"search\",\"arguments\":\"\"},\"output_index\":0}\n",
            "\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"call_id\":\"call_1\",\"delta\":\"{\\\"q\\\":\\\"hello\\\"}\"}\n",
            "\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"call_id\":\"call_1\",\"name\":\"search\",\"arguments\":\"{\\\"q\\\":\\\"hello\\\"}\"}\n",
            "\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":8,\"total_tokens\":18}}}\n",
            "\n",
        );

        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert!(!evts.is_empty(), "expected events");

                // Should have output_item.added.
                assert!(
                    evts.iter()
                        .any(|e| matches!(e, OpenAIResponsesEvent::ResponseOutputItemAdded { .. })),
                    "expected ResponseOutputItemAdded"
                );

                // Should have arguments delta.
                assert!(
                    evts.iter().any(|e| matches!(
                        e,
                        OpenAIResponsesEvent::ResponseFunctionCallArgumentsDelta { .. }
                    )),
                    "expected ResponseFunctionCallArgumentsDelta"
                );

                // Should have arguments done.
                assert!(
                    evts.iter().any(|e| matches!(
                        e,
                        OpenAIResponsesEvent::ResponseFunctionCallArgumentsDone { .. }
                    )),
                    "expected ResponseFunctionCallArgumentsDone"
                );
            }
            other => panic!("expected Events, got {:?}", other),
        }

        assert!(state.done);
    }

    #[tokio::test]
    async fn test_step_reasoning_delta() {
        let protocol = OpenAiResponsesProtocol;
        let mut state = OpenAIResponsesState::default();

        let frames = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\",\"model\":\"gpt-4o\"}}\n",
            "\n",
            "data: {\"type\":\"response.reasoning.delta\",\"delta\":\"thinking...\",\"item_id\":\"rs_1\",\"output_index\":0}\n",
            "\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5,\"total_tokens\":15,\"output_tokens_details\":{\"reasoning_tokens\":3}}}}\n",
            "\n",
        );

        let result = protocol.step(&mut state, Some(frames.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert!(!evts.is_empty(), "expected events");

                // Should have reasoning delta.
                assert!(
                    evts.iter()
                        .any(|e| matches!(e, OpenAIResponsesEvent::ResponseReasoningDelta { .. })),
                    "expected ResponseReasoningDelta"
                );

                // Should have completed with usage containing reasoning tokens.
                if let Some(OpenAIResponsesEvent::ResponseCompleted { response }) = evts
                    .iter()
                    .find(|e| matches!(e, OpenAIResponsesEvent::ResponseCompleted { .. }))
                {
                    let usage = extract_usage(&response);
                    assert!(usage.is_some());
                    let u = usage.unwrap();
                    assert_eq!(u.input_tokens, 10);
                    assert_eq!(u.output_tokens, 5);
                    assert!(u.breakdown.is_some());
                    assert_eq!(u.breakdown.unwrap().reasoning_tokens, Some(3));
                }
            }
            other => panic!("expected Events, got {:?}", other),
        }

        assert!(state.done);
    }

    #[tokio::test]
    async fn test_step_error() {
        let protocol = OpenAiResponsesProtocol;
        let mut state = OpenAIResponsesState::default();

        let chunk = "data: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"Bad request\"}}\n\n";

        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        match result {
            StepOutput::Events(evts) => {
                assert!(
                    matches!(evts[0], OpenAIResponsesEvent::Error { .. }),
                    "expected Error event, got {:?}",
                    evts[0]
                );
            }
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_step_need_more() {
        let protocol = OpenAiResponsesProtocol;
        let mut state = OpenAIResponsesState::default();

        // Incomplete frame — no double-newline.
        let chunk = "data: {\"type\":\"response.created\"";
        let result = protocol.step(&mut state, Some(chunk.as_bytes())).await;
        assert!(matches!(result, StepOutput::NeedMore));
    }

    #[test]
    fn test_map_tool_choice() {
        assert!(map_tool_choice(&ToolChoice::Auto).is_none());
        assert_eq!(
            map_tool_choice(&ToolChoice::Any),
            Some(Value::String("required".to_string()))
        );
        assert_eq!(
            map_tool_choice(&ToolChoice::None),
            Some(Value::String("none".to_string()))
        );
        let specific = map_tool_choice(&ToolChoice::Specific { name: "foo".into() });
        assert!(specific.is_some());
    }

    #[test]
    fn test_responses_route_has_correct_values() {
        let r = responses_route();
        assert_eq!(r.protocol, "openai-responses-2025-01-01");
        assert_eq!(r.endpoint.base_url, "https://api.openai.com");
        assert_eq!(r.framing, Framing::Sse);
        assert_eq!(r.transport, Transport::Http);
        assert!(r.auth.contains_key("Authorization"));
    }

    #[test]
    fn test_extract_usage_with_details() {
        let response = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "total_tokens": 150,
                "output_tokens_details": {
                    "reasoning_tokens": 20
                }
            }
        });

        let usage = extract_usage(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
        assert_eq!(usage.breakdown.unwrap().reasoning_tokens, Some(20));
    }

    #[test]
    fn test_extract_usage_with_cached_tokens() {
        let response = serde_json::json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "total_tokens": 150,
                "input_tokens_details": {
                    "cached_tokens": 30
                }
            }
        });

        let usage = extract_usage(&response).unwrap();
        assert_eq!(usage.cache_read_input_tokens, 30);
    }

    #[test]
    fn test_body_from_request_with_reasoning_part() {
        let protocol = OpenAiResponsesProtocol;
        let req = LlmRequest {
            model: ModelRef::parse("openai/gpt-4o").unwrap(),
            messages: vec![Message {
                role: "assistant".into(),
                content: vec![
                    ContentPart::Reasoning {
                        text: "I need to think about this.".into(),
                    },
                    ContentPart::Text {
                        text: "Here is my answer.".into(),
                    },
                ],
            }],
            system: None,
            tools: None,
            tool_choice: None,
            generation_params: None,
            stream: false,
            route_id: None,
        };

        let (body, _state) = protocol.body_from_request(&req).unwrap();
        assert!(!body.input.is_empty(), "should have input items");

        // Should have a reasoning item.
        let has_reasoning = body
            .input
            .iter()
            .any(|item| matches!(item, ResponseInputItem::Reasoning { .. }));
        assert!(has_reasoning, "should contain reasoning item");
    }
}
