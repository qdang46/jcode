use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum PluginEvent {
    #[serde(rename = "PreToolUse")]
    PreToolUse = 0,
    #[serde(rename = "PostToolUse")]
    PostToolUse = 1,
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure = 2,
    #[serde(rename = "ToolExecutionStart")]
    ToolExecutionStart = 3,
    #[serde(rename = "ToolExecutionEnd")]
    ToolExecutionEnd = 4,
    #[serde(rename = "SessionStart")]
    SessionStart = 5,
    #[serde(rename = "SessionEnd")]
    SessionEnd = 6,
    #[serde(rename = "SessionSwitch")]
    SessionSwitch = 7,
    #[serde(rename = "SessionCompact")]
    SessionCompact = 8,
    #[serde(rename = "SessionBeforeCompact")]
    SessionBeforeCompact = 9,
    #[serde(rename = "SessionShutdown")]
    SessionShutdown = 10,
    #[serde(rename = "PermissionRequest")]
    PermissionRequest = 12,
    #[serde(rename = "PermissionDenied")]
    PermissionDenied = 13,
    #[serde(rename = "AgentStart")]
    AgentStart = 14,
    #[serde(rename = "AgentEnd")]
    AgentEnd = 15,
    #[serde(rename = "TurnStart")]
    TurnStart = 16,
    #[serde(rename = "TurnEnd")]
    TurnEnd = 17,
    #[serde(rename = "MessageStart")]
    MessageStart = 18,
    #[serde(rename = "MessageEnd")]
    MessageEnd = 19,
    #[serde(rename = "PreCompact")]
    PreCompact = 20,
    #[serde(rename = "PostCompact")]
    PostCompact = 21,
    #[serde(rename = "TaskCreated")]
    TaskCreated = 22,
    #[serde(rename = "TaskCompleted")]
    TaskCompleted = 23,
    #[serde(rename = "AutoCompactionStart")]
    AutoCompactionStart = 24,
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit = 25,
    #[serde(rename = "Stop")]
    Stop = 26,
    #[serde(rename = "Notification")]
    Notification = 27,
}

impl PluginEvent {
    /// Total number of event variants.
    /// Note: discriminant 11 is intentionally skipped (reserved for future use).
    pub const COUNT: u32 = 27;

    /// All event variants
    pub fn all() -> Vec<PluginEvent> {
        use PluginEvent::*;
        vec![
            PreToolUse,
            PostToolUse,
            PostToolUseFailure,
            ToolExecutionStart,
            ToolExecutionEnd,
            SessionStart,
            SessionEnd,
            SessionSwitch,
            SessionCompact,
            SessionBeforeCompact,
            SessionShutdown,
            PermissionRequest,
            PermissionDenied,
            AgentStart,
            AgentEnd,
            TurnStart,
            TurnEnd,
            MessageStart,
            MessageEnd,
            PreCompact,
            PostCompact,
            TaskCreated,
            TaskCompleted,
            AutoCompactionStart,
            UserPromptSubmit,
            Stop,
            Notification,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum EventInput {
    #[serde(rename = "PreToolUse")]
    PreToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
        session_id: String,
    },
    #[serde(rename = "PostToolUse")]
    PostToolUse {
        tool_name: String,
        tool_input: serde_json::Value,
        tool_output: serde_json::Value,
        duration_ms: u64,
        success: bool,
        session_id: String,
    },
    #[serde(rename = "PostToolUseFailure")]
    PostToolUseFailure {
        tool_name: String,
        tool_input: serde_json::Value,
        error: String,
        duration_ms: u64,
        session_id: String,
    },
    #[serde(rename = "SessionStart")]
    SessionStart {
        session_id: String,
        project_dir: String,
        model: String,
        provider: String,
    },
    #[serde(rename = "SessionEnd")]
    SessionEnd {
        session_id: String,
        duration_seconds: u64,
        message_count: u64,
    },
    #[serde(rename = "PermissionRequest")]
    PermissionRequest {
        action: String,
        tool_name: Option<String>,
        target: Option<String>,
        session_id: String,
    },
    #[serde(rename = "AgentStart")]
    AgentStart {
        session_id: String,
        system_prompt: serde_json::Value,
        tools: serde_json::Value,
    },
    #[serde(rename = "TurnStart")]
    TurnStart {
        session_id: String,
        turn_number: u32,
        messages: serde_json::Value,
    },
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit { content: String, session_id: String },
    #[serde(rename = "PreCompact")]
    PreCompact {
        session_id: String,
        message_count: u32,
        token_count: u64,
        system_prompt: serde_json::Value,
    },
    #[serde(rename = "PostCompact")]
    PostCompact {
        session_id: String,
        messages_removed: u32,
        tokens_saved: u64,
    },
    #[serde(rename = "Stop")]
    Stop { session_id: String, reason: String },
    #[serde(rename = "Notification")]
    Notification {
        level: String,
        message: String,
        session_id: Option<String>,
    },
    #[serde(rename = "ToolExecutionStart")]
    ToolExecutionStart {
        tool_name: String,
        tool_input: serde_json::Value,
        session_id: String,
    },
    #[serde(rename = "ToolExecutionEnd")]
    ToolExecutionEnd {
        tool_name: String,
        tool_output: serde_json::Value,
        duration_ms: u64,
        session_id: String,
    },
    #[serde(rename = "AgentEnd")]
    AgentEnd {
        session_id: String,
        duration_seconds: u64,
        message_count: u64,
    },
    #[serde(rename = "SessionSwitch")]
    SessionSwitch {
        session_id: String,
        target_session_id: String,
    },
    #[serde(rename = "SessionCompact")]
    SessionCompact { session_id: String, reason: String },
    #[serde(rename = "TurnEnd")]
    TurnEnd {
        session_id: String,
        turn_number: u32,
        duration_ms: u64,
    },
    #[serde(rename = "MessageStart")]
    MessageStart { session_id: String, role: String },
    #[serde(rename = "MessageEnd")]
    MessageEnd {
        session_id: String,
        role: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum EventOutput {
    #[serde(rename = "PreToolUse")]
    PreToolUse {
        block: Option<String>,
        modified_input: Option<serde_json::Value>,
    },
    #[serde(rename = "PostToolUse")]
    PostToolUse {
        modified_output: Option<serde_json::Value>,
    },
    #[serde(rename = "PermissionRequest")]
    PermissionRequest {
        decision: Option<PermissionDecision>,
        message: Option<String>,
    },
    #[serde(rename = "AgentStart")]
    AgentStart {
        additional_system_prompt: Vec<String>,
    },
    #[serde(rename = "PreCompact")]
    PreCompact {
        system_prompt: Option<serde_json::Value>,
        instructions: Option<String>,
        prevent: bool,
    },
    #[serde(rename = "UserPromptSubmit")]
    UserPromptSubmit { modified_prompt: Option<String> },
    #[serde(rename = "Notification")]
    Notification {
        suppress: Option<bool>,
        modified_message: Option<String>,
    },
    #[serde(rename = "Stop")]
    Stop { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PermissionDecision {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerResult {
    #[serde(default)]
    pub action: HandlerAction,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum HandlerAction {
    #[default]
    #[serde(rename = "continue")]
    Continue,
    #[serde(rename = "block")]
    Block(String),
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "error")]
    Error,
}

impl Default for HandlerResult {
    fn default() -> Self {
        Self {
            action: HandlerAction::Continue,
            output: None,
            error: None,
        }
    }
}
