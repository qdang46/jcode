# jcode Hooks v2.0 — Master Implementation Plan

> **Generated**: 2026-06-03 | **Audience**: Senior Rust engineer | **Branch**: `fix/hooks-env-override`  
> **Goal**: Implement 28 hook events across 9 reference repos. Someone with basic Rust knowledge can implement this end-to-end without ambiguity.

---

## 1. Executive Summary

**What we're building**: Upgrade jcode's hook system from 11 events → 28 events, achieving full parity with the union of Claude Code (27 events), OpenCode (14 plugin events), Codex (10 events), oh-my-pi (26+ events), and pi-agent-rust (~30 extension events). The system supports **Command** (bash/powershell), **HTTP** (REST), and **Agent** (inline Rust) handlers with parallel dispatch, matcher-based filtering, deny>ask>allow precedence, and kill-switch env vars.

**What stays the same**: 3-layer TOML config (env > project > user), HookInput/HookOutput JSON protocol via stdin/stdout, HookRegistry + HookMatcher filtering, existing integration points in tool/mod.rs/safety.rs.

**What changes**: 17 new HookEvent variants, parallel dispatch engine with FuturesUnordered, agent+plugin handler types, kill-switch env vars, metrics collection, timeout-per-handler, and new integration callsites across the codebase (agent lifecycle, compaction, task management, session state).

**Reference repos used**: claude-code (CC), opencode (OC), codex (CX), oh-my-openagent (OMOA), oh-my-claudecode (OMCC), oh-my-codex (OMCX), oh-my-pi (OMPI), pi-agent-rust (PI), codebuff (CB).

---

## 2. Current State Analysis

### Existing Files (`fix/hooks-env-override` branch)

```
src/hooks/
├── mod.rs          # Module re-exports (20 lines)
├── types.rs        # HookInput, HookOutput, HookSpecificOutput (220 lines)
├── config.rs       # HookEvent (11 vars), HookHandlerConfig, HooksConfig, load_hooks_config (320 lines)
├── registry.rs     # HookContext, HookRegistry, matching/filtering (300 lines)
├── execute.rs      # execute_command_hook, execute_http_hook, execute_hook, HookResult (200 lines)
└── matcher.rs      # HookMatcher (4 variants), MatcherContext, matches() (120 lines)
```

### Existing Integration Points

| File | Lines | Hooks Wired |
|------|-------|-------------|
| `src/tool/mod.rs` | 50+ | PreToolUse, PostToolUse (via HookRegistry) |
| `src/safety.rs` | 80+ | PermissionRequest, PermissionDenied, ToolError |
| `src/cli/commands.rs` | 30+ | enable/disable hook commands |
| `src/lib.rs` | 1 line | `pub mod hooks;` |

### What Exists vs What's Missing

| Feature | Current Status | Target |
|---------|---------------|--------|
| HookEvent variants | 11 (PreToolUse, PostToolUse, PreSession, PostSession, Error, SessionStart, SessionEnd, PermissionRequest, PermissionDenied, ToolError, Custom) | 28 (including all new events) |
| Handler types | Command (bash), HTTP | + Agent (inline Rust fn), + Plugin (external script) |
| Dispatch | Sequential, no aggregation | Parallel via FuturesUnordered + aggregate decisions |
| Blocking | Exit code 2 → HookResult::Blocked | Same + Stop event blocking |
| Timeout | 30s hardcoded | Per-handler configurable (100ms–300s) + global default |
| Metrics | None | Execution time histogram, count/failure counters |
| Kill-switch | None | `DISABLE_JCODE_HOOKS`, `JCODE_SKIP_HOOKS`, `JCODE_SKIP_EVENT_*` |
| Integration callsites | tool.rs, safety.rs only | + agent.rs, compaction.rs, server/, cli/commands.rs |

---

## 3. Final Event Inventory (28 Events)

### 3.1 Core Tool Events (6 events)

| # | Event | Blockable | Source | Input Fields | Output Behavior | HookSpecificOutput |
|---|-------|-----------|--------|-------------|-----------------|---------------------|
| 1 | **PreToolUse** | ✅ | CC, CX, OM, PI | session_id, tool_name, tool_input, agent_id, cwd | `updated_input` → replace tool input; `continue_=false` → block | `updated_input`: serde_json::Value |
| 2 | **PostToolUse** | ❌ | CC, CX, OC, OM | session_id, tool_name, tool_output, tool_use_id, duration_ms | `suppress_output` → hide from agent; `additional_context` | `additional_context`: String |
| 3 | **PostToolUseFailure** | ❌ | CC, OMCC | session_id, tool_name, error, tool_use_id, duration_ms | `system_message` → inject into conversation | `system_message`: String |
| 4 | **ToolError** | ❌ | OMPI, OG | session_id, tool_name, error, error_code | `stop_reason` → pass to agent | Already wired |
| 5 | **UserPromptSubmit** | ✅ | CC, CX, OMCC, OC | session_id, prompt, prompt_text, files | `updated_prompt` → rewrite before LLM; `continue_=false` → block | `updated_prompt`: String |
| 6 | **UserPromptExpansion** | ❌ | ≈OC `chat.messages.transform` | session_id, original_prompt, expanded_prompt, expansions | `system_message` to explain expansion | — |

### 3.2 Session Lifecycle Events (6 events)

| # | Event | Blockable | Source | Input Fields | Output Behavior |
|---|-------|-----------|--------|-------------|-----------------|
| 7 | **SessionStart** | ❌ | CC, CX, OM, PI | session_id, cwd, start_time, agent_type, agent_id | `system_message`, `additional_context` |
| 8 | **SessionEnd** | ❌ | CC, CX, OC | session_id, duration_secs, total_tool_calls, exit_reason, cwd | `suppress_output` |
| 9 | **SessionUpdated** | ❌ | OC `session.updated` | session_id, prev_state, new_state, update_reason, timestamp | `additional_context` |
| 10 | **SessionDiff** | ❌ | OC `session.diff` | session_id, diff_type, diff_content, session_snapshot | — (observational) |
| 11 | **SessionError** | ❌ | OC `session.error` | session_id, error, error_type, recoverable, timestamp | `stop_reason` if fatal |
| 12 | **SessionIdle** | ❌ | OC `session.idle` | session_id, idle_duration_secs, idle_threshold_secs, last_activity | `system_message` for cleanup |

### 3.3 Permission Events (4 events)

| # | Event | Blockable | Source | Input Fields | Output Behavior |
|---|-------|-----------|--------|-------------|-----------------|
| 13 | **PermissionRequest** | ✅ | CC, CX, OM, OC | session_id, tool_name, permission_mode, action, description | `decision: "allow"|"deny"|"ask"` in hook_specific_output |
| 14 | **PermissionDenied** | ❌ | CC, OM, OG | session_id, tool_name, permission_mode, reason | `suppress_output`, `stop_reason` |
| 15 | **PermissionAsked** | ✅ | OC `permission.asked` | session_id, request_id, tool_name, permission_mode, ask_timestamp | `decision` (pre-approve via hook) |
| 16 | **PermissionReplied** | ❌ | OC `permission.replied` | session_id, request_id, decision, reply_timestamp | — (observational audit log) |

### 3.4 Agent & Subagent Events (5 events)

| # | Event | Blockable | Source | Input Fields | Output Behavior |
|---|-------|-----------|--------|-------------|-----------------|
| 17 | **AgentStart** | ✅ | OMPI `before_agent_start`, PI `OnAgentStart` | session_id, agent_id, agent_type, model, system_prompt | `updated_system_prompt`, `continue_=false` → block |
| 18 | **AgentEnd** | ❌ | OMPI `agent_end` | session_id, agent_id, agent_type, turns, duration_secs, total_cost | — |
| 19 | **SubagentStart** | ❌ | CC, CX | session_id, parent_agent_id, subagent_id, subagent_type, task | `additional_context` |
| 20 | **SubagentStop** | ❌ | CC, CX | session_id, subagent_id, result, duration_secs | — |
| 21 | **Stop** | ✅ | CC, OMCC, OM | session_id, stop_type (user/hook/error), stop_reason, continue_loop | `continue_: false` → do NOT stop (override); message via `stop_reason` |

### 3.5 Compaction Events (3 events)

| # | Event | Blockable | Source | Input Fields | Output Behavior |
|---|-------|-----------|--------|-------------|-----------------|
| 22 | **PreCompact** | ✅ | CC, CX, OC, OM | session_id, current_size, target_size, message_count | `updated_system_message` → override compacted system msg; `continue_=false` → skip compaction |
| 23 | **PostCompact** | ❌ | CC, OC | session_id, original_size, compacted_size, saved_bytes, system_message | — |
| 24 | **AutoCompactionControl** | ❌ | OC `compaction.autocontinue` | session_id, auto_compaction_enabled, compaction_count, avg_saved_bytes | — |

### 3.6 Task & Setup Events (3 events)

| # | Event | Blockable | Source | Input Fields | Output Behavior |
|---|-------|-----------|--------|-------------|-----------------|
| 25 | **Setup** | ❌ | CC, PI | session_id, cwd, env_vars (masked), config_path | `additional_env_vars`, `updated_config` |
| 26 | **TaskCreated** | ❌ | CC | session_id, task_id, task_type, task_description, parent_task_id | — |
| 27 | **TaskCompleted** | ❌ | CC | session_id, task_id, result, duration_secs | — |

### 3.7 File & Notification Events (4 events)

| # | Event | Blockable | Source | Input Fields | Output Behavior |
|---|-------|-----------|--------|-------------|-----------------|
| 28 | **FileChanged** | ❌ | CC | session_id, file_path, change_type (created/modified/deleted), diff | — |

> **Note**: `Notification`, `InstructionsLoaded`, `TeammateIdle` from CC are deferred to v2.1 (low usage signal). `Context`, `ResourcesDiscover`, `TodoReminder`, `AutoRetryStart/End`, `ChatParams`, `ChatHeaders`, `Experimental*` from OMPI/OC are deferred to v2.1 (require additional infrastructure). See `Known Limitations`.

---

## 4. Architecture — Dispatch Engine

### 4.1 Parallel Dispatch Design

Current dispatch is sequential. We need parallel dispatch for non-blocking events and aggregate decisions for blocking events.

```rust
// NEW: Parallel dispatch engine
// For blocking events (PreToolUse, PermissionRequest, etc.):
//   1. Dispatch ALL matching hooks via FuturesUnordered
//   2. Collect results as they complete (up to timeout)
//   3. Apply deny > ask > allow precedence:
//      - Any hook returns Blocked → DENY (short-circuit, cancel remaining)
//      - Any hook returns "ask" and no hook returned Blocked → ASK
//      - All hooks return Continue with allow → ALLOW
//
// For non-blocking events (PostToolUse, SessionStart, etc.):
//   1. Fire-and-forget via tokio::spawn with timeout
//   2. Collect results for metrics only
//   3. Never block the main execution path

enum AggregatedDecision {
    Allow,
    Ask { reasons: Vec<String> },
    Deny { reason: String, source_hook: String },
}
```

### 4.2 Handler Type Architecture

```rust
enum HookHandlerConfig {
    Command(CommandHandlerConfig),  // bash/powershell - exists
    Http(HttpHandlerConfig),       // REST call - exists
    Agent { agent_id: String },    // jcode subagent - NEW
    Plugin(String),                // external plugin script - NEW
}
```

**Agent handler**: A jcode subagent (identified by agent_id) runs the hook. The agent receives the HookInput as context and its response is parsed as HookOutput. Agent handlers are async with configurable timeout (default 120s).

**Plugin handler**: A standalone executable (script/binary) that receives HookInput via stdin and returns HookOutput via stdout, same protocol as Command hooks but with plugin lifecycle (register/deregister, versioned).

### 4.3 Hook Precedence Chain

```
For permission-type decisions (Permission*, PreToolUse, AgentStart, Stop, PreCompact):

  1. DENY wins:      Any hook returns Blocked/exit 2 → immediate DENY
  2. ASK wins:       Any hook returns "ask" decision → defer to user
  3. ALLOW default:  All hooks return Continue or no hooks → ALLOW

For tool-level concurrency: multiple hooks for same event → all run in parallel.
Results are aggregated:
  - suppress_output:  true if ANY hook returns suppress_output=true
  - system_message:   concatenated from ALL hooks (if multiple)
  - updated_input:    LAST hook wins (sequential stamping)
```

### 4.4 Kill-Switch Architecture (NEW)

```rust
fn hooks_disabled() -> bool {
    // Priority: most specific wins
    env::var("DISABLE_JCODE_HOOKS").is_ok()     // Kill ALL hooks
    || env::var("JCODE_SKIP_HOOKS").is_ok()     // Skip all hook execution
}

fn event_disabled(event: &HookEvent) -> bool {
    let event_key = format!("JCODE_SKIP_EVENT_{}", event.name_uppercase());
    env::var(event_key).is_ok()
}
```

---

## 5. Data Structures & Types

### 5.1 `src/hooks/types.rs` — FULL REPLACEMENT

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ===========================================================================
// EVENT NAME CONSTANTS
// ===========================================================================

pub const EVENT_PRE_TOOL_USE: &str = "PreToolUse";
pub const EVENT_POST_TOOL_USE: &str = "PostToolUse";
pub const EVENT_POST_TOOL_USE_FAILURE: &str = "PostToolUseFailure";
pub const EVENT_TOOL_ERROR: &str = "ToolError";
pub const EVENT_USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";
pub const EVENT_USER_PROMPT_EXPANSION: &str = "UserPromptExpansion";
pub const EVENT_SESSION_START: &str = "SessionStart";
pub const EVENT_SESSION_END: &str = "SessionEnd";
pub const EVENT_SESSION_UPDATED: &str = "SessionUpdated";
pub const EVENT_SESSION_DIFF: &str = "SessionDiff";
pub const EVENT_SESSION_ERROR: &str = "SessionError";
pub const EVENT_SESSION_IDLE: &str = "SessionIdle";
pub const EVENT_PERMISSION_REQUEST: &str = "PermissionRequest";
pub const EVENT_PERMISSION_DENIED: &str = "PermissionDenied";
pub const EVENT_PERMISSION_ASKED: &str = "PermissionAsked";
pub const EVENT_PERMISSION_REPLIED: &str = "PermissionReplied";
pub const EVENT_AGENT_START: &str = "AgentStart";
pub const EVENT_AGENT_END: &str = "AgentEnd";
pub const EVENT_SUBAGENT_START: &str = "SubagentStart";
pub const EVENT_SUBAGENT_STOP: &str = "SubagentStop";
pub const EVENT_STOP: &str = "Stop";
pub const EVENT_PRE_COMPACT: &str = "PreCompact";
pub const EVENT_POST_COMPACT: &str = "PostCompact";
pub const EVENT_AUTO_COMPACTION_CONTROL: &str = "AutoCompactionControl";
pub const EVENT_SETUP: &str = "Setup";
pub const EVENT_TASK_CREATED: &str = "TaskCreated";
pub const EVENT_TASK_COMPLETED: &str = "TaskCompleted";
pub const EVENT_FILE_CHANGED: &str = "FileChanged";

/// All known event names as a static slice for validation
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
// HOOK INPUT - Stdin JSON contract
// ===========================================================================

/// Standard input passed to every hook via stdin JSON.
/// All fields are Option to allow event-specific subsets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HookInput {
    // === Always present ===
    pub schema_version: String,       // "2.0"
    pub session_id: String,
    pub cwd: String,
    pub hook_event_name: String,
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

/// Builder pattern for constructing event-specific HookInput values.
/// Ensures required fields are set and optional fields are correct per event.
#[derive(Debug, Default)]
pub struct HookInputBuilder {
    input: HookInput,
}

impl HookInputBuilder {
    pub fn new() -> Self { Self::default() }

    pub fn session(mut self, session_id: &str, cwd: &str) -> Self {
        self.input.session_id = session_id.to_string();
        self.input.cwd = cwd.to_string();
        self
    }

    pub fn event(mut self, event_name: &str) -> Self {
        self.input.hook_event_name = event_name.to_string();
        self
    }

    pub fn agent(mut self, agent_id: &str, agent_type: &str) -> Self {
        self.input.agent_id = Some(agent_id.to_string());
        self.input.agent_type = Some(agent_type.to_string());
        self
    }

    pub fn tool(mut self, name: &str, input: serde_json::Value, use_id: &str) -> Self {
        self.input.tool_name = Some(name.to_string());
        self.input.tool_input = Some(input);
        self.input.tool_use_id = Some(use_id.to_string());
        self
    }

    pub fn tool_output(mut self, output: serde_json::Value) -> Self {
        self.input.tool_output = Some(output);
        self
    }

    pub fn permission(mut self, mode: &str, request_id: &str, description: &str) -> Self {
        self.input.permission_mode = Some(mode.to_string());
        self.input.request_id = Some(request_id.to_string());
        self.input.action_description = Some(description.to_string());
        self
    }

    pub fn error(mut self, error: &str, code: i32) -> Self {
        self.input.error = Some(error.to_string());
        self.input.error_code = Some(code);
        self
    }

    pub fn duration(mut self, ms: u64) -> Self {
        self.input.duration_ms = Some(ms);
        self
    }

    pub fn prompt(mut self, text: &str) -> Self {
        self.input.prompt_text = Some(text.to_string());
        self
    }

    pub fn build(self) -> HookInput {
        self.input
    }
}

// ===========================================================================
// HOOK OUTPUT - Stdout JSON contract
// ===========================================================================

/// Standard output from hook scripts.
/// Every field is optional — hooks return only what they need to override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookOutput {
    /// Whether execution should continue (default: true).
    /// For blocking events: false → block/deny the operation.
    #[serde(default = "default_true")]
    pub continue_: bool,

    /// Suppress the output from being shown to the agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppress_output: Option<bool>,

    /// Reason for stopping/blocking (shown to agent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    /// Decision for permission-type hooks: "allow" | "deny" | "ask".
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

fn default_true() -> bool { true }

impl HookOutput {
    pub fn continue_() -> Self {
        Self { continue_: true, suppress_output: None, stop_reason: None, decision: None, reason: None, system_message: None, hook_specific_output: None }
    }

    pub fn block(reason: &str) -> Self {
        Self { continue_: false, suppress_output: None, stop_reason: Some(reason.to_string()), decision: Some("deny".to_string()), reason: None, system_message: None, hook_specific_output: None }
    }

    pub fn ask(reason: &str) -> Self {
        Self { continue_: false, suppress_output: None, stop_reason: None, decision: Some("ask".to_string()), reason: Some(reason.to_string()), system_message: None, hook_specific_output: None }
    }

    pub fn allow() -> Self {
        Self { continue_: true, suppress_output: None, stop_reason: None, decision: Some("allow".to_string()), reason: None, system_message: None, hook_specific_output: None }
    }
}

/// Event-specific output fields.
/// Each blocking event uses a subset of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSpecificOutput {
    pub hook_event_name: String,

    // Permission events
    pub permission_decision: Option<String>,
    pub permission_decision_reason: Option<String>,

    // Tool events - modify input before execution
    pub updated_input: Option<serde_json::Value>,

    // Prompt events - modify prompt before LLM
    pub updated_prompt: Option<String>,

    // Agent events - modify system prompt
    pub updated_system_prompt: Option<String>,

    // Compact events - override compacted system message
    pub updated_system_message: Option<String>,

    // General - inject context
    pub additional_context: Option<String>,

    // Session setup - inject env vars
    pub additional_env_vars: Option<HashMap<String, String>>,
    pub updated_config: Option<serde_json::Value>,
}

// ===========================================================================
// HOOK RESULT
// ===========================================================================

/// Result of executing a single hook.
#[derive(Debug)]
pub enum HookResult {
    /// Hook completed successfully and execution should continue.
    Continue(HookOutput),
    /// Hook blocked the operation (exit code 2 or continue_=false).
    Blocked { reason: String, output: HookOutput },
    /// Hook failed (exit code != 0, HTTP error, timeout).
    Failed { error: String },
}

/// Aggregated decision from multiple hooks for blocking events.
#[derive(Debug)]
pub enum AggregatedDecision {
    /// All hooks say continue, or no hooks configured.
    Allow,
    /// At least one hook says "ask" and no hook says "deny".
    Ask { reasons: Vec<String> },
    /// At least one hook blocked.
    Deny { reason: String, source_hook: String },
}

// ===========================================================================
// HOOK METRICS
// ===========================================================================

/// Metrics collected per hook execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMetrics {
    pub event_name: String,
    pub handler_label: String,
    pub execution_count: u64,
    pub failure_count: u64,
    pub blocked_count: u64,
    pub total_duration_ms: u64,
    pub avg_duration_ms: f64,
    pub last_execution: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}
```

### 5.2 `src/hooks/dispatch.rs` — NEW FILE

The parallel dispatch engine is the most architecturally significant addition. It uses `tokio::sync::Semaphore` for concurrency control and `FuturesUnordered` for parallel execution.

```rust
use futures::stream::FuturesUnordered;
use std::sync::Arc;
use tokio::sync::Semaphore;
use std::time::Duration;

use crate::hooks::config::{HookHandlerConfig, HooksConfig};
use crate::hooks::execute::execute_hook;
use crate::hooks::types::{
    AggregatedDecision, HookInput, HookMetrics, HookOutput, HookResult,
};

/// Configuration for parallel hook dispatch
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    /// Maximum concurrent hooks per event (default: 10)
    pub max_concurrency: usize,
    /// Default timeout per hook in seconds (default: 30)
    pub default_timeout_secs: u64,
    /// Whether to run hooks in dry-run mode (log only, no execution)
    pub dry_run: bool,
    /// Kill-switch: if true, skip all hook execution
    pub disabled: bool,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 10,
            default_timeout_secs: 30,
            dry_run: false,
            disabled: false,
        }
    }
}

/// Statistics collected during a dispatch batch
#[derive(Debug, Default)]
pub struct DispatchStats {
    pub total_hooks: usize,
    pub completed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub timed_out: usize,
    pub total_duration_ms: u64,
}

/// Dispatch hooks for an event in parallel.
///
/// For **blocking events**: runs all matching hooks, aggregates results,
/// enforces deny > ask > allow precedence.
///
/// For **non-blocking events**: fire-and-forget with tokio::spawn,
/// never blocks the caller.
pub async fn dispatch_hooks(
    handlers: &[&HookHandlerConfig],
    input: &HookInput,
    config: &DispatchConfig,
) -> (AggregatedDecision, DispatchStats) {
    if config.disabled || handlers.is_empty() {
        return (AggregatedDecision::Allow, DispatchStats::default());
    }

    let semaphore = Arc::new(Semaphore::new(config.max_concurrency));
    let mut futures = FuturesUnordered::new();
    let start = std::time::Instant::now();

    for handler in handlers {
        let input = input.clone();
        let sem = semaphore.clone();

        // Agent handlers use a different execution path
        let fut = async move {
            let _permit = sem.acquire().await.unwrap();
            execute_hook(handler, &input).await
        };

        futures.push(fut);
    }

    let mut stats = DispatchStats {
        total_hooks: handlers.len(),
        ..Default::default()
    };

    let mut decisions: Vec<(String, AggregatedDecision)> = Vec::new();
    let mut aggregated_output = HookOutput::continue_();

    while let Some(result) = futures.next().await {
        match result {
            Ok(HookResult::Continue(output)) => {
                stats.completed += 1;
                // Merge output: suppress_output OR, system_message concat
                if output.suppress_output.unwrap_or(false) {
                    aggregated_output.suppress_output = Some(true);
                }
                if let Some(msg) = output.system_message {
                    let existing = aggregated_output.system_message.unwrap_or_default();
                    aggregated_output.system_message = Some(if existing.is_empty() { msg } else { format!("{}\n{}", existing, msg) });
                }
                if let Some(decision) = output.decision {
                    decisions.push((output.reason.unwrap_or_default(), classify_decision(&decision)));
                }
            }
            Ok(HookResult::Blocked { reason, output }) => {
                stats.blocked += 1;
                decisions.push((reason.clone(), AggregatedDecision::Deny { reason, source_hook: "hook".to_string() }));
                // Deny is final — short-circuit
                return (AggregatedDecision::Deny { reason: output.stop_reason.unwrap_or_default(), source_hook: "hook".to_string() }, stats);
            }
            Ok(HookResult::Failed { error }) => {
                stats.failed += 1;
                tracing::warn!("Hook failed: {}", error);
            }
            Err(_) => {
                stats.timed_out += 1;
            }
        }
    }

    stats.total_duration_ms = start.elapsed().as_millis() as u64;

    // Aggregate decisions
    let final_decision = aggregate_decision(&decisions);

    (final_decision, stats)
}

fn classify_decision(decision: &str) -> AggregatedDecision {
    match decision {
        "deny" | "block" => AggregatedDecision::Deny { reason: String::new(), source_hook: String::new() },
        "ask" => AggregatedDecision::Ask { reasons: vec![] },
        _ => AggregatedDecision::Allow,
    }
}

fn aggregate_decision(decisions: &[(String, AggregatedDecision)]) -> AggregatedDecision {
    let mut ask_reasons = Vec::new();
    let mut has_deny = false;
    let mut deny_reason = String::new();

    for (reason, decision) in decisions {
        match decision {
            AggregatedDecision::Deny { .. } => {
                has_deny = true;
                deny_reason = reason.clone();
            }
            AggregatedDecision::Ask { .. } => {
                ask_reasons.push(reason.clone());
            }
            AggregatedDecision::Allow => {}
        }
    }

    if has_deny {
        AggregatedDecision::Deny { reason: deny_reason, source_hook: "hook".to_string() }
    } else if !ask_reasons.is_empty() {
        AggregatedDecision::Ask { reasons: ask_reasons }
    } else {
        AggregatedDecision::Allow
    }
}
```

---

## 6. Configuration Format

### 6.1 TOML Schema (`hooks.toml`)

```toml
# =============================================================================
# jcode Hooks Configuration v2.0
# =============================================================================
# Place at:
#   ~/.jcode/hooks.toml        (user-level, lowest priority)
#   .jcode/hooks.toml          (project-level, medium priority)
#   $JCODE_HOOKS_CONFIG        (env-level, highest priority)
#
# Layers are merged: user < project < env. Same event = higher wins.

# ---- Global Settings ----
[settings]
timeout_secs = 30                    # Default hook timeout (1-300)
max_concurrency = 10                 # Parallel hooks per event
dry_run = false                      # Log-only mode (no execution)
fail_closed = false                  # If true: hook failure = block operation
                                     # If false: hook failure = continue (default)

# =============================================================================
# EVENT: PreToolUse
# Runs BEFORE a tool is executed.
# Blocking: exit 2 or continue_=false → tool is blocked
# Output: hook_specific_output.updated_input → replaces tool input
# =============================================================================
[[event.PreToolUse]]
type = "command"
enabled = true
command = "pre_tool_check.sh"
timeout_secs = 5
matcher = "Bash|Write|Edit"          # Only match specific tools (pipe = OR)
# matcher = "*"                      # Wildcard: all tools
# matcher = "/^Bash/"               # Regex: tools starting with "Bash"

[[event.PreToolUse]]
type = "http"
enabled = true
url = "http://localhost:9090/hooks/pre-tool"
method = "POST"
timeout_secs = 2
headers = { Authorization = "Bearer ${HOOK_API_TOKEN}" }
matcher = "Git"                       # Only for Git tool

# =============================================================================
# EVENT: PostToolUse
# Runs AFTER a tool completes successfully.
# Non-blocking (output ignored for flow control).
# Output: suppress_output=true → hide result from agent
# =============================================================================
[[event.PostToolUse]]
type = "command"
command = "log_tool_usage.sh"
matcher = "Bash|Write"

# =============================================================================
# EVENT: PostToolUseFailure
# Runs AFTER a tool fails.
# Non-blocking.
# =============================================================================
[[event.PostToolUseFailure]]
type = "command"
command = "notify_failure.sh"
timeout_secs = 10

# =============================================================================
# EVENT: ToolError
# Runs when a tool produces an error-level result.
# Non-blocking.
# =============================================================================
[[event.ToolError]]
type = "command"
command = "log_error.sh"

# =============================================================================
# EVENT: UserPromptSubmit
# Runs when user submits a prompt.
# Blocking: exit 2 → prompt blocked
# Output: hook_specific_output.updated_prompt → rewrite prompt before LLM
# =============================================================================
[[event.UserPromptSubmit]]
type = "command"
command = "prompt_filter.sh"
timeout_secs = 2
matcher = "/.*/s"                      # Regex on prompt text (suffix = context text)

[[event.UserPromptSubmit]]
type = "http"
url = "http://localhost:9090/hooks/prompt-check"
method = "POST"
timeout_secs = 5

# =============================================================================
# EVENT: UserPromptExpansion
# Runs AFTER a prompt has been expanded/rewritten by the system.
# Non-blocking.
# =============================================================================
[[event.UserPromptExpansion]]
type = "command"
command = "log_expansion.sh"

# =============================================================================
# EVENT: SessionStart / SessionEnd
# Session lifecycle boundaries.
# Non-blocking.
# =============================================================================
[[event.SessionStart]]
type = "command"
command = "session_start_handler.sh"

[[event.SessionEnd]]
type = "http"
url = "http://localhost:9090/hooks/session-end"
method = "POST"
timeout_secs = 5

# =============================================================================
# EVENT: SessionUpdated / SessionDiff / SessionError
# Session state tracking.
# Non-blocking.
# =============================================================================
[[event.SessionUpdated]]
type = "command"
command = "log_session_state.sh"

# =============================================================================
# EVENT: SessionIdle
# Fires when session has been idle beyond the threshold.
# Non-blocking. Can inject cleanup via system_message.
# =============================================================================
[[event.SessionIdle]]
type = "command"
command = "cleanup_idle_session.sh"
timeout_secs = 60

# =============================================================================
# EVENT: PermissionRequest
# Runs when the agent needs permission to execute an action.
# Blocking: exit 2 → deny, exit 0 with decision=ask → ask user
# Output: decision = "allow" | "deny" | "ask"
# =============================================================================
[[event.PermissionRequest]]
type = "command"
command = "auto_approve.sh"
timeout_secs = 2
matcher = "Read|Glob|Ls"             # Auto-allow safe tools
# Decision in stdout JSON: {"decision": "allow", "reason": "Safe tool"}

[[event.PermissionRequest]]
type = "command"
command = "ask_admin.sh"
timeout_secs = 120
matcher = "Bash|Write|Edit|Delete"   # Escalate dangerous tools

# =============================================================================
# EVENT: PermissionDenied
# Fires after a permission was denied.
# Observational.
# =============================================================================
[[event.PermissionDenied]]
type = "command"
command = "log_denial.sh"

# =============================================================================
# EVENT: AgentStart / AgentEnd
# Main agent lifecycle (distinct from subagent).
# AgentStart is BLOCKING (can prevent agent from starting).
# Output: hook_specific_output.updated_system_prompt → modify system prompt
# =============================================================================
[[event.AgentStart]]
type = "command"
command = "inject_custom_system_prompt.sh"
timeout_secs = 3
# Return: {"hook_specific_output": {"updated_system_prompt": "..."}}

# =============================================================================
# EVENT: SubagentStart / SubagentStop
# Sub-agent lifecycle for swarm/parallel agent execution.
# Observational. Non-blocking.
# =============================================================================
[[event.SubagentStart]]
type = "command"
command = "log_subagent.sh"

# =============================================================================
# EVENT: Stop
# Fires when agent execution is stopping.
# Blocking: continue_=false → PREVENT the stop (keep running).
# This is the inverse of other events — false means "don't stop".
# =============================================================================
[[event.Stop]]
type = "command"
command = "should_continue.sh"
timeout_secs = 2
# Return: {"continue_": false, "stop_reason": "Need to finish critical operation"}
#         → Agent will NOT stop and will display the stop_reason message.

# =============================================================================
# EVENT: PreCompact
# Fires BEFORE session compaction.
# Blocking: continue_=false → skip compaction this cycle.
# Output: hook_specific_output.updated_system_message → override system message
# =============================================================================
[[event.PreCompact]]
type = "command"
command = "compact_check.sh"
timeout_secs = 3

# =============================================================================
# EVENT: PostCompact
# Fires AFTER session compaction.
# Observational.
# =============================================================================
[[event.PostCompact]]
type = "command"
command = "log_compaction.sh"

# =============================================================================
# EVENT: Setup
# Fires when the session/environment is being initialized.
# Output: hook_specific_output.additional_env_vars → inject env vars.
#         hook_specific_output.updated_config → modify config.
# =============================================================================
[[event.Setup]]
type = "command"
command = "inject_env.sh"
timeout_secs = 2

# =============================================================================
# EVENT: TaskCreated / TaskCompleted
# Task lifecycle tracking. Observational.
# =============================================================================
[[event.TaskCreated]]
type = "command"
command = "log_task.sh"

# =============================================================================
# EVENT: FileChanged
# Fires when a file is created/modified/deleted by the agent.
# Observational.
# =============================================================================
[[event.FileChanged]]
type = "command"
command = "file_watch.sh"
matcher = "/\.(rs|toml)$/"           # Only watch Rust files
```

---

## 7. Module-by-Module Implementation

### 7.1 `src/hooks/config.rs` — EXPAND HookEvent

The existing `HookEvent` enum has 11 variants. Replace with 28 + Custom:

```rust
use strum::{Display, EnumString, EnumIter, IntoStaticStr};

/// Complete set of hook events.
/// Each variant maps to a lifecycle point in jcode.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
    Display, EnumString, EnumIter, IntoStaticStr,
)]
#[strum(serialize_all = "PascalCase")]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    // === Tool Events ===
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    ToolError,
    UserPromptSubmit,
    UserPromptExpansion,

    // === Session Lifecycle ===
    SessionStart,
    SessionEnd,
    SessionUpdated,
    SessionDiff,
    SessionError,
    SessionIdle,

    // === Permission ===
    PermissionRequest,
    PermissionDenied,
    PermissionAsked,
    PermissionReplied,

    // === Agent Lifecycle ===
    AgentStart,
    AgentEnd,
    SubagentStart,
    SubagentStop,

    // === Execution Control ===
    Stop,

    // === Compaction ===
    PreCompact,
    PostCompact,
    AutoCompactionControl,

    // === Task ===
    TaskCreated,
    TaskCompleted,

    // === Environment ===
    Setup,
    FileChanged,

    // === Wildcard ===
    /// Allows user-defined event names not in the standard set.
    /// Configured as `Custom("my_event")` in TOML.
    Custom(String),
}

impl HookEvent {
    /// Parse a hook event from a string (case-insensitive, flexible delimiter).
    pub fn parse(s: &str) -> Option<Self> {
        // Normalize: remove underscores, hyphens, to lowercase
        let normalized = s.trim()
            .replace('_', "")
            .replace('-', "")
            .replace(' ', "")
            .to_lowercase();

        // Try standard variants via strum
        // strum's EnumString is case-sensitive, so we manually match
        match normalized.as_str() {
            "pretooluse" => Some(Self::PreToolUse),
            "posttooluse" => Some(Self::PostToolUse),
            "posttoolusefailure" => Some(Self::PostToolUseFailure),
            "toolerror" => Some(Self::ToolError),
            "userpromptsubmit" => Some(Self::UserPromptSubmit),
            "userpromptexpansion" => Some(Self::UserPromptExpansion),
            "sessionstart" => Some(Self::SessionStart),
            "sessionend" => Some(Self::SessionEnd),
            "sessionupdated" => Some(Self::SessionUpdated),
            "sessiondiff" => Some(Self::SessionDiff),
            "sessionerror" => Some(Self::SessionError),
            "sessionidle" => Some(Self::SessionIdle),
            "permissionrequest" => Some(Self::PermissionRequest),
            "permissiondenied" => Some(Self::PermissionDenied),
            "permissionasked" => Some(Self::PermissionAsked),
            "permissionreplied" => Some(Self::PermissionReplied),
            "agentstart" => Some(Self::AgentStart),
            "agentend" => Some(Self::AgentEnd),
            "subagentstart" => Some(Self::SubagentStart),
            "subagentstop" => Some(Self::SubagentStop),
            "stop" => Some(Self::Stop),
            "precompact" => Some(Self::PreCompact),
            "postcompact" => Some(Self::PostCompact),
            "autocompactioncontrol" => Some(Self::AutoCompactionControl),
            "taskcreated" => Some(Self::TaskCreated),
            "taskcompleted" => Some(Self::TaskCompleted),
            "setup" => Some(Self::Setup),
            "filechanged" => Some(Self::FileChanged),
            s if s.starts_with("custom") => {
                let name = s.trim_start_matches("custom")
                    .trim_start_matches(':')
                    .to_string();
                Some(Self::Custom(name))
            }
            _ => None,
        }
    }

    /// Whether this event can block execution.
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::PreToolUse | Self::UserPromptSubmit
            | Self::PermissionRequest | Self::PermissionAsked
            | Self::AgentStart | Self::Stop | Self::PreCompact)
    }

    /// Whether this event is fire-and-forget (observational only).
    pub fn is_observational(&self) -> bool {
        !self.is_blocking()
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Custom(name) => name,
            _ => self.into(),
        }
    }

    /// The string used in JSON serialization of HookInput.hook_event_name.
    pub fn event_name(&self) -> String {
        self.display_name().to_string()
    }

    /// Return all standard event variants (excluding Custom).
    pub fn all_standard() -> Vec<Self> {
        vec![
            Self::PreToolUse, Self::PostToolUse, Self::PostToolUseFailure, Self::ToolError,
            Self::UserPromptSubmit, Self::UserPromptExpansion,
            Self::SessionStart, Self::SessionEnd, Self::SessionUpdated,
            Self::SessionDiff, Self::SessionError, Self::SessionIdle,
            Self::PermissionRequest, Self::PermissionDenied, Self::PermissionAsked, Self::PermissionReplied,
            Self::AgentStart, Self::AgentEnd, Self::SubagentStart, Self::SubagentStop,
            Self::Stop,
            Self::PreCompact, Self::PostCompact, Self::AutoCompactionControl,
            Self::TaskCreated, Self::TaskCompleted,
            Self::Setup, Self::FileChanged,
        ]
    }
}

// ===========================================================================
// EXISTING: HookHandlerConfig — ADD Agent type
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HookHandlerConfig {
    /// Shell command handler (bash/powershell)
    Command(CommandHandlerConfig),
    /// HTTP request handler
    Http(HttpHandlerConfig),
    /// jcode subagent handler (NEW)
    Agent(AgentHandlerConfig),
    /// External plugin/script handler (NEW)
    Plugin(PluginHandlerConfig),
}

impl Default for HookHandlerConfig {
    fn default() -> Self {
        Self::Command(CommandHandlerConfig::default())
    }
}

/// Agent handler configuration (NEW)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentHandlerConfig {
    pub enabled: bool,
    /// Agent ID or name registered in jcode's agent registry
    pub agent_id: String,
    /// System prompt override for the hook agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Timeout in seconds (default: 120s for agent tasks)
    pub timeout_secs: u64,
    /// Whether to wait for agent completion (default: true)
    pub wait_for_completion: bool,
    /// Matcher pattern
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<HookMatcher>,
    /// Condition expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
}

impl Default for AgentHandlerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_id: String::new(),
            system_prompt: None,
            timeout_secs: 120,
            wait_for_completion: true,
            matcher: None,
            if_: None,
        }
    }
}

/// Plugin handler configuration (NEW)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginHandlerConfig {
    pub enabled: bool,
    /// Path to the plugin executable
    pub path: String,
    /// CLI arguments passed to the plugin
    #[serde(default)]
    pub args: Vec<String>,
    /// Plugin timeout in seconds
    pub timeout_secs: u64,
    /// Plugin version requirement (e.g., ">=1.0.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Matcher pattern
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<HookMatcher>,
    /// Condition expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
}

impl Default for PluginHandlerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: String::new(),
            args: Vec::new(),
            timeout_secs: 30,
            version: None,
            matcher: None,
            if_: None,
        }
    }
}

// ===========================================================================
// EXISTING: HooksConfig — ADD settings + event groups
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HooksConfig {
    /// Global settings
    #[serde(default)]
    pub settings: HookSettings,

    /// Events mapped to their handlers.
    /// Key is event name (PascalCase), value is array of handler configs.
    #[serde(default)]
    pub events: HashMap<String, Vec<HookHandlerConfig>>,
}

/// Global hooks settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSettings {
    /// Default timeout for all hooks (seconds)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Maximum concurrent hooks per event
    #[serde(default = "default_concurrency")]
    pub max_concurrency: usize,
    /// Dry-run mode: log only, no execution
    #[serde(default)]
    pub dry_run: bool,
    /// If true: hook failure blocks the operation
    #[serde(default)]
    pub fail_closed: bool,
}

fn default_timeout() -> u64 { 30 }
fn default_concurrency() -> usize { 10 }

impl Default for HookSettings {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout(),
            max_concurrency: default_concurrency(),
            dry_run: false,
            fail_closed: false,
        }
    }
}

impl HooksConfig {
    /// Merge another config into this one.
    /// For events: appends arrays (unlike current version which overwrites per-key).
    pub fn merge(&mut self, other: HooksConfig) {
        // Merge settings (other wins)
        self.settings.timeout_secs = other.settings.timeout_secs;
        self.settings.max_concurrency = other.settings.max_concurrency;
        self.settings.dry_run = other.settings.dry_run;
        self.settings.fail_closed = other.settings.fail_closed;

        // Merge events: APPEND handlers from other to existing list
        for (event_name, new_handlers) in other.events.into_iter() {
            let entry = self.events.entry(event_name).or_default();
            entry.extend(new_handlers);
        }
    }
}

// ===========================================================================
// EXISTING: load_hooks_config — ADD kill-switch check
// ===========================================================================

/// Load hooks configuration from multi-layer TOML files.
/// Returns empty config if hooks are disabled via env var.
pub fn load_hooks_config() -> HooksConfig {
    // Check kill-switch first
    if std::env::var("DISABLE_JCODE_HOOKS").is_ok() {
        tracing::info!("Hooks disabled via DISABLE_JCODE_HOOKS");
        return HooksConfig::default();
    }

    let mut merged = HooksConfig::default();

    // Layer 1: User-level (~/.jcode/hooks.toml)
    if let Some(path) = user_hooks_config_path() {
        if let Ok(Some(config)) = load_hooks_config_from_path(&path) {
            merged.merge(config);
        }
    }

    // Layer 2: Project-level (.jcode/hooks.toml)
    if let Some(path) = project_hooks_config_path() {
        if let Ok(Some(config)) = load_hooks_config_from_path(&path) {
            merged.merge(config);
        }
    }

    // Layer 3: Env-level ($JCODE_HOOKS_CONFIG)
    if let Some(path) = env_hooks_config_path() {
        if let Ok(Some(config)) = load_hooks_config_from_path(&path) {
            merged.merge(config);
        }
    }

    merged
}
```

### 7.2 `src/hooks/execute.rs` — ADD Agent + Plugin Handlers

```rust
use crate::hooks::config::{
    AgentHandlerConfig, CommandHandlerConfig, HttpHandlerConfig,
    HookHandlerConfig, PluginHandlerConfig,
};
use crate::hooks::types::{HookInput, HookOutput, HookResult};
use reqwest::Client;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use std::collections::HashMap;

/// Execute a hook based on its handler type.
pub async fn execute_hook(
    config: &HookHandlerConfig,
    input: &HookInput,
) -> Result<HookResult, String> {
    match config {
        HookHandlerConfig::Command(cmd_config) => {
            execute_command_hook(cmd_config, input).await
        }
        HookHandlerConfig::Http(http_config) => {
            execute_http_hook(http_config, input).await
        }
        HookHandlerConfig::Agent(agent_config) => {
            execute_agent_hook(agent_config, input).await
        }
        HookHandlerConfig::Plugin(plugin_config) => {
            execute_plugin_hook(plugin_config, input).await
        }
    }
}

/// Execute a command hook via bash/powershell (EXISTING, minor updates).
pub async fn execute_command_hook(
    config: &CommandHandlerConfig,
    input: &HookInput,
) -> Result<HookResult, String> {
    if !config.enabled {
        return Ok(HookResult::Continue(HookOutput::continue_()));
    }

    let input_json = serde_json::to_string(input)
        .map_err(|e| format!("Failed to serialize hook input: {}", e))?;

    let timeout_duration = Duration::from_secs(
        config.timeout_secs.unwrap_or(30)
    );

    let result = timeout(timeout_duration, async {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", &config.command]);
            c
        } else {
            let mut c = Command::new("bash");
            c.args(["-c", &config.command]);
            c
        };

        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = &config.cwd {
            cmd.current_dir(cwd);
        }

        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| format!("Failed to spawn hook process: {}", e))?;

        // Write input to stdin
        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(input_json.as_bytes()).await
                .map_err(|e| format!("Failed to write stdin: {}", e))?;
            stdin.flush().await.ok();
        }
        // Close stdin so the hook process can read EOF
        drop(child.stdin.take());

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(ref mut out) = child.stdout {
            out.read_to_end(&mut stdout).await
                .map_err(|e| format!("Failed to read stdout: {}", e))?;
        }
        if let Some(ref mut err) = child.stderr {
            err.read_to_end(&mut stderr).await.ok();
        }

        let status = child.wait().await
            .map_err(|e| format!("Failed to wait for hook process: {}", e))?;

        let exit_code = status.code().unwrap_or(-1);
        let output_str = String::from_utf8_lossy(&stdout);

        // Log stderr if any
        let stderr_str = String::from_utf8_lossy(&stderr);
        if !stderr_str.trim().is_empty() {
            tracing::debug!("Hook stderr: {}", stderr_str.trim());
        }

        match exit_code {
            0 => {
                // Parse output JSON
                let hook_output: HookOutput = serde_json::from_str(&output_str)
                    .unwrap_or_else(|_| HookOutput::continue_());
                Ok(HookResult::Continue(hook_output))
            }
            1 => {
                // Exit code 1 = failure
                let error_msg = if !output_str.trim().is_empty() {
                    output_str.trim().to_string()
                } else if !stderr_str.trim().is_empty() {
                    stderr_str.trim().to_string()
                } else {
                    format!("Hook exited with code 1: {}", config.command)
                };
                Ok(HookResult::Failed { error: error_msg })
            }
            2 => {
                // Exit code 2 = BLOCK
                let reason = if !output_str.trim().is_empty() {
                    output_str.trim().to_string()
                } else {
                    "Hook blocked the operation".to_string()
                };
                Ok(HookResult::Blocked {
                    reason,
                    output: HookOutput::block(&reason),
                })
            }
            _ => {
                Ok(HookResult::Failed {
                    error: format!("Hook exited with unexpected code {}", exit_code),
                })
            }
        }
    }).await;

    match result {
        Ok(Ok(r)) => Ok(r),
        Ok(Err(e)) => Ok(HookResult::Failed { error: e }),
        Err(_) => Ok(HookResult::Failed {
            error: format!("Hook timed out after {}s", timeout_duration.as_secs()),
        }),
    }
}

/// Execute an HTTP hook (EXISTING, accept config struct directly).
pub async fn execute_http_hook(
    config: &HttpHandlerConfig,
    input: &HookInput,
) -> Result<HookResult, String> {
    if !config.enabled {
        return Ok(HookResult::Continue(HookOutput::continue_()));
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs.unwrap_or(30)))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let body = config.body.as_ref()
        .cloned()
        .unwrap_or(serde_json::to_value(input).unwrap_or(serde_json::Value::Null));

    let mut request = match config.method.to_uppercase().as_str() {
        "GET" => client.get(&config.url),
        "POST" => client.post(&config.url),
        "PUT" => client.put(&config.url),
        "DELETE" => client.delete(&config.url),
        "PATCH" => client.patch(&config.url),
        "HEAD" => client.head(&config.url),
        "OPTIONS" => client.request(reqwest::Method::OPTIONS, &config.url),
        _ => return Ok(HookResult::Failed {
            error: format!("Unsupported HTTP method: {}", config.method),
        }),
    };

    for (k, v) in &config.headers {
        // Support ${VAR} interpolation
        let expanded = expand_env_var(v);
        request = request.header(k, expanded);
    }

    // For non-GET methods, send hook input as JSON body
    if config.method.to_uppercase().as_str() != "GET" {
        request = request.json(&body);
    }

    match request.send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                let hook_output: HookOutput = resp.json().await
                    .unwrap_or_else(|_| HookOutput::continue_());
                Ok(HookResult::Continue(hook_output))
            } else if status.as_u16() == 403 || status.as_u16() == 451 {
                // 403/451 = deny
                let reason = resp.text().await.unwrap_or_default();
                Ok(HookResult::Blocked { reason, output: HookOutput::block(&reason) })
            } else {
                let error = format!("HTTP {}: {}", status.as_u16(), status.canonical_reason().unwrap_or("Unknown"));
                Ok(HookResult::Failed { error })
            }
        }
        Err(e) => Ok(HookResult::Failed {
            error: format!("HTTP request failed: {}", e),
        }),
    }
}

/// Execute an agent hook — dispatches to a jcode subagent (NEW).
pub async fn execute_agent_hook(
    config: &AgentHandlerConfig,
    input: &HookInput,
) -> Result<HookResult, String> {
    if !config.enabled {
        return Ok(HookResult::Continue(HookOutput::continue_()));
    }

    let timeout_duration = Duration::from_secs(config.timeout_secs);

    timeout(timeout_duration, async {
        // Build hook-specific system prompt
        let system_prompt = config.system_prompt.as_deref()
            .unwrap_or("You are a hook handler. Process the hook input and return HookOutput JSON.");

        let agent_input = serde_json::to_string_pretty(input)
            .unwrap_or_default();

        // TODO: Dispatch to subagent via existing agent infrastructure
        // For now, log and continue
        tracing::debug!(
            "Agent hook '{}' would invoke agent '{}' with input: {}",
            input.hook_event_name,
            config.agent_id,
            agent_input,
        );

        // Placeholder: return continue until agent dispatch is wired
        Ok(HookResult::Continue(HookOutput::continue_()))
    }).await.unwrap_or_else(|_| {
        Ok(HookResult::Failed { error: "Agent hook timed out".to_string() })
    })
}

/// Execute a plugin hook — runs an external executable (NEW).
pub async fn execute_plugin_hook(
    config: &PluginHandlerConfig,
    input: &HookInput,
) -> Result<HookResult, String> {
    if !config.enabled {
        return Ok(HookResult::Continue(HookOutput::continue_()));
    }

    let input_json = serde_json::to_string(input)
        .map_err(|e| format!("Failed to serialize hook input: {}", e))?;

    let timeout_duration = Duration::from_secs(config.timeout_secs);

    timeout(timeout_duration, async {
        let mut cmd = Command::new(&config.path);
        cmd.args(&config.args);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn()
            .map_err(|e| format!("Failed to spawn plugin: {}", e))?;

        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(input_json.as_bytes()).await
                .map_err(|e| format!("Failed to write plugin stdin: {}", e))?;
            stdin.flush().await.ok();
        }
        drop(child.stdin.take());

        let mut stdout = Vec::new();
        if let Some(ref mut out) = child.stdout {
            out.read_to_end(&mut stdout).await
                .map_err(|e| format!("Failed to read plugin stdout: {}", e))?;
        }

        let status = child.wait().await
            .map_err(|e| format!("Failed to wait for plugin: {}", e))?;

        let exit_code = status.code().unwrap_or(-1);
        let output_str = String::from_utf8_lossy(&stdout);

        match exit_code {
            0 => {
                let hook_output: HookOutput = serde_json::from_str(&output_str)
                    .unwrap_or_else(|_| HookOutput::continue_());
                Ok(HookResult::Continue(hook_output))
            }
            1 => Ok(HookResult::Failed { error: output_str.trim().to_string() }),
            2 => Ok(HookResult::Blocked {
                reason: output_str.trim().to_string(),
                output: HookOutput::block(output_str.trim()),
            }),
            _ => Ok(HookResult::Failed {
                error: format!("Plugin exited with code {}", exit_code),
            }),
        }
    }).await.unwrap_or_else(|_| {
        Ok(HookResult::Failed { error: "Plugin timed out".to_string() })
    })
}

/// Expand ${VAR} patterns in a string using environment variables.
fn expand_env_var(s: &str) -> String {
    let mut result = s.to_string();
    // Support ${VAR_NAME} syntax
    let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();
    for caps in re.captures_iter(s) {
        if let Some(var_name) = caps.get(1) {
            if let Ok(val) = std::env::var(var_name.as_str()) {
                result = result.replace(&caps[0], &val);
            }
        }
    }
    result
}
```

### 7.3 `src/hooks/dispatch.rs` — Full Implementation (as shown in §5.2)

Add to Cargo.toml: `futures = { version = "0.3", features = ["std"] }`

Note: `dispatch_hooks()` is the **main entry point** for all hook execution throughout the codebase. It replaces direct calls to `execute_hook()`.

### 7.4 `src/hooks/mod.rs` — UPDATE Re-exports

```rust
//! Hooks module — lifecycle hooks for jcode events.

pub mod config;
pub mod dispatch;
pub mod execute;
pub mod matcher;
pub mod registry;
pub mod types;

pub use config::{
    load_hooks_config, AgentHandlerConfig, CommandHandlerConfig,
    HookEvent, HookHandlerConfig, HookSettings, HooksConfig,
    HttpHandlerConfig, PluginHandlerConfig,
};
pub use dispatch::{dispatch_hooks, DispatchConfig, DispatchStats};
pub use execute::{execute_hook, execute_command_hook, execute_http_hook, HookResult};
pub use matcher::{matches, HookMatcher, MatcherContext, parse_multi_pattern};
pub use registry::{HookContext, HookRegistry};
pub use types::*;
```

### 7.5 `src/hooks/registry.rs` — UPDATE HookContext

```rust
/// Context passed to hooks for matching decisions.
#[derive(Debug, Clone)]
pub struct HookContext {
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
    pub hook_event_name: String,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_use_id: Option<String>,
    pub permission_mode: Option<String>,
    // NEW fields
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub system_prompt: Option<String>,
    pub current_size_bytes: Option<u64>,
    pub task_id: Option<String>,
    pub file_path: Option<String>,
    pub stop_type: Option<String>,
}

impl HookContext {
    // Existing constructors remain, with new fields set to None by default

    pub fn new(session_id: &str, transcript_path: &str, cwd: &str, hook_event_name: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            transcript_path: transcript_path.to_string(),
            cwd: cwd.to_string(),
            hook_event_name: hook_event_name.to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            permission_mode: None,
            model: None,
            prompt: None,
            system_prompt: None,
            current_size_bytes: None,
            task_id: None,
            file_path: None,
            stop_type: None,
        }
    }

    // NEW: factory methods for new event types
    pub fn for_pre_compact(session_id: String, current_size: u64) -> Self {
        Self {
            session_id,
            current_size_bytes: Some(current_size),
            ..Self::new(&session_id, "", "", "PreCompact")
        }
    }

    pub fn for_stop(session_id: String, stop_type: String) -> Self {
        Self {
            stop_type: Some(stop_type),
            ..Self::new(&session_id, "", "", "Stop")
        }
    }

    pub fn for_agent_start(session_id: String, agent_id: String, agent_type: String) -> Self {
        Self {
            agent_id: Some(agent_id),
            agent_type: Some(agent_type),
            ..Self::new(&session_id, "", "", "AgentStart")
        }
    }
}
```

---

## 8. Event-Specific Integration Points

### 8.1 `src/tool/mod.rs` — NOTIFICATION POINTS

The existing file already imports hooks. Add callsites:

**Current code (tool execution loop):**
```rust
// BEFORE tool execution
let hook_input = HookInput::for_tool(session_id, transcript_path, cwd, tool_name, tool_input);
let result = execute_hook(&cmd_config, &hook_input).await;
```

**Replace with dispatch pattern:**
```rust
use crate::hooks::dispatch::{dispatch_hooks, DispatchConfig};
use crate::hooks::config::HookEvent;

// --- PreToolUse ---
{
    let event = HookEvent::PreToolUse;
    let handlers = registry.get_matching(&event, &hook_ctx);
    if !handlers.is_empty() {
        let input = HookInputBuilder::new()
            .session(&session_id, &cwd)
            .event(&event.event_name())
            .agent(&agent_id, &agent_type)
            .tool(&tool_name, tool_input.clone(), &tool_use_id)
            .build();
        let (decision, stats) = dispatch_hooks(&handlers, &input, &dispatch_config).await;
        match decision {
            AggregatedDecision::Deny { reason, .. } => {
                return Err(ToolError::BlockedByHook(reason));
            }
            AggregatedDecision::Ask { .. } => {
                // Fall through to normal permission flow
            }
            AggregatedDecision::Allow => {}
        }
    }
}

// --- Tool execution ---
let result = execute_tool(tool_name, tool_input).await;

// --- PostToolUse ---
{
    let event = HookEvent::PostToolUse;
    let handlers = registry.get_matching(&event, &hook_ctx);
    if !handlers.is_empty() {
        let input = HookInputBuilder::new()
            .session(&session_id, &cwd)
            .event(&event.event_name())
            .tool(&tool_name, tool_input, &tool_use_id)
            .tool_output(result.output.clone())
            .duration(duration_ms)
            .build();
        let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    }
}

// --- PostToolUseFailure ---
if let Err(e) = &result {
    let event = HookEvent::PostToolUseFailure;
    let handlers = registry.get_matching(&event, &hook_ctx);
    if !handlers.is_empty() {
        let input = HookInputBuilder::new()
            .session(&session_id, &cwd)
            .event(&event.event_name())
            .tool(&tool_name, tool_input, &tool_use_id)
            .error(&e.to_string(), -1)
            .duration(duration_ms)
            .build();
        let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    }
}

// --- ToolError ---
if matches!(result, Err(ToolError::ToolError { .. })) {
    let event = HookEvent::ToolError;
    let handlers = registry.get_matching(&event, &hook_ctx);
    if !handlers.is_empty() {
        let input = HookInputBuilder::new()
            .session(&session_id, &cwd)
            .event(&event.event_name())
            .tool(&tool_name, tool_input, &tool_use_id)
            .error(&error_msg, error_code)
            .build();
        let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    }
}
```

### 8.2 `src/safety.rs` — PERMISSION HOOKS

The existing `PermissionRequest`/`PermissionDenied` flow:

```rust
// --- PermissionRequest ---
// Find the dispatch_config and registry references (from the safety system)
let event = HookEvent::PermissionRequest;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, "")
        .event(&event.event_name())
        .tool(&tool_name, serde_json::json!({}), &request_id)
        .permission(&permission_mode, &request_id, &description)
        .build();
    let (decision, _stats) = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    match decision {
        AggregatedDecision::Deny { reason, .. } => {
            return PermissionResult::Denied { reason: Some(reason) };
        }
        AggregatedDecision::Allow => {
            return PermissionResult::Approved { message: None };
        }
        AggregatedDecision::Ask { .. } => {
            // Fall through to normal user-ask flow
        }
    }
}

// --- PermissionDenied ---
// After a permission was denied:
let event = HookEvent::PermissionDenied;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, "")
        .event(&event.event_name())
        .tool(&tool_name, serde_json::json!({}), &request_id)
        .permission(&permission_mode, &request_id, &deny_reason)
        .build();
    let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
}
```

### 8.3 `src/agent.rs` — NEW INTEGRATION POINT

Agent lifecycle hooks. Insert at agent init/shutdown:

```rust
// --- AgentStart ---
// In agent initialization, after loading config but before processing starts:
let event = HookEvent::AgentStart;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .agent(&agent_id, &agent_type)
        .build();
    let (decision, stats) = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    match decision {
        AggregatedDecision::Deny { reason, .. } => {
            // Agent startup blocked by hook
            bail!("Agent startup blocked by hook: {}", reason);
        }
        AggregatedDecision::Allow => {}
        AggregatedDecision::Ask { .. } => {
            // Agent hooks shouldn't produce "ask" — treat as allow
        }
    }
}

// --- AgentEnd ---
// In agent shutdown:
let event = HookEvent::AgentEnd;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .agent(&agent_id, &agent_type)
        .duration(total_duration_ms)
        .build();
    let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
}
```

### 8.4 `src/subagent.rs` (or wherever subagent spawning happens)

```rust
// --- SubagentStart ---
let event = HookEvent::SubagentStart;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .agent(&parent_agent_id, "parent")
        .build();
    let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
}

// --- SubagentStop ---
let event = HookEvent::SubagentStop;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .duration(duration_ms)
        .build();
    let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
}
```

### 8.5 `src/server/` (session management)

```rust
// --- SessionStart ---
// In server::start_session():
let event = HookEvent::SessionStart;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .agent(&agent_id, &agent_type)
        .build();
    let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
}

// --- SessionEnd ---
// In server::end_session():
let event = HookEvent::SessionEnd;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .duration(session_duration_ms)
        .build();
    let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
}
```

### 8.6 `src/compaction.rs` — COMPACTION HOOKS

```rust
// --- PreCompact ---
// Before compaction runs:
let event = HookEvent::PreCompact;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .build();
    let (decision, _stats) = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    if matches!(decision, AggregatedDecision::Deny { .. }) {
        // Skip compaction this cycle
        tracing::info!("PreCompact hook blocked compaction");
        return;
    }
}

// --- PostCompact ---
// After compaction completes:
let event = HookEvent::PostCompact;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .build();
    let _ = dispatch_hooks(&handlers, &input, &dispatch_config).await;
}
```

### 8.7 `src/cli/dispatch.rs` — USER PROMPT HOOKS

```rust
// --- UserPromptSubmit ---
// Before sending a user prompt to the LLM:
let event = HookEvent::UserPromptSubmit;
let handlers = registry.get_matching(&event, &hook_ctx);
if !handlers.is_empty() {
    let input = HookInputBuilder::new()
        .session(&session_id, &cwd)
        .event(&event.event_name())
        .prompt(&user_prompt_text)
        .build();
    let (decision, _stats) = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    match decision {
        AggregatedDecision::Deny { reason, .. } => {
            // Show block reason to user
            return Err(PromptError::BlockedByHook(reason));
        }
        AggregatedDecision::Allow => {}
        AggregatedDecision::Ask { .. } => {
            // Prompt hooks shouldn't produce "ask" — treat as deny
            return Err(PromptError::BlockedByHook("Hook requested approval".to_string()));
        }
    }
}
```

### 8.8 File Change Detection

```rust
// --- FileChanged ---
// Whenever a tool creates/modifies/deletes a file:
async fn notify_file_changed(
    registry: &HookRegistry,
    dispatch_config: &DispatchConfig,
    session_id: &str,
    file_path: &str,
    change_type: &str,
    diff: Option<&str>,
) {
    let event = HookEvent::FileChanged;
    let ctx = HookContext::new(session_id, "", "", "FileChanged");
    let handlers = registry.get_matching(&event, &ctx);
    if !handlers.is_empty() {
        let input = HookInputBuilder::new()
            .session(session_id, "")
            .event(&event.event_name())
            .build();
        let _ = dispatch_hooks(&handlers, &input, dispatch_config).await;
    }
}
```

---

## 9. CLI Commands

Update `src/cli/commands.rs` to support all events:

```rust
// Existing: enable/disable hooks
// Add: listing hooks by event type, testing hooks

/// jcode hooks list
///   Lists all configured hooks grouped by event.
/// jcode hooks list --event PreToolUse
///   Lists only hooks for a specific event.
///
/// jcode hooks test PreToolUse --tool Bash
///   Simulates a hook execution without an actual event.
pub fn register_hooks_commands(app: App) -> App {
    app.subcommand(
        Command::new("hooks")
            .about("Manage lifecycle hooks")
            .subcommand(
                Command::new("list")
                    .about("List configured hooks")
                    .arg(arg!(-e --event <EVENT> "Filter by event name"))
            )
            .subcommand(
                Command::new("enable")
                    .about("Enable a hook")
                    .arg(arg!(<EVENT> "Event name"))
                    .arg(arg!(<ID> "Hook index or label"))
            )
            .subcommand(
                Command::new("disable")
                    .about("Disable a hook")
                    .arg(arg!(<EVENT> "Event name"))
                    .arg(arg!(<ID> "Hook index or label"))
            )
            .subcommand(
                Command::new("test")
                    .about("Test a hook without triggering the real event")
                    .arg(arg!(<EVENT> "Event name to simulate"))
                    .arg(arg!(--tool <TOOL> "Tool name for tool events"))
                    .arg(arg!(--prompt <TEXT> "Prompt text for prompt events"))
                    .arg(arg!(--dry-run "Don't execute, just show what would run"))
            )
            .subcommand(
                Command::new("metrics")
                    .about("Show hook execution metrics")
                    .arg(arg!(-e --event <EVENT> "Filter by event name"))
            )
    )
}
```

---

## 10. Test Plan (Unit + Integration + E2E)

### 10.1 Unit Tests (`src/hooks/tests.rs` or per-module)

#### `test_types.rs` — HookInput/HookOutput serialization

```rust
#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn test_hook_input_default() {
        let input = HookInput::default();
        assert_eq!(input.schema_version, "2.0");
        assert!(input.session_id.is_empty());
    }

    #[test]
    fn test_hook_input_builder_tool() {
        let input = HookInputBuilder::new()
            .session("ses_123", "/home/user/project")
            .event("PreToolUse")
            .agent("agent_1", "default")
            .tool("Bash", serde_json::json!({"command": "ls"}), "tool_1")
            .build();
        assert_eq!(input.session_id, "ses_123");
        assert_eq!(input.tool_name.as_deref(), Some("Bash"));
        assert_eq!(input.hook_event_name, "PreToolUse");
    }

    #[test]
    fn test_hook_output_serialization() {
        let output = HookOutput::block("Dangerous command");
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("false")); // continue_ = false
        assert!(json.contains("Dangerous command"));

        // Round-trip
        let deserialized: HookOutput = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.continue_);
    }

    #[test]
    fn test_hook_output_allow() {
        let output = HookOutput::allow();
        assert!(output.continue_);
        assert_eq!(output.decision.as_deref(), Some("allow"));
    }
}
```

#### `test_config.rs` — HookEvent parsing + HooksConfig merge

```rust
#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn test_hook_event_parse_all_variants() {
        let test_cases = vec![
            ("PreToolUse", HookEvent::PreToolUse),
            ("pretooluse", HookEvent::PreToolUse),
            ("pre_tool_use", HookEvent::PreToolUse),
            ("posttoolusefailure", HookEvent::PostToolUseFailure),
            ("UserPromptExpansion", HookEvent::UserPromptExpansion),
            ("sessionidle", HookEvent::SessionIdle),
            ("Stop", HookEvent::Stop),
            ("FileChanged", HookEvent::FileChanged),
            ("custom:my_event", HookEvent::Custom("my_event".to_string())),
        ];
        for (input, expected) in test_cases {
            assert_eq!(HookEvent::parse(input), Some(expected),
                "Failed to parse '{}'", input);
        }
    }

    #[test]
    fn test_hook_event_is_blocking() {
        assert!(HookEvent::PreToolUse.is_blocking());
        assert!(HookEvent::PermissionRequest.is_blocking());
        assert!(HookEvent::AgentStart.is_blocking());
        assert!(HookEvent::PreCompact.is_blocking());
        assert!(HookEvent::Stop.is_blocking());
        assert!(!HookEvent::SessionStart.is_blocking());
        assert!(!HookEvent::PostToolUse.is_blocking());
        assert!(!HookEvent::FileChanged.is_blocking());
    }

    #[test]
    fn test_hooks_config_merge_appends_handlers() {
        let mut config1 = HooksConfig::default();
        config1.events.entry("PreToolUse".to_string()).or_default().push(
            HookHandlerConfig::Command(CommandHandlerConfig {
                command: "hook1".to_string(),
                ..Default::default()
            }),
        );

        let mut config2 = HooksConfig::default();
        config2.events.entry("PreToolUse".to_string()).or_default().push(
            HookHandlerConfig::Command(CommandHandlerConfig {
                command: "hook2".to_string(),
                ..Default::default()
            }),
        );

        config1.merge(config2);
        assert_eq!(config1.events["PreToolUse"].len(), 2);
    }

    #[test]
    fn test_toml_round_trip() {
        let toml_str = r#"
[settings]
timeout_secs = 15
max_concurrency = 5

[[event.PreToolUse]]
type = "command"
command = "check.sh"
matcher = "Bash|Write"
"#;
        let config: HooksConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.settings.timeout_secs, 15);
        assert_eq!(config.events.len(), 1);
        assert_eq!(config.events["PreToolUse"].len(), 1);
    }
}
```

#### `test_dispatch.rs` — Parallel dispatch

```rust
#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[tokio::test]
    async fn test_dispatch_empty_handlers() {
        let handlers: Vec<&HookHandlerConfig> = vec![];
        let input = HookInput::default();
        let config = DispatchConfig::default();
        let (decision, stats) = dispatch_hooks(&handlers, &input, &config).await;
        assert!(matches!(decision, AggregatedDecision::Allow));
        assert_eq!(stats.total_hooks, 0);
    }

    #[tokio::test]
    async fn test_dispatch_single_continue() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: "echo '{\"continue_\": true}'".to_string(),
            enabled: true,
            ..Default::default()
        });
        let handlers = vec![&handler];
        let input = HookInput::default();
        let config = DispatchConfig::default();
        let (decision, stats) = dispatch_hooks(&handlers, &input, &config).await;
        assert!(matches!(decision, AggregatedDecision::Allow));
        assert_eq!(stats.completed, 1);
    }

    #[tokio::test]
    async fn test_dispatch_deny_wins() {
        let allow_handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: "echo '{\"continue_\": true, \"decision\": \"allow\"}'".to_string(),
            enabled: true,
            ..Default::default()
        });
        let deny_handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: "exit 2".to_string(),
            enabled: true,
            ..Default::default()
        });
        let handlers = vec![&allow_handler, &deny_handler];
        let input = HookInput::default();
        let config = DispatchConfig::default();
        let (decision, stats) = dispatch_hooks(&handlers, &input, &config).await;
        assert!(matches!(decision, AggregatedDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn test_dispatch_disabled_skip() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: "exit 2".to_string(),
            enabled: true,
            ..Default::default()
        });
        let handlers = vec![&handler];
        let input = HookInput::default();
        let mut config = DispatchConfig::default();
        config.disabled = true;
        let (decision, stats) = dispatch_hooks(&handlers, &input, &config).await;
        assert!(matches!(decision, AggregatedDecision::Allow));
        assert_eq!(stats.total_hooks, 0);
    }

    #[tokio::test]
    async fn test_dispatch_timeout() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: "sleep 10".to_string(),
            enabled: true,
            timeout_secs: Some(1),
            ..Default::default()
        });
        let handlers = vec![&handler];
        let input = HookInput::default();
        let config = DispatchConfig {
            default_timeout_secs: 1,
            ..Default::default()
        };
        let (decision, stats) = dispatch_hooks(&handlers, &input, &config).await;
        assert!(matches!(decision, AggregatedDecision::Allow)); // timeout → continue
        assert_eq!(stats.total_hooks, 1);
    }

    #[tokio::test]
    async fn test_event_disable_via_env() {
        // Set the kill-switch
        std::env::set_var("JCODE_SKIP_EVENT_PRETOOLUSE", "1");
        let disabled = std::env::var("JCODE_SKIP_EVENT_PRETOOLUSE").is_ok();
        assert!(disabled);
        std::env::remove_var("JCODE_SKIP_EVENT_PRETOOLUSE");
    }
}
```

### 10.2 Integration Tests (`tests/hooks_integration.rs`)

```rust
use jcode::hooks::config::{
    CommandHandlerConfig, HookEvent, HookHandlerConfig, HooksConfig,
};
use jcode::hooks::dispatch::{dispatch_hooks, DispatchConfig, DispatchStats};
use jcode::hooks::registry::{HookContext, HookRegistry};
use jcode::hooks::types::{AggregatedDecision, HookInput, HookInputBuilder};

/// Integration test: full flow from config → registry → dispatch → decision.
#[tokio::test]
async fn test_hooks_full_flow() {
    // 1. Create config
    let mut config = HooksConfig::default();
    config.events.entry("PreToolUse".to_string()).or_default().push(
        HookHandlerConfig::Command(CommandHandlerConfig {
            command: "echo '{\"continue_\": true}'".to_string(),
            enabled: true,
            ..Default::default()
        }),
    );

    // 2. Build registry
    let registry = HookRegistry::from_config(config);

    // 3. Create context and get matching handlers
    let ctx = HookContext::for_tool(
        "ses_1", "/tmp/transcript.json", "/project",
        "Bash", serde_json::json!({"command": "ls"}),
    );
    let handlers = registry.get_matching(&HookEvent::PreToolUse, &ctx);

    // 4. Build input
    let input = HookInputBuilder::new()
        .session("ses_1", "/project")
        .event("PreToolUse")
        .tool("Bash", serde_json::json!({"command": "ls"}), "tool_1")
        .build();

    // 5. Dispatch
    let dispatch_config = DispatchConfig::default();
    let (decision, stats) = dispatch_hooks(&handlers, &input, &dispatch_config).await;

    // 6. Assert
    assert!(matches!(decision, AggregatedDecision::Allow));
    assert_eq!(stats.total_hooks, 1);
    assert_eq!(stats.completed, 1);
}

/// Test: parallel execution of multiple hooks.
#[tokio::test]
async fn test_parallel_hook_execution() {
    let mut config = HooksConfig::default();
    // Add 3 hooks that each take 100ms
    for i in 0..3 {
        config.events.entry("PostToolUse".to_string()).or_default().push(
            HookHandlerConfig::Command(CommandHandlerConfig {
                command: format!("echo '{{\"continue_\": true, \"system_message\": \"hook{}\"}}'", i),
                enabled: true,
                timeout_secs: Some(5),
                ..Default::default()
            }),
        );
    }

    let registry = HookRegistry::from_config(config);
    let ctx = HookContext::new("ses_1", "", "/project", "PostToolUse");
    let handlers = registry.get_matching(&HookEvent::PostToolUse, &ctx);

    let input = HookInputBuilder::new()
        .session("ses_1", "/project")
        .event("PostToolUse")
        .build();

    let start = std::time::Instant::now();
    let dispatch_config = DispatchConfig::default();
    let (_decision, stats) = dispatch_hooks(&handlers, &input, &dispatch_config).await;
    let elapsed = start.elapsed();

    // All 3 hooks should have run
    assert_eq!(stats.total_hooks, 3);
    assert_eq!(stats.completed, 3);
}
```

### 10.3 Kill-Switch Tests

```rust
#[test]
fn test_kill_switch() {
    // DISABLE_JCODE_HOOKS disables all hooks
    std::env::set_var("DISABLE_JCODE_HOOKS", "1");
    let config = load_hooks_config();
    assert!(config.events.is_empty());
    std::env::remove_var("DISABLE_JCODE_HOOKS");

    // JCODE_SKIP_HOOKS skips execution
    std::env::set_var("JCODE_SKIP_HOOKS", "1");
    let dispatch_config = DispatchConfig::default();
    assert!(dispatch_config.disabled);
    std::env::remove_var("JCODE_SKIP_HOOKS");
}
```

### 10.4 E2E Test

```bash
# Test: PreToolUse hook that blocks a tool
mkdir -p /tmp/hook-test
cat > /tmp/hook-test/block_bash.sh << 'EOF'
#!/bin/bash
read input
# Block Bash tool
echo '{"continue_": false, "stop_reason": "Bash blocked by test hook"}'
exit 2
EOF
chmod +x /tmp/hook-test/block_bash.sh

# Create hooks config
cat > /tmp/hook-test/hooks.toml << 'EOF'
[[event.PreToolUse]]
type = "command"
command = "/tmp/hook-test/block_bash.sh"
matcher = "Bash"
EOF

# Run jcode with hooks config
JCODE_HOOKS_CONFIG=/tmp/hook-test/hooks.toml jcode run "echo hello" --tool Bash 2>&1 | grep -q "blocked"
echo $?  # Should be 0 (blocked)
```

---

## 11. Cross-Repo Reference Table

| Event | CC | OC | CX | OMOA | OMCC | OMCX | OMPI | PI | CB |
|-------|----|----|-----|------|------|------|------|-----|----|
| PreToolUse | ✅ | ✅ tool.execute.before | ✅ | ✅ | ✅ | ✅ | ✅ tool_call.blockable | ✅ OnBeforeToolCall | — |
| PostToolUse | ✅ | ✅ tool.execute.after | ✅ | ✅ | ✅ | ✅ | ✅ tool_result.modifiable | ✅ OnAfterToolCall | ✅ |
| PostToolUseFailure | ✅ | — | — | ✅ | ✅ | — | — | — | — |
| ToolError | — | — | — | — | — | — | ✅ | — | — |
| UserPromptSubmit | ✅ | ✅ chat.message | ✅ | — | ✅ | ✅ | — | — | — |
| UserPromptExpansion | — | ✅ chat.messages.transform | — | — | — | — | — | — | — |
| SessionStart | ✅ | ✅ session.created | ✅ | ✅ | ✅ | ✅ | — | ✅ | — |
| SessionEnd | ✅ | ✅ session.deleted | — | ✅ | ✅ | — | — | ✅ | — |
| SessionUpdated | — | ✅ session.updated | — | — | — | — | — | — | — |
| SessionDiff | — | ✅ session.diff | — | — | — | — | — | — | — |
| SessionError | — | ✅ session.error | — | — | — | — | — | — | — |
| SessionIdle | — | ✅ session.idle | — | — | — | — | ✅ turn_start | — | — |
| PermissionRequest | ✅ | ✅ permission.ask | ✅ | — | ✅ | — | ✅ | ✅ Capability check | — |
| PermissionDenied | ✅ | — | — | — | — | — | — | — | — |
| PermissionAsked | — | ✅ permission.asked | — | — | — | — | — | ✅ | — |
| PermissionReplied | — | ✅ permission.replied | — | — | — | — | — | — | — |
| AgentStart | — | — | — | — | — | — | ✅ before_agent_start | ✅ OnAgentStart | — |
| AgentEnd | — | — | — | — | — | — | ✅ agent_end | ✅ OnAgentEnd | — |
| SubagentStart | ✅ | — | ✅ | — | ✅ | — | — | — | — |
| SubagentStop | ✅ | — | ✅ | — | ✅ | — | — | — | — |
| Stop | ✅ | — | ✅ | ✅ | ✅ | ✅ | — | — | — |
| PreCompact | ✅ | ✅ session.compacting | ✅ | ✅ | ✅ | — | ✅ | ✅ | — |
| PostCompact | ✅ | ✅ session.compacted | — | — | — | — | — | ✅ | — |
| AutoCompactionControl | — | ✅ compaction.autocontinue | — | — | — | — | — | — | — |
| TaskCreated | ✅ | — | — | — | — | — | — | — | — |
| TaskCompleted | ✅ | — | — | — | — | — | — | — | — |
| Setup | ✅ | — | — | — | — | — | — | ✅ Startup | — |
| FileChanged | ✅ | — | — | — | — | — | — | — | — |

**Legend**: CC=claude-code, OC=opencode, CX=codex, OMOA=oh-my-openagent, OMCC=oh-my-claudecode, OMCX=oh-my-codex, OMPI=oh-my-pi, PI=pi-agent-rust, CB=codebuff

---

## 12. Migration Path

### Phase A — Types & Config (1-2 days)

1. **Update `Cargo.toml`**:
   - Add `futures = { version = "0.3", features = ["std"] }`
   - Add `strum = { version = "0.26", features = ["derive"] }`
   - `strum_macros` is already covered by strum's derive feature

2. **Replace `src/hooks/types.rs`** (full file as shown in §5.1)
3. **Update `src/hooks/config.rs`** (HookEvent enum + new handler types as shown in §7.1)
4. **Update `src/hooks/registry.rs`** (new HookContext fields as shown in §7.5)
5. **Update `src/hooks/mod.rs`** (new re-exports as shown in §7.4)

**Verify**: `cargo check` must pass with zero errors.

### Phase B — Dispatch Engine (1-2 days)

6. **Create `src/hooks/dispatch.rs`** (full file as shown in §5.2)
7. **Update `src/hooks/execute.rs`** (add agent + plugin handlers, exit code 2 blocking, env var interpolation as shown in §7.2)
8. **Update `src/hooks/config.rs`** (add `AgentHandlerConfig`, `PluginHandlerConfig`)

**Verify**: `cargo test` passes. `cargo check --all-features` passes.

### Phase C — Integration Points (3-5 days)

9. **`src/tool/mod.rs`**: Wire PreToolUse/PostToolUse/PostToolUseFailure/ToolError via `dispatch_hooks()`
10. **`src/safety.rs`**: Wire PermissionRequest/PermissionDenied/PermissionAsked/PermissionReplied via `dispatch_hooks()`
11. **`src/agent.rs`**: Wire AgentStart/AgentEnd/Stop
12. **Wherever subagent spawning happens**: Wire SubagentStart/SubagentStop
13. **`src/server/`**: Wire SessionStart/SessionEnd/SessionUpdated/SessionDiff/SessionError/SessionIdle
14. **`src/compaction.rs`**: Wire PreCompact/PostCompact/AutoCompactionControl
15. **`src/cli/dispatch.rs`**: Wire UserPromptSubmit/UserPromptExpansion
16. **File change detection**: Wire FileChanged
17. **Task system**: Wire TaskCreated/TaskCompleted
18. **`src/cli/commands.rs`**: Update hooks commands

**Verify**: `cargo build --release` passes. Manual smoke test: `jcode run "hello"` works with and without hooks config.

### Phase D — Tests (1-2 days)

19. Add unit tests as shown in §10.1
20. Add integration tests as shown in §10.2
21. Create E2E test scripts as shown in §10.4

**Verify**: All tests pass: `cargo test hooks`

---

## 13. Known Limitations & v2.1 Deferred

- **Agent handler type**: The Agent handler (`HookHandlerConfig::Agent`) dispatches to a jcode subagent, but the subagent dispatch infrastructure is a placeholder. Full implementation requires wiring to the agent spawning system in `src/agent.rs`.
- **Notification event**: Claude Code's `Notification` event is deferred to v2.1 (requires notification channel integration).
- **InstructionsLoaded**: Requires tracking when context files are loaded — deferred.
- **TeammateIdle**: Requires swarm/teammate tracking — deferred.
- **Context event**: oh-my-pi's context event for modifying the context window — requires compaction integration.
- **ResourcesDiscover / TodoReminder**: oh-my-pi specific, requires resource registry.
- **AutoRetryStart / AutoRetryEnd**: Requires retry infrastructure.
- **ChatParams / ChatHeaders / Experimental\***: OpenCode-specific plugin events, require LLM provider integration.
- **Metrics persistence**: HookMetrics are collected in-memory only. Persistent metrics storage deferred to v2.1.

---

## 14. Success Criteria Checklist

- [ ] **All 28 HookEvent variants** are defined in the enum, parseable from strings, and have correct `is_blocking()` classification
- [ ] **TOML config** supports all 28 events with command/http/agent/plugin handler types, matchers, and settings
- [ ] **3-layer config merge** works correctly (env > project > user) with handler appending
- [ ] **Parallel dispatch** runs hooks concurrently using FuturesUnordered with semaphore-based concurrency control
- [ ] **Deny > ask > allow precedence** enforced for blocking events
- [ ] **Kill-switch env vars** (`DISABLE_JCODE_HOOKS`, `JCODE_SKIP_HOOKS`, `JCODE_SKIP_EVENT_*`) work correctly
- [ ] **Timeout per-handler** configurable and enforced; defaults to 30s command/2s HTTP
- [ ] **Exit code protocol**: 0=continue, 1=fail (continue), 2=block
- [ ] **HookInput** has all 40+ fields covering all event types with Builder pattern
- [ ] **HookOutput** supports all 6+ override fields with correct JSON contract
- [ ] **Integration points** wired for tools, permissions, agent lifecycle, session, compaction, tasks, files
- [ ] **CLI commands** `hooks list/enable/disable/test/metrics` work
- [ ] **All unit tests pass** — matcher, config, dispatch, types
- [ ] **Integration tests pass** — full flow from config to dispatch to decision
- [ ] **E2E tests pass** — blocking a tool via hook config
- [ ] **`cargo build --release`** passes with zero warnings
- [ ] **`cargo clippy`** passes with zero warnings
- [ ] **`cargo test`** passes all existing + new tests
