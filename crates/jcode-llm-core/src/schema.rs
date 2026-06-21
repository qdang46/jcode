use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A provider identifier (e.g. "anthropic", "openai", "gemini").
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::ops::Deref for ProviderId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

/// A routing identifier for provider model resolution.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
pub struct RouteId(pub String);

impl RouteId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for RouteId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RouteId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::ops::Deref for RouteId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

/// Reference to a specific model on a provider.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelRef {
    pub provider_id: ProviderId,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Why generation finished.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolUse,
    ContentFiltered,
    Error,
    Unknown,
}

/// A part of message content.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Media {
        media_type: String,
        data: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    Reasoning {
        text: String,
    },
}

/// A message in a conversation with an LLM.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentPart>,
}

/// Definition of a tool that can be called by the model.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Controls how the model should pick tools.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    Any,
    /// Provide no tool to the model (equivalent to not sending tools).
    #[serde(rename = "none")]
    None,
    /// Force the model to use a specific named tool.
    Specific {
        name: String,
    },
}

/// Parameters controlling text generation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerationParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// Token usage breakdown with per-category detail.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UsageBreakdown {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

/// Token usage for an LLM request (inclusive totals + breakdown).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<UsageBreakdown>,
}

/// HTTP response context for error reporting.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HttpContext {
    pub status_code: u16,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Errors that can occur during LLM requests.
///
/// Contains 9 variants:
/// - `HttpError` — HTTP-level failure with full context.
/// - `RateLimited` — provider rate limit hit.
/// - `Authentication` — invalid or missing credentials.
/// - `InvalidRequest` — malformed or rejected request.
/// - `ModelNotFound` — the requested model does not exist.
/// - `ModelOverloaded` — provider model is overloaded.
/// - `ContextLengthExceeded` — context window exceeded.
/// - `Timeout` — request exceeded the deadline.
/// - `Internal` — unexpected provider or transport error.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmError {
    HttpError {
        context: HttpContext,
    },
    RateLimited {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_secs: Option<f64>,
    },
    Authentication {
        message: String,
    },
    InvalidRequest {
        message: String,
    },
    ModelNotFound {
        model_id: String,
    },
    ModelOverloaded {
        message: String,
    },
    ContextLengthExceeded {
        context_tokens: u64,
        max_tokens: u64,
    },
    Timeout {
        elapsed_secs: f64,
    },
    Internal {
        message: String,
    },
}

/// Events emitted during an LLM request lifecycle.
///
/// 15 variants covering the full lifecycle of a request:
/// creation, streaming (text, tool calls, reasoning),
/// usage updates, errors, and cancellation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LlmEvent {
    /// Request object created but not yet dispatched.
    RequestCreated { id: String, model: ModelRef },
    /// Request dispatched to the provider.
    RequestStarted { id: String },
    /// Request completed (all output received).
    RequestFinished {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish_reason: Option<FinishReason>,
    },
    /// First response data received from the provider.
    ResponseStarted { id: String },
    /// Response fully received from the provider.
    ResponseFinished { id: String },
    /// Text content delta received.
    TextGenerated { id: String, delta: String },
    /// A new tool call was initiated by the model.
    ToolCallCreated {
        id: String,
        tool_call_id: String,
        name: String,
    },
    /// Streaming tool call input delta received.
    ToolCallUpdated {
        id: String,
        tool_call_id: String,
        delta: String,
    },
    /// Tool call input fully received.
    ToolCallCompleted { id: String, tool_call_id: String },
    /// Model reasoning (thinking) started.
    ReasoningStarted { id: String },
    /// Reasoning content delta received.
    ReasoningDelta { id: String, delta: String },
    /// Model reasoning finished.
    ReasoningFinished { id: String },
    /// Token usage information updated.
    UsageUpdated { id: String, usage: Usage },
    /// An error occurred during the request.
    Error { id: String, error: LlmError },
    /// Request was cancelled before completion.
    Cancelled {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// An LLM request with all parameters.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LlmRequest {
    pub model: ModelRef,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_params: Option<GenerationParams>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<RouteId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_from_str() {
        let pid: ProviderId = "anthropic".into();
        assert_eq!(pid.as_str(), "anthropic");
    }

    #[test]
    fn test_route_id_from_str() {
        let rid: RouteId = "default".into();
        assert_eq!(rid.as_str(), "default");
    }

    #[test]
    fn test_model_ref_serde() {
        let model = ModelRef {
            provider_id: "anthropic".into(),
            id: "claude-sonnet-4-20250514".into(),
            variant: None,
        };
        let json = serde_json::to_string(&model).unwrap();
        let deserialized: ModelRef = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.provider_id.as_str(), "anthropic");
        assert_eq!(deserialized.id, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_usage_round_trip() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_input_tokens: 10,
            cache_creation_input_tokens: 5,
            total_tokens: 165,
            breakdown: Some(UsageBreakdown {
                audio_input_tokens: Some(0),
                reasoning_tokens: Some(20),
            }),
        };
        let json = serde_json::to_string(&usage).unwrap();
        let deserialized: Usage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_tokens, 165);
        assert!(deserialized.breakdown.is_some());
        assert_eq!(deserialized.breakdown.unwrap().reasoning_tokens, Some(20));
    }

    #[test]
    fn test_llm_request_serde() {
        let request = LlmRequest {
            model: ModelRef {
                provider_id: "anthropic".into(),
                id: "claude-sonnet-4-20250514".into(),
                variant: None,
            },
            messages: vec![Message {
                role: "user".into(),
                content: vec![ContentPart::Text {
                    text: "Hello".into(),
                }],
            }],
            system: Some("You are a helpful assistant.".into()),
            tools: None,
            tool_choice: None,
            generation_params: Some(GenerationParams {
                temperature: Some(0.7),
                max_tokens: Some(4096),
                stop_sequences: None,
                top_p: None,
                top_k: None,
                presence_penalty: None,
                frequency_penalty: None,
                seed: None,
            }),
            stream: false,
            route_id: Some("default".into()),
        };
        let json = serde_json::to_string(&request).unwrap();
        let deserialized: LlmRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model.id, "claude-sonnet-4-20250514");
        assert_eq!(deserialized.messages.len(), 1);
        assert!(deserialized.system.is_some());
        assert!(deserialized.generation_params.is_some());
    }

    #[test]
    fn test_llm_event_count() {
        // Ensure we can construct all 15 variants
        let model = ModelRef {
            provider_id: "test".into(),
            id: "test".into(),
            variant: None,
        };
        let usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            total_tokens: 0,
            breakdown: None,
        };
        let _events: Vec<LlmEvent> = vec![
            LlmEvent::RequestCreated {
                id: "1".into(),
                model: model.clone(),
            },
            LlmEvent::RequestStarted { id: "1".into() },
            LlmEvent::RequestFinished {
                id: "1".into(),
                finish_reason: Some(FinishReason::Stop),
            },
            LlmEvent::ResponseStarted { id: "1".into() },
            LlmEvent::ResponseFinished { id: "1".into() },
            LlmEvent::TextGenerated {
                id: "1".into(),
                delta: "hello".into(),
            },
            LlmEvent::ToolCallCreated {
                id: "1".into(),
                tool_call_id: "tc1".into(),
                name: "search".into(),
            },
            LlmEvent::ToolCallUpdated {
                id: "1".into(),
                tool_call_id: "tc1".into(),
                delta: "{\"q".into(),
            },
            LlmEvent::ToolCallCompleted {
                id: "1".into(),
                tool_call_id: "tc1".into(),
            },
            LlmEvent::ReasoningStarted { id: "1".into() },
            LlmEvent::ReasoningDelta {
                id: "1".into(),
                delta: "thinking...".into(),
            },
            LlmEvent::ReasoningFinished { id: "1".into() },
            LlmEvent::UsageUpdated {
                id: "1".into(),
                usage,
            },
            LlmEvent::Error {
                id: "1".into(),
                error: LlmError::Internal {
                    message: "oops".into(),
                },
            },
            LlmEvent::Cancelled {
                id: "1".into(),
                reason: Some("user interrupted".into()),
            },
        ];
    }

    #[test]
    fn test_content_part_variants() {
        let text = ContentPart::Text { text: "hi".into() };
        let media = ContentPart::Media {
            media_type: "image/png".into(),
            data: "base64data".into(),
        };
        let tc = ContentPart::ToolCall {
            id: "tc1".into(),
            name: "search".into(),
            input: serde_json::json!({"q": "hello"}),
        };
        let tr = ContentPart::ToolResult {
            tool_use_id: "tc1".into(),
            content: "result".into(),
            is_error: None,
        };
        let reasoning = ContentPart::Reasoning {
            text: "thinking".into(),
        };

        for part in &[&text, &media, &tc, &tr, &reasoning] {
            let json = serde_json::to_string(part).unwrap();
            let _back: ContentPart = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_tool_choice_serde() {
        assert_eq!(
            serde_json::to_string(&ToolChoice::Auto).unwrap(),
            r#""auto""#
        );
        assert_eq!(serde_json::to_string(&ToolChoice::Any).unwrap(), r#""any""#);
        assert_eq!(
            serde_json::to_string(&ToolChoice::None).unwrap(),
            r#""none""#
        );
        let specific = serde_json::to_string(&ToolChoice::Specific {
            name: "search".into(),
        })
        .unwrap();
        assert!(specific.contains("search"));
    }

    #[test]
    fn test_finish_reason_serde() {
        let reasons = [
            FinishReason::Stop,
            FinishReason::Length,
            FinishReason::ToolUse,
            FinishReason::ContentFiltered,
            FinishReason::Error,
            FinishReason::Unknown,
        ];
        for reason in &reasons {
            let json = serde_json::to_string(reason).unwrap();
            let _back: FinishReason = serde_json::from_str(&json).unwrap();
        }
    }

    #[test]
    fn test_llm_error_serde() {
        use std::collections::HashMap;

        let http_err = LlmError::HttpError {
            context: HttpContext {
                status_code: 429,
                url: "https://api.anthropic.com/v1/messages".into(),
                headers: Some(HashMap::from([("x-request-id".into(), "req_123".into())])),
                body: Some("{\"error\": {\"type\": \"rate_limit\"}}".into()),
            },
        };
        let json = serde_json::to_string(&http_err).unwrap();
        let deserialized: LlmError = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, LlmError::HttpError { .. }));

        let all_errors: Vec<LlmError> = vec![
            LlmError::HttpError {
                context: HttpContext {
                    status_code: 500,
                    url: "https://api.example.com".into(),
                    headers: None,
                    body: None,
                },
            },
            LlmError::RateLimited {
                retry_after_secs: Some(30.0),
            },
            LlmError::Authentication {
                message: "invalid key".into(),
            },
            LlmError::InvalidRequest {
                message: "bad request".into(),
            },
            LlmError::ModelNotFound {
                model_id: "gpt-5".into(),
            },
            LlmError::ModelOverloaded {
                message: "overloaded".into(),
            },
            LlmError::ContextLengthExceeded {
                context_tokens: 200_000,
                max_tokens: 100_000,
            },
            LlmError::Timeout {
                elapsed_secs: 120.0,
            },
            LlmError::Internal {
                message: "unexpected".into(),
            },
        ];
        assert_eq!(all_errors.len(), 9);
        for err in &all_errors {
            let json = serde_json::to_string(err).unwrap();
            let _back: LlmError = serde_json::from_str(&json).unwrap();
        }
    }
}
