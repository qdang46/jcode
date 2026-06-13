//! Hook v2 types — Input/Output protocol, event constants, result enums, and metrics.
//!
//! This module defines the complete JSON contract between jcode and hook handlers.
//! Every hook receives a `HookInput` via stdin and returns a `HookOutput` via stdout.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ===========================================================================
// EVENT NAME CONSTANTS (28 events)
// ===========================================================================

/// Core tool events
pub const EVENT_PRE_TOOL_USE: &str = "PreToolUse";
pub const EVENT_POST_TOOL_USE: &str = "PostToolUse";
pub const EVENT_POST_TOOL_USE_FAILURE: &str = "PostToolUseFailure";
pub const EVENT_TOOL_ERROR: &str = "ToolError";
pub const EVENT_USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";
pub const EVENT_USER_PROMPT_EXPANSION: &str = "UserPromptExpansion";

/// Session lifecycle events
pub const EVENT_SESSION_START: &str = "SessionStart";
pub const EVENT_SESSION_END: &str = "SessionEnd";
pub const EVENT_SESSION_UPDATED: &str = "SessionUpdated";
pub const EVENT_SESSION_DIFF: &str = "SessionDiff";
pub const EVENT_SESSION_ERROR: &str = "SessionError";
pub const EVENT_SESSION_IDLE: &str = "SessionIdle";

/// Permission events
pub const EVENT_PERMISSION_REQUEST: &str = "PermissionRequest";
pub const EVENT_PERMISSION_DENIED: &str = "PermissionDenied";
pub const EVENT_PERMISSION_ASKED: &str = "PermissionAsked";
pub const EVENT_PERMISSION_REPLIED: &str = "PermissionReplied";

/// Agent and subagent events
pub const EVENT_AGENT_START: &str = "AgentStart";
pub const EVENT_AGENT_END: &str = "AgentEnd";
pub const EVENT_SUBAGENT_START: &str = "SubagentStart";
pub const EVENT_SUBAGENT_STOP: &str = "SubagentStop";

/// Turn lifecycle events
pub const EVENT_TURN_END: &str = "TurnEnd";

/// Execution control events
pub const EVENT_STOP: &str = "Stop";

/// Compaction events
pub const EVENT_PRE_COMPACT: &str = "PreCompact";
pub const EVENT_POST_COMPACT: &str = "PostCompact";
pub const EVENT_AUTO_COMPACTION_CONTROL: &str = "AutoCompactionControl";

/// Task and environment events
pub const EVENT_SETUP: &str = "Setup";
pub const EVENT_TASK_CREATED: &str = "TaskCreated";
pub const EVENT_TASK_COMPLETED: &str = "TaskCompleted";

/// File events
pub const EVENT_FILE_CHANGED: &str = "FileChanged";

/// All known event names as a static slice for validation and iteration.
pub const ALL_EVENT_NAMES: &[&str] = &[
    EVENT_PRE_TOOL_USE,
    EVENT_POST_TOOL_USE,
    EVENT_POST_TOOL_USE_FAILURE,
    EVENT_TOOL_ERROR,
    EVENT_USER_PROMPT_SUBMIT,
    EVENT_USER_PROMPT_EXPANSION,
    EVENT_SESSION_START,
    EVENT_SESSION_END,
    EVENT_SESSION_UPDATED,
    EVENT_SESSION_DIFF,
    EVENT_SESSION_ERROR,
    EVENT_SESSION_IDLE,
    EVENT_PERMISSION_REQUEST,
    EVENT_PERMISSION_DENIED,
    EVENT_PERMISSION_ASKED,
    EVENT_PERMISSION_REPLIED,
    EVENT_AGENT_START,
    EVENT_AGENT_END,
    EVENT_SUBAGENT_START,
    EVENT_SUBAGENT_STOP,
    EVENT_TURN_END,
    EVENT_STOP,
    EVENT_PRE_COMPACT,
    EVENT_POST_COMPACT,
    EVENT_AUTO_COMPACTION_CONTROL,
    EVENT_SETUP,
    EVENT_TASK_CREATED,
    EVENT_TASK_COMPLETED,
    EVENT_FILE_CHANGED,
];

// ===========================================================================
// HOOK INPUT — Stdin JSON contract
// ===========================================================================

/// Standard input passed to every hook via stdin JSON.
///
/// All fields except the five required ones are `Option` to allow
/// event-specific subsets. Hooks receive only the fields relevant
/// to the triggering event; unused fields arrive as `null`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HookInput {
    // === Always present (required) ===
    /// Schema version — always "2.0" for the v2 hook protocol.
    pub schema_version: String,
    /// Unique session identifier.
    pub session_id: String,
    /// Current working directory at the time the event fired.
    pub cwd: String,
    /// The event name (e.g. "PreToolUse", "SessionStart").
    pub hook_event_name: String,
    /// UTC timestamp when the event was generated.
    pub timestamp: DateTime<Utc>,

    // === Session info ===
    pub transcript_path: Option<String>,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,

    // === Tool-related ===
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_output: Option<serde_json::Value>,
    pub tool_use_id: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<i32>,
    pub duration_ms: Option<u64>,

    // === Permission-related ===
    pub permission_mode: Option<String>,
    pub permission_decision: Option<String>,
    pub request_id: Option<String>,
    pub action_description: Option<String>,

    // === User prompt ===
    pub prompt: Option<String>,
    pub prompt_text: Option<String>,
    pub files: Option<Vec<String>>,
    pub expanded_prompt: Option<String>,

    // === Agent lifecycle ===
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub agent_turns: Option<u32>,
    pub total_cost: Option<f64>,
    pub parent_agent_id: Option<String>,
    pub subagent_id: Option<String>,
    pub subagent_type: Option<String>,

    // === Compact ===
    pub current_size_bytes: Option<u64>,
    pub target_size_bytes: Option<u64>,
    pub message_count: Option<u64>,
    pub compacted_size_bytes: Option<u64>,
    pub saved_bytes: Option<u64>,

    // === Session state ===
    pub prev_state: Option<String>,
    pub new_state: Option<String>,
    pub update_reason: Option<String>,
    pub idle_duration_secs: Option<u64>,
    pub idle_threshold_secs: Option<u64>,
    pub last_activity: Option<DateTime<Utc>>,

    // === Task ===
    pub task_id: Option<String>,
    pub task_type: Option<String>,
    pub task_description: Option<String>,
    pub parent_task_id: Option<String>,
    pub task_result: Option<serde_json::Value>,

    // === File ===
    pub file_path: Option<String>,
    pub change_type: Option<String>,
    pub diff: Option<String>,

    // === Env ===
    pub env_vars: Option<HashMap<String, String>>,
    pub config_path: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub exit_reason: Option<String>,
    pub total_tool_calls: Option<u64>,
    pub stop_type: Option<String>,
    pub stop_reason: Option<String>,
    pub continue_loop: Option<bool>,
}

impl Default for HookInput {
    fn default() -> Self {
        Self {
            schema_version: "2.0".to_string(),
            session_id: String::new(),
            cwd: String::new(),
            hook_event_name: String::new(),
            timestamp: Utc::now(),

            transcript_path: None,
            agent_id: None,
            agent_type: None,

            tool_name: None,
            tool_input: None,
            tool_output: None,
            tool_use_id: None,
            error: None,
            error_code: None,
            duration_ms: None,

            permission_mode: None,
            permission_decision: None,
            request_id: None,
            action_description: None,

            prompt: None,
            prompt_text: None,
            files: None,
            expanded_prompt: None,

            model: None,
            system_prompt: None,
            agent_turns: None,
            total_cost: None,
            parent_agent_id: None,
            subagent_id: None,
            subagent_type: None,

            current_size_bytes: None,
            target_size_bytes: None,
            message_count: None,
            compacted_size_bytes: None,
            saved_bytes: None,

            prev_state: None,
            new_state: None,
            update_reason: None,
            idle_duration_secs: None,
            idle_threshold_secs: None,
            last_activity: None,

            task_id: None,
            task_type: None,
            task_description: None,
            parent_task_id: None,
            task_result: None,

            file_path: None,
            change_type: None,
            diff: None,

            env_vars: None,
            config_path: None,
            start_time: None,
            exit_reason: None,
            total_tool_calls: None,
            stop_type: None,
            stop_reason: None,
            continue_loop: None,
        }
    }
}

// ===========================================================================
// HOOK INPUT BUILDER
// ===========================================================================

/// Builder pattern for constructing event-specific `HookInput` values.
///
/// Ensures required fields are set and optional fields are correct per event.
///
/// # Example
///
/// ```ignore
/// let input = HookInputBuilder::new()
///     .session("ses_123", "/home/user/project")
///     .event("PreToolUse")
///     .agent("agent_1", "default")
///     .tool("Bash", serde_json::json!({"command": "ls"}), "tool_1")
///     .build();
/// ```
#[derive(Debug, Default)]
pub struct HookInputBuilder {
    input: HookInput,
}

impl HookInputBuilder {
    /// Create a new builder with default (empty) values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the session identifier and working directory.
    pub fn session(mut self, session_id: &str, cwd: &str) -> Self {
        self.input.session_id = session_id.to_string();
        self.input.cwd = cwd.to_string();
        self
    }

    /// Set the hook event name.
    pub fn event(mut self, event_name: &str) -> Self {
        self.input.hook_event_name = event_name.to_string();
        self
    }

    /// Set agent identification fields.
    pub fn agent(mut self, agent_id: &str, agent_type: &str) -> Self {
        self.input.agent_id = Some(agent_id.to_string());
        self.input.agent_type = Some(agent_type.to_string());
        self
    }

    /// Set tool-related fields: name, input payload, and use-id.
    pub fn tool(mut self, name: &str, input: serde_json::Value, use_id: &str) -> Self {
        self.input.tool_name = Some(name.to_string());
        self.input.tool_input = Some(input);
        self.input.tool_use_id = Some(use_id.to_string());
        self
    }

    /// Set the tool output value.
    pub fn tool_output(mut self, output: serde_json::Value) -> Self {
        self.input.tool_output = Some(output);
        self
    }

    /// Set permission-related fields.
    pub fn permission(mut self, mode: &str, request_id: &str, description: &str) -> Self {
        self.input.permission_mode = Some(mode.to_string());
        self.input.request_id = Some(request_id.to_string());
        self.input.action_description = Some(description.to_string());
        self
    }

    /// Set error information.
    pub fn error(mut self, error: &str, code: i32) -> Self {
        self.input.error = Some(error.to_string());
        self.input.error_code = Some(code);
        self
    }

    /// Set the execution duration in milliseconds.
    pub fn duration(mut self, ms: u64) -> Self {
        self.input.duration_ms = Some(ms);
        self
    }

    /// Set session state transition fields (SessionUpdated).
    pub fn session_state(mut self, prev_state: &str, new_state: &str, update_reason: &str) -> Self {
        self.input.prev_state = Some(prev_state.to_string());
        self.input.new_state = Some(new_state.to_string());
        self.input.update_reason = Some(update_reason.to_string());
        self
    }

    /// Set diff information (SessionDiff).
    pub fn diff(mut self, diff_text: &str, file_path: Option<&str>) -> Self {
        self.input.diff = Some(diff_text.to_string());
        self.input.file_path = file_path.map(|s| s.to_string());
        self
    }

    /// Set idle state information (SessionIdle).
    pub fn idle_state(
        mut self,
        idle_duration_secs: u64,
        idle_threshold_secs: Option<u64>,
        last_activity: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Self {
        self.input.idle_duration_secs = Some(idle_duration_secs);
        self.input.idle_threshold_secs = idle_threshold_secs;
        self.input.last_activity = last_activity;
        self
    }

    /// Set the user prompt text.
    pub fn prompt(mut self, text: &str) -> Self {
        self.input.prompt_text = Some(text.to_string());
        self
    }

    /// Consume the builder and produce the final `HookInput`.
    pub fn build(self) -> HookInput {
        self.input
    }
}

// ===========================================================================
// HOOK OUTPUT — Stdout JSON contract
// ===========================================================================

/// Standard output returned by hook scripts via stdout JSON.
///
/// Every field is optional — hooks return only what they need to override.
/// The `continue_` field defaults to `true` via serde when absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookOutput {
    /// Whether execution should continue. Default: `true`.
    /// For blocking events, setting this to `false` blocks/denies the operation.
    #[serde(default = "default_true")]
    pub continue_: bool,

    /// Suppress the tool/event output from being shown to the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,

    /// Reason for stopping/blocking (shown to agent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    /// Decision for permission-type hooks: `"allow"`, `"deny"`, or `"ask"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,

    /// Human-readable reason for the decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// System message to inject into the conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_message: Option<String>,

    /// Event-specific output overrides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

/// Serde default function: returns `true`.
fn default_true() -> bool {
    true
}

impl HookOutput {
    /// Create a default "continue" output (all fields at defaults).
    pub fn continue_() -> Self {
        Self {
            continue_: true,
            suppress_output: None,
            stop_reason: None,
            decision: None,
            reason: None,
            system_message: None,
            hook_specific_output: None,
        }
    }

    /// Create a "block/deny" output with the given reason.
    pub fn block(reason: &str) -> Self {
        Self {
            continue_: false,
            suppress_output: None,
            stop_reason: Some(reason.to_string()),
            decision: Some("deny".to_string()),
            reason: None,
            system_message: None,
            hook_specific_output: None,
        }
    }

    /// Create an "ask the user" output with the given reason.
    pub fn ask(reason: &str) -> Self {
        Self {
            continue_: false,
            suppress_output: None,
            stop_reason: None,
            decision: Some("ask".to_string()),
            reason: Some(reason.to_string()),
            system_message: None,
            hook_specific_output: None,
        }
    }

    /// Create an explicit "allow" output.
    pub fn allow() -> Self {
        Self {
            continue_: true,
            suppress_output: None,
            stop_reason: None,
            decision: Some("allow".to_string()),
            reason: None,
            system_message: None,
            hook_specific_output: None,
        }
    }
}

// ===========================================================================
// HOOK SPECIFIC OUTPUT
// ===========================================================================

/// Event-specific output fields carried inside `HookOutput.hook_specific_output`.
///
/// Each blocking event uses a subset of these fields to communicate
/// fine-grained overrides back to the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpecificOutput {
    /// The event name this output corresponds to.
    pub hook_event_name: String,

    // Permission events
    /// Permission decision override: `"allow"`, `"deny"`, or `"ask"`.
    pub permission_decision: Option<String>,
    /// Reason accompanying the permission decision.
    pub permission_decision_reason: Option<String>,

    // Tool events — modify input before execution
    /// Replacement tool input (PreToolUse).
    pub updated_input: Option<serde_json::Value>,

    // Prompt events — modify prompt before LLM
    /// Replacement prompt text (UserPromptSubmit).
    pub updated_prompt: Option<String>,

    // Agent events — modify system prompt
    /// Replacement system prompt (AgentStart).
    pub updated_system_prompt: Option<String>,

    // Compact events — override compacted system message
    /// Replacement system message after compaction (PreCompact).
    pub updated_system_message: Option<String>,

    // General — inject context
    /// Additional context string to inject into the conversation.
    pub additional_context: Option<String>,

    // Session setup — inject env vars
    /// Additional environment variables to set (Setup).
    pub additional_env_vars: Option<HashMap<String, String>>,
    /// Updated configuration values (Setup).
    pub updated_config: Option<serde_json::Value>,
}

// ===========================================================================
// HOOK RESULT
// ===========================================================================

/// Result of executing a single hook handler.
#[derive(Debug)]
pub enum HookResult {
    /// Hook completed successfully and execution should continue.
    Continue(HookOutput),
    /// Hook blocked the operation (exit code 2 or `continue_` = false).
    Blocked { reason: String, output: HookOutput },
    /// Hook failed (non-zero exit code other than 2, HTTP error, timeout).
    Failed { error: String },
}

// ===========================================================================
// AGGREGATED DECISION
// ===========================================================================

/// Aggregated decision from multiple hooks for blocking events.
///
/// Precedence: deny > ask > allow.
#[derive(Debug)]
pub enum AggregatedDecision {
    /// All hooks say continue, or no hooks configured.
    Allow,
    /// At least one hook says "ask" and no hook says "deny".
    Ask { reasons: Vec<String> },
    /// At least one hook blocked/denied the operation.
    Deny { reason: String, source_hook: String },
}

// ===========================================================================
// HOOK METRICS
// ===========================================================================

/// Metrics collected per hook execution for observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMetrics {
    /// The event this metric covers.
    pub event_name: String,
    /// Human-readable label for the handler (command string, URL, agent id).
    pub handler_label: String,
    /// Total number of times this hook has been executed.
    pub execution_count: u64,
    /// Number of executions that ended in failure.
    pub failure_count: u64,
    /// Number of executions that blocked the operation.
    pub blocked_count: u64,
    /// Cumulative execution time in milliseconds.
    pub total_duration_ms: u64,
    /// Average execution time in milliseconds.
    pub avg_duration_ms: f64,
    /// Timestamp of the most recent execution.
    pub last_execution: Option<DateTime<Utc>>,
    /// Error message from the most recent failure, if any.
    pub last_error: Option<String>,
}

// ===========================================================================
// TESTS
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Event name constants ---

    #[test]
    fn test_all_event_names_count() {
        assert_eq!(ALL_EVENT_NAMES.len(), 29);
    }

    #[test]
    fn test_event_name_constants_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for name in ALL_EVENT_NAMES {
            assert!(seen.insert(*name), "duplicate event name: {}", name);
        }
    }

    #[test]
    fn test_event_names_are_pascal_case() {
        for name in ALL_EVENT_NAMES {
            let first = name.chars().next().unwrap();
            assert!(
                first.is_uppercase(),
                "event name '{}' does not start with uppercase",
                name
            );
        }
    }

    // --- HookInput ---

    #[test]
    fn test_hook_input_default() {
        let input = HookInput::default();
        assert_eq!(input.schema_version, "2.0");
        assert!(input.session_id.is_empty());
        assert!(input.cwd.is_empty());
        assert!(input.hook_event_name.is_empty());
        assert!(input.tool_name.is_none());
        assert!(input.agent_id.is_none());
    }

    #[test]
    fn test_hook_input_serialization_roundtrip() {
        let input = HookInput {
            session_id: "ses_123".to_string(),
            cwd: "/home/user".to_string(),
            hook_event_name: EVENT_PRE_TOOL_USE.to_string(),
            tool_name: Some("Bash".to_string()),
            tool_input: Some(serde_json::json!({"command": "ls -la"})),
            ..Default::default()
        };
        let json = serde_json::to_string(&input).unwrap();
        let deserialized: HookInput = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "ses_123");
        assert_eq!(deserialized.tool_name, Some("Bash".to_string()));
        assert_eq!(deserialized.schema_version, "2.0");
    }

    #[test]
    fn test_hook_input_json_omits_none_fields() {
        let input = HookInput::default();
        let json = serde_json::to_string(&input).unwrap();
        // Optional fields should be serialized as null (serde default)
        assert!(json.contains("\"schema_version\":\"2.0\""));
    }

    // --- HookInputBuilder ---

    #[test]
    fn test_builder_session_and_event() {
        let input = HookInputBuilder::new()
            .session("ses_456", "/tmp/project")
            .event("SessionStart")
            .build();
        assert_eq!(input.session_id, "ses_456");
        assert_eq!(input.cwd, "/tmp/project");
        assert_eq!(input.hook_event_name, "SessionStart");
    }

    #[test]
    fn test_builder_tool() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("PreToolUse")
            .tool("Bash", serde_json::json!({"command": "ls"}), "tool_1")
            .build();
        assert_eq!(input.tool_name, Some("Bash".to_string()));
        assert_eq!(input.tool_use_id, Some("tool_1".to_string()));
        assert!(input.tool_input.is_some());
    }

    #[test]
    fn test_builder_tool_output() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("PostToolUse")
            .tool("Read", serde_json::json!({"file": "main.rs"}), "tool_2")
            .tool_output(serde_json::json!({"content": "fn main() {}"}))
            .duration(42)
            .build();
        assert!(input.tool_output.is_some());
        assert_eq!(input.duration_ms, Some(42));
    }

    #[test]
    fn test_builder_agent() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("AgentStart")
            .agent("agent_alpha", "coder")
            .build();
        assert_eq!(input.agent_id, Some("agent_alpha".to_string()));
        assert_eq!(input.agent_type, Some("coder".to_string()));
    }

    #[test]
    fn test_builder_permission() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("PermissionRequest")
            .permission("auto", "req_001", "Execute bash command")
            .build();
        assert_eq!(input.permission_mode, Some("auto".to_string()));
        assert_eq!(input.request_id, Some("req_001".to_string()));
        assert_eq!(
            input.action_description,
            Some("Execute bash command".to_string())
        );
    }

    #[test]
    fn test_builder_error() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("ToolError")
            .error("command not found", 127)
            .build();
        assert_eq!(input.error, Some("command not found".to_string()));
        assert_eq!(input.error_code, Some(127));
    }

    #[test]
    fn test_builder_prompt() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("UserPromptSubmit")
            .prompt("fix the bug in main.rs")
            .build();
        assert_eq!(
            input.prompt_text,
            Some("fix the bug in main.rs".to_string())
        );
    }

    #[test]
    fn test_builder_full_chain() {
        let input = HookInputBuilder::new()
            .session("ses_full", "/workspace")
            .event("PreToolUse")
            .agent("agent_1", "default")
            .tool("Bash", serde_json::json!({"command": "cargo test"}), "tu_1")
            .duration(1500)
            .build();
        assert_eq!(input.session_id, "ses_full");
        assert_eq!(input.hook_event_name, "PreToolUse");
        assert_eq!(input.agent_id, Some("agent_1".to_string()));
        assert_eq!(input.tool_name, Some("Bash".to_string()));
        assert_eq!(input.duration_ms, Some(1500));
    }

    // --- HookOutput ---

    #[test]
    fn test_hook_output_continue() {
        let output = HookOutput::continue_();
        assert!(output.continue_);
        assert!(output.suppress_output.is_none());
        assert!(output.stop_reason.is_none());
        assert!(output.decision.is_none());
    }

    #[test]
    fn test_hook_output_block() {
        let output = HookOutput::block("Dangerous command");
        assert!(!output.continue_);
        assert_eq!(output.stop_reason.as_deref(), Some("Dangerous command"));
        assert_eq!(output.decision.as_deref(), Some("deny"));
    }

    #[test]
    fn test_hook_output_ask() {
        let output = HookOutput::ask("Need approval");
        assert!(!output.continue_);
        assert_eq!(output.decision.as_deref(), Some("ask"));
        assert_eq!(output.reason.as_deref(), Some("Need approval"));
    }

    #[test]
    fn test_hook_output_allow() {
        let output = HookOutput::allow();
        assert!(output.continue_);
        assert_eq!(output.decision.as_deref(), Some("allow"));
    }

    #[test]
    fn test_hook_output_serialization_roundtrip() {
        let output = HookOutput::block("nope");
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: HookOutput = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.continue_);
        assert_eq!(deserialized.stop_reason.as_deref(), Some("nope"));
    }

    #[test]
    fn test_hook_output_default_true_on_empty_json() {
        let json = r#"{}"#;
        let output: HookOutput = serde_json::from_str(json).unwrap();
        assert!(output.continue_);
        assert!(output.suppress_output.is_none());
    }

    #[test]
    fn test_hook_output_continue_false_from_json() {
        let json = r#"{"continue_": false, "stop_reason": "blocked"}"#;
        let output: HookOutput = serde_json::from_str(json).unwrap();
        assert!(!output.continue_);
        assert_eq!(output.stop_reason.as_deref(), Some("blocked"));
    }

    // --- HookSpecificOutput ---

    #[test]
    fn test_hook_specific_output_serialization() {
        let specific = HookSpecificOutput {
            hook_event_name: "PreToolUse".to_string(),
            permission_decision: None,
            permission_decision_reason: None,
            updated_input: Some(serde_json::json!({"command": "safe-ls"})),
            updated_prompt: None,
            updated_system_prompt: None,
            updated_system_message: None,
            additional_context: None,
            additional_env_vars: None,
            updated_config: None,
        };
        let json = serde_json::to_string(&specific).unwrap();
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("safe-ls"));
    }

    #[test]
    fn test_hook_specific_output_permission() {
        let specific = HookSpecificOutput {
            hook_event_name: "PermissionRequest".to_string(),
            permission_decision: Some("allow".to_string()),
            permission_decision_reason: Some("Safe read operation".to_string()),
            updated_input: None,
            updated_prompt: None,
            updated_system_prompt: None,
            updated_system_message: None,
            additional_context: None,
            additional_env_vars: None,
            updated_config: None,
        };
        assert_eq!(specific.permission_decision.as_deref(), Some("allow"));
    }

    // --- HookResult ---

    #[test]
    fn test_hook_result_variants() {
        let continue_result = HookResult::Continue(HookOutput::continue_());
        assert!(matches!(continue_result, HookResult::Continue(_)));

        let blocked_result = HookResult::Blocked {
            reason: "nope".to_string(),
            output: HookOutput::block("nope"),
        };
        assert!(matches!(blocked_result, HookResult::Blocked { .. }));

        let failed_result = HookResult::Failed {
            error: "timeout".to_string(),
        };
        assert!(matches!(failed_result, HookResult::Failed { .. }));
    }

    // --- AggregatedDecision ---

    #[test]
    fn test_aggregated_decision_allow() {
        let decision = AggregatedDecision::Allow;
        assert!(matches!(decision, AggregatedDecision::Allow));
    }

    #[test]
    fn test_aggregated_decision_ask() {
        let decision = AggregatedDecision::Ask {
            reasons: vec!["needs review".to_string()],
        };
        if let AggregatedDecision::Ask { reasons } = decision {
            assert_eq!(reasons.len(), 1);
        } else {
            panic!("expected Ask variant");
        }
    }

    #[test]
    fn test_aggregated_decision_deny() {
        let decision = AggregatedDecision::Deny {
            reason: "forbidden".to_string(),
            source_hook: "security_hook".to_string(),
        };
        if let AggregatedDecision::Deny {
            reason,
            source_hook,
        } = decision
        {
            assert_eq!(reason, "forbidden");
            assert_eq!(source_hook, "security_hook");
        } else {
            panic!("expected Deny variant");
        }
    }

    // --- HookMetrics ---

    #[test]
    fn test_hook_metrics_serialization() {
        let metrics = HookMetrics {
            event_name: "PreToolUse".to_string(),
            handler_label: "security_check.sh".to_string(),
            execution_count: 100,
            failure_count: 2,
            blocked_count: 5,
            total_duration_ms: 42000,
            avg_duration_ms: 420.0,
            last_execution: Some(Utc::now()),
            last_error: None,
        };
        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("PreToolUse"));
        assert!(json.contains("security_check.sh"));

        let deserialized: HookMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.execution_count, 100);
        assert_eq!(deserialized.failure_count, 2);
        assert_eq!(deserialized.blocked_count, 5);
    }

    #[test]
    fn test_hook_metrics_with_error() {
        let metrics = HookMetrics {
            event_name: "PostToolUse".to_string(),
            handler_label: "logger.sh".to_string(),
            execution_count: 50,
            failure_count: 1,
            blocked_count: 0,
            total_duration_ms: 15000,
            avg_duration_ms: 300.0,
            last_execution: Some(Utc::now()),
            last_error: Some("exit code 1".to_string()),
        };
        assert_eq!(metrics.last_error.as_deref(), Some("exit code 1"));
    }

    // --- Full protocol roundtrip ---

    #[test]
    fn test_full_protocol_roundtrip() {
        let input = HookInputBuilder::new()
            .session("ses_proto", "/workspace")
            .event("PreToolUse")
            .agent("coder", "default")
            .tool(
                "Write",
                serde_json::json!({"file_path": "main.rs", "content": "fn main() {}"}),
                "tool_99",
            )
            .build();

        // Serialize to JSON (what the hook receives via stdin)
        let json = serde_json::to_string_pretty(&input).unwrap();

        // Deserialize back
        let parsed: HookInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, "2.0");
        assert_eq!(parsed.session_id, "ses_proto");
        assert_eq!(parsed.hook_event_name, "PreToolUse");
        assert_eq!(parsed.tool_name, Some("Write".to_string()));

        // Build a response
        let output = HookOutput::allow();
        let output_json = serde_json::to_string(&output).unwrap();
        let parsed_output: HookOutput = serde_json::from_str(&output_json).unwrap();
        assert!(parsed_output.continue_);
        assert_eq!(parsed_output.decision.as_deref(), Some("allow"));
    }
}
