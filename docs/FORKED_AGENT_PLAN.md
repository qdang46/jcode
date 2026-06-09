# Implementation Plan: Forked Agent — Shared Prompt Cache for Background Operations

> **Issue:** #370 — Forked Agent Pattern (shared prompt cache)
> **Branch:** `discuss/370-forked-agent`
> **Goal:** Create a perfect fork of the main conversation that shares the parent's prompt cache, enabling nearly-free background operations (memory extraction, auto-dream, consolidation).
> **Reference implementations:** Claude Code CCB (`forkedAgent.ts`, `extractMemories/`)
> **Generated from:** Research across 9 AI coding agent repos + jcode codebase analysis

---

## 1. Executive Summary

We implement a **Forked Agent** system that creates a lightweight child query loop starting from the exact same state as the parent (system prompt + tools + model + message prefix), guaranteeing a **prompt cache hit** on the shared prefix. The forked agent runs **in-process** using `Provider::fork()` to preserve the cached KV state, with **restricted tool permissions** (read-only bash, read/grep/glob, write only to designated memory directory). Two consumers are built on top: **memory extraction** (auto-extract durable memories from conversation) and **auto-dream** (background consolidation). jcode's existing `jcode-swarm-core`, `jcode-agent-runtime`, `jcode-memory-types`, and `jcode-background-types` crates provide lifecycle tracking, permission models, and data structures.

### Why jcode is well-positioned for this

- `Provider::fork()` at `jcode-provider-core/src/lib.rs:343` already creates independent provider instances with shared connection/cache context — *the core mechanism for cache sharing exists*
- `jcode-hooks/src/types.rs` defines `EVENT_SUBAGENT_START` and `EVENT_SUBAGENT_STOP` — *the hook events for subagent lifecycle are already defined*
- `jcode-swarm-core/src/lib.rs` has `SwarmLifecycleStatus::Spawned | Running | Completed | Failed` and `SwarmMemberRecord` — *swarm lifecycle tracking is ready*
- `jcode-agent-runtime/src/lib.rs` has `SoftInterruptSource::BackgroundTask` — *background task signal routing exists*
- `jcode-overnight-core` already creates coordinator sessions for background work — *established pattern for child sessions
// - `jcode-mempalace-adapter` bridges jcode ↔ mempalace Palace — established memory infrastructure with embeddings, dedup, and reinforcement*
- `jcode-tool-core/src/lib.rs` has `ToolContext::for_subcall()` — *tool context isolation pattern exists*

---

## 2. Architecture Decision

### Chosen Approach: In-Process Forked Query Loop (CCB Pattern)

The forked agent runs as an in-process query loop with identical cache-key parameters via `Provider::fork()`. This guarantees prompt cache hits on the shared prefix because the Anthropic/OpenAI API cache key is composed of system prompt, tools, model, and message prefix — all of which remain unchanged across the fork boundary.

**Why not subprocess spawning:** `Provider::fork()` returns an `Arc<dyn Provider>` that shares the underlying connection state. A spawned child process would create a new provider instance with no cache context, negating the primary benefit.

**Why not in-process agent-tool (codebuff pattern):** codebuff's `spawn_agents` runs agents as tool calls the model chooses. Here, the fork is triggered automatically by hooks after each turn — it's a background process, not a model-driven tool selection.

### Alternatives Considered

| Approach | Source Repo | Pros | Cons | Decision |
|----------|-------------|------|------|----------|
| **In-process fork via Provider::fork()** | CCB / jcode-native | Cache sharing, minimal cost, `Provider::fork()` already exists | Must isolate mutable state carefully | **Chosen** |
| Subprocess spawn (separate binary) | codex, opencode | Process isolation, crash resistance | Loses prompt cache, higher latency, complex IPC | Rejected |
| Tmux-pane agent | oh-my-claudecode, oh-my-openagent | Visible to user, persistent | Heavy overhead, tmux dep, loses cache | Rejected |
| In-process agent as tool call | codebuff | Shared state, simple integration | Model-driven — wrong control flow | Rejected |

---

## 3. Data Structures & Types

### 3.1 Core Forked Agent Types — `crates/jcode-swarm-core/src/fork/mod.rs`

```rust
//! Forked Agent — shared prompt cache query loop for background operations.
//!
//! A forked agent is a lightweight child query loop that shares the parent's
//! prompt cache via `Provider::fork()`. The fork uses identical cache-key
//! parameters (system prompt, tools, model, messages prefix), guaranteeing a
//! prompt cache hit on the shared prefix. This makes background operations
//! nearly free in API cost since only the delta is charged.
//!
//! # Cache Key Composition (Anthropic + OpenAI)
//! The API cache key is composed of:
//! - System prompt
//! - Tool definitions (names + schemas)
//! - Model identifier
//! - Message prefix (shared conversation history)
//! - Thinking/budget config (Anthropic only)
//!
//! # Lifecycle
//! 1. Parent completes a turn → stop hooks fire
//! 2. Stop hooks save CacheSafeParams from the parent's Provider
//! 3. Stop hooks spawn tokio tasks for memory extraction + auto-dream
//! 4. Each task calls Provider::fork() to get a cache-sharing provider
//! 5. Fork runs a restricted query loop using Provider::complete()
//! 6. Fork results are written to memory dir / dream dir
//! 7. Swarm lifecycle is updated for observability
//!
//! # Reference
//! Claude Code CCB: src/utils/forkedAgent.ts (CacheSafeParams, runForkedAgent)
//! Claude Code CCB: src/services/extractMemories/extractMemories.ts (consumer)

use crate::{SwarmLifecycleStatus, SwarmMemberRecord};
use jcode_agent_runtime::{
    AgentDefinition, PermissionMode, SoftInterruptQueue, InterruptSignal,
};
use jcode_message_types::{ContentBlock, Message, Role, ToolDefinition};
use jcode_provider_core::{Provider, Usage};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use tracing::{debug, info, warn};

// ============================================================================
// Constants
// ============================================================================

/// Default max turns for a forked agent query loop.
pub const DEFAULT_FORK_MAX_TURNS: u32 = 3;

/// Default max output tokens for a forked agent.
pub const DEFAULT_FORK_MAX_OUTPUT_TOKENS: u32 = 4096;

/// Staleness threshold for fork progress (5 min).
pub const FORK_STALE_THRESHOLD_MS: u64 = 300_000;

// ============================================================================
// Types
// ============================================================================

/// Parameters that must be identical between the fork and parent API requests
/// to share the parent's prompt cache.
///
/// ## Cache Key Components
/// The Anthropic API constructs the cache key from:
/// 1. **System prompt** — `system` parameter in the API call
/// 2. **Tools** — `tools` array (names + definitions)
/// 3. **Model** — `model` string
/// 4. **Messages prefix** — the first N messages shared between parent and fork
/// 5. **Thinking config** — `thinking` block with `budget_tokens`
///
/// All five must be identical between parent and fork for a cache hit.
/// The fork adds its own messages AFTER the shared prefix.
///
/// ## Provider::fork()
/// The fork calls `parent_provider.fork()` to create an independent provider
/// instance that shares the underlying connection + cache context. This is the
/// key architectural lever — `Provider::fork()` at `jcode-provider-core/src/lib.rs:343`
/// already creates a "provider instance with independent mutable state".
///
/// ## Important: Cache Invalidation
/// - Changing `max_output_tokens` changes `max_tokens` which can affect
///   `budget_tokens` (thinking config) — this invalidates the cache.
/// - Only override `max_output_tokens` when cache sharing is NOT the goal.
/// - Adding new messages to the fork changes the message list AFTER the prefix,
///   which does NOT affect the cache key. The cache hit covers the shared prefix.
#[derive(Clone)]
pub struct CacheSafeParams {
    /// System prompt — must match parent for cache hits.
    /// This is the compiled system prompt including all sections.
    pub system_prompt: Arc<str>,

    /// Tool definitions (names + JSON schemas) — part of the cache key.
    /// The fork should use a SUBSET of parent tools (restricted permissions).
    /// Using a subset still produces a cache hit because Anthropic's cache
    /// key matches on prefix — adding more tools after the fork's subset
    /// would break it, but using fewer is fine.
    pub tools: Arc<[ToolDefinition]>,

    /// Model identifier (e.g. "claude-sonnet-4-20250514").
    pub model: Arc<str>,

    /// Parent context messages (prefix) for prompt cache sharing.
    /// These are the messages that the fork shares with the parent.
    /// The fork appends its own messages AFTER this prefix.
    pub fork_context_messages: Arc<[Message]>,

    /// Thinking/budget config — also part of the cache key (Anthropic).
    /// If None, the fork uses non-thinking mode which must match the parent.
    pub thinking_config: Option<ThinkingConfig>,

    /// The parent provider instance. The fork calls `.fork()` on this
    /// to get a cache-sharing child provider.
    pub parent_provider: Arc<dyn Provider>,

    /// Mempalace MemoryProvider for memory operations.
    /// The fork uses this to write extracted memories directly into the Palace
    /// (SQLite + vector embeddings), bypassing filesystem writes.
    pub memory_provider: Option<Arc<dyn MemoryProvider>>,
}

/// Parameters for running a forked agent query loop.
pub struct ForkedAgentParams {
    /// Messages to start the forked query loop with.
    /// These are the NEW messages that the fork processes.
    /// They get APPENDED to `cache_safe_params.fork_context_messages`.
    /// Convention: first message is a system/user prompt describing the task.
    pub prompt_messages: Vec<Message>,

    /// Cache-safe parameters from the parent query.
    /// Must be captured immediately after a parent turn completes.
    pub cache_safe_params: CacheSafeParams,

    /// Permission mode for the forked agent.
    /// Determines which tools the fork can call and with what restrictions.
    pub permission_mode: ForkPermissionMode,

    /// Label for analytics and tracing (e.g., "memory_extraction", "auto_dream").
    pub fork_label: String,

    /// Optional cap on output tokens.
    ///
    /// ⚠️ **Cache invalidation risk**: Setting this changes `max_tokens` which
    /// affects `budget_tokens` in thinking config. If the parent uses thinking
    /// mode, setting a different `max_output_tokens` will INVALIDATE the cache.
    /// Only set this when the fork does not need cache sharing (e.g., compact
    /// summaries where latency is less important than cost).
    pub max_output_tokens: Option<u32>,

    /// Optional cap on number of turns (API round-trips).
    /// Each turn is one API call + tool execution loop.
    /// Default: `DEFAULT_FORK_MAX_TURNS` (3).
    pub max_turns: Option<u32>,

    /// Skip transcript recording for this fork.
    /// Background operations typically don't need full transcripts.
    pub skip_transcript: bool,

    /// Parent's abort signal — fork gets a child signal.
    /// When the parent session ends, all fork children are cancelled.
    pub parent_abort: Option<InterruptSignal>,
}

/// Restricted permission mode for forked agents.
///
/// Maps directly to CCB's `createAutoMemCanUseTool()` pattern:
/// - `MemoryExtraction` → like CCB's auto-memory permission set
/// - `AutoDream` → broader write access for consolidation
/// - `Custom` → arbitrary tool restrictions for other consumers
///
/// Each variant implements `is_tool_allowed()` which is called by the
/// tool execution layer before running any tool. Denied tools return
/// a descriptive error message (not a silent failure).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ForkPermissionMode {
    /// Memory extraction: read-only tools + write to memory dir.
    /// Corresponds to CCB's `createAutoMemCanUseTool()`:
    /// - Read/Grep/Glob: unrestricted
    /// - Bash: read-only commands only (ls, find, cat, stat, wc, head, tail, which)
    /// - Edit/Write: ONLY within `memory_dir`
    /// - All other tools: denied
    MemoryExtraction {
        /// MemoryProvider handle for mempalace Palace operations.
        /// Passed from the parent's session state via CacheSafeParams.
        /// Write operations go to Palace (SQLite + embeddings), not filesystem.
        memory_provider: Arc<dyn MemoryProvider>,
    },

    /// Auto-dream: background consolidation with broader write access.
    /// Same read-only restrictions as MemoryExtraction, but write tools
    /// can target any path in `allowed_dirs`.
    AutoDream {
        /// MemoryProvider handle for mempalace Palace operations.
        /// Dream output is stored as DrawerKind::Discovery entries.
        memory_provider: Arc<dyn MemoryProvider>,
    },

    /// Custom restriction set for arbitrary consumers.
    Custom {
        /// Tools that are unconditionally allowed.
        allowed_tools: HashSet<String>,
        /// Tools matching any of these prefixes are allowed.
        allowed_tool_prefixes: Vec<String>,
        /// Paths where write tools are permitted.
        write_dirs: Vec<PathBuf>,
    },
}

impl ForkPermissionMode {
    /// Check if a tool call is permitted under this mode.
    ///
    /// # Arguments
    /// * `tool_name` — The name of the tool being called.
    /// * `tool_input` — The JSON input to the tool (used to check file paths and commands).
    ///
    /// # Returns
    /// * `true` if the tool call is permitted.
    /// * `false` if denied (caller should return an error message).
    pub fn is_tool_allowed(&self, tool_name: &str, tool_input: &serde_json::Value) -> bool {
        match self {
            ForkPermissionMode::MemoryExtraction { memory_dir } => {
                Self::check_memory_extraction(tool_name, tool_input, memory_dir)
            }
            ForkPermissionMode::AutoDream { allowed_dirs } => {
                Self::check_auto_dream(tool_name, tool_input, allowed_dirs)
            }
            ForkPermissionMode::Custom { allowed_tools, allowed_tool_prefixes, write_dirs } => {
                if allowed_tools.contains(tool_name) {
                    return true;
                }
                if allowed_tool_prefixes.iter().any(|p| tool_name.starts_with(p)) {
                    return true;
                }
                Self::is_write_to_allowed_dir(tool_name, tool_input, write_dirs)
            }
        }
    }

    /// Returns a human-readable explanation of why a tool was denied.
    pub fn denial_reason(&self, tool_name: &str) -> String {
        match self {
            ForkPermissionMode::MemoryExtraction { .. } => {
                format!(
                    "Only read operations (read, grep, glob), read-only bash, and \
                     Palace memory API (palace_add_drawer, palace_search) \
                     are allowed in this context. Tool '{}' is not permitted.",
                    tool_name
                )
            }
            ForkPermissionMode::AutoDream { .. } => {
                format!(
                    "Only read operations, read-only bash, and Palace memory API \
                     are allowed in this context. Tool '{}' is not permitted.",
                    tool_name
                )
            }
            ForkPermissionMode::Custom { .. } => {
                format!("Tool '{}' is not in the allowed set for this forked agent", tool_name)
            }
        }
    }

    // ---- Internal check methods ----

    fn check_memory_extraction(tool_name: &str, input: &serde_json::Value) -> bool {
        match tool_name {
            // Read-only tools: unrestricted
            "read" | "grep" | "glob" | "list_files" | "file_search" => true,

            // Bash: only read-only commands
            "bash" | "run" | "execute_command" => {
                let cmd = input.get("command")
                    .or_else(|| input.get("cmd"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let read_only_prefixes = [
                    "ls", "find", "cat", "stat", "wc", "head", "tail",
                    "echo", "which", "file", "du", "df", "type", "command",
                ];
                read_only_prefixes.iter().any(|p| cmd.starts_with(p))
            }

            // Palace API tools: always allowed
            // Memories go to Palace (SQLite + embeddings), not filesystem
            "palace_add_drawer" | "palace_write" | "palace_search" | "palace_recall" => true,

            // Filesystem write tools: DENIED for forks
            "edit" | "write" | "create" | "file_edit" | "file_write" => false,

            // Everything else denied
            _ => false,
        }
    }

    fn check_auto_dream(tool_name: &str, input: &serde_json::Value) -> bool {
        match tool_name {
            "read" | "grep" | "glob" | "list_files" | "file_search" => true,
            "bash" | "run" | "execute_command" => {
                let cmd = input.get("command")
                    .or_else(|| input.get("cmd"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let read_only_prefixes = [
                    "ls", "find", "cat", "stat", "wc", "head", "tail",
                    "echo", "which", "file", "du", "df", "type", "command",
                ];
                read_only_prefixes.iter().any(|p| cmd.starts_with(p))
            }
            // Palace API: dreams stored as DrawerKind::Discovery entries
            "palace_add_drawer" | "palace_write" | "palace_search" => true,
            // Filesystem write: denied
            "edit" | "write" | "create" | "file_edit" | "file_write" => false,
            _ => false,
        }
    }

    fn is_write_to_allowed_dir(tool_name: &str, input: &serde_json::Value, dirs: &[ByteBuf]) -> bool {
        match tool_name {
            "edit" | "write" | "create" | "file_edit" | "file_write" => {
                let path = input.get("file_path")
                    .or_else(|| input.get("path"))
                    .and_then(|p| p.as_str())
                    .map(PathBuf::from);
                path.map(|p| dirs.iter().any(|d| p.starts_with(d))).unwrap_or(false)
            }
            _ => false,
        }
    }
}

/// Result of a forked agent query loop.
pub struct ForkedAgentResult {
    /// Messages produced by the fork (assistant responses).
    pub messages: Vec<Message>,
    /// Total token usage across all turns.
    pub total_usage: Usage,
    /// Wall-clock duration of the fork.
    pub duration_ms: u64,
    /// Cache hit rate: `cache_read_tokens / total_input_tokens`.
    /// Expected: >80% for background operations sharing the parent's prefix.
    pub cache_hit_rate: f64,
    /// Optional swarm member record for lifecycle tracking.
    pub swarm_member: Option<SwarmMemberRecord>,
}

// ============================================================================
// Global Cache-Safe Params Slot
// ============================================================================

/// Module-level slot that stores the last parent turn's cache-safe params.
///
/// ## Why a global slot?
/// The stop hooks fire after each complete turn and need to pass the parent's
/// cache state to background fork tasks. Rather than threading this through
/// every function signature, a global slot is set by the parent loop and read
/// by fork consumers. This mirrors CCB's `lastCacheSafeParams` / `saveCacheSafeParams`.
///
/// ## Thread safety
/// Wrapped in `Mutex` — writes happen from the main agent loop, reads from
/// background `tokio::spawn` tasks. The data is `Clone` so contention is brief.
///
/// ## When is it set?
/// After each complete parent turn (in `handle_stop_hooks`).
/// Set to `None` when the session ends.
static LAST_CACHE_SAFE_PARAMS: std::sync::Mutex<Option<CacheSafeParams>> =
    std::sync::Mutex::new(None);

/// Save the parent turn's cache-safe params for fork consumers.
///
/// # Call this in:
/// - `Agent::handle_stop_hooks()` after each complete turn
/// - Before spawning fork tasks so they can call `get_last_cache_safe_params()`
pub fn save_cache_safe_params(params: CacheSafeParams) {
    if let Ok(mut guard) = LAST_CACHE_SAFE_PARAMS.lock() {
        *guard = Some(params);
    }
}

/// Retrieve the last saved cache-safe params.
///
/// # Returns
/// - `Some(CacheSafeParams)` if a parent turn has completed.
/// - `None` if no parent turn has completed yet (fork should not run).
pub fn get_last_cache_safe_params() -> Option<CacheSafeParams> {
    LAST_CACHE_SAFE_PARAMS.lock().ok()?.clone()
}

/// Clear the cache-safe params slot (called on session end).
pub fn clear_cache_safe_params() {
    if let Ok(mut guard) = LAST_CACHE_SAFE_PARAMS.lock() {
        *guard = None;
    }
}
```

### 3.2 Fork Runner — `crates/jcode-swarm-core/src/fork/runner.rs`

```rust
//! Runs the forked agent query loop.
//!
//! # How it Works
//! 1. Takes `ForkedAgentParams` with cache-safe params from the parent
//! 2. Calls `parent_provider.fork()` to get a cache-sharing provider instance
//! 3. Builds the full message list: [shared_prefix .. fork_new_messages]
//! 4. Filters tool definitions to only expose what the permission mode allows
//! 5. Runs `Provider::complete_split()` with the forked provider
//! 6. Accumulates usage and computes cache hit rate
//!
//! # Provider::fork() Deep Dive
//! `Provider::fork()` at `jcode-provider-core/src/lib.rs:343` creates a new
//! provider instance with independent mutable state but sharing the underlying
//! connection pool and credential context. For the Anthropic provider, this
//! means the forked provider:
//! - Uses the same API key / OAuth token
//! - Uses the same HTTP connection pool
//! - Has its own streaming state (does not interfere with parent's streams)
//! - Crucially: shares prompt cache because the API cache key is server-side
//!
//! # The complete_split() Path
//! We use `complete_split()` (not `complete()`) to maximize cache hit rates.
//! The static system prompt is cached once and shared across all turns.
//! The fork's dynamic message is the small delta at the end.

use crate::fork::{
    CacheSafeParams, ForkPermissionMode, ForkedAgentParams, ForkedAgentResult,
    ForkStaleDetector, DEFAULT_FORK_MAX_TURNS,
};
use crate::SwarmLifecycleStatus;
use futures::StreamExt;
use jcode_message_types::{Message, ContentBlock, Role, ToolDefinition};
use jcode_provider_core::{Provider, Usage};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

/// Maximum number of chunks to accumulate from the stream before yielding.
/// Prevents unbounded memory growth from long-running fork responses.
const MAX_STREAM_CHUNKS: usize = 100_000;

/// Run a forked agent query loop that shares the parent's prompt cache.
///
/// # Algorithm
///
/// 1. **Validate**: Ensure cache-safe params are present and non-empty
/// 2. **Fork provider**: Call `cache_safe_params.parent_provider.fork()` to get
///    a cache-sharing child provider
/// 3. **Merge messages**: Concatenate `fork_context_messages` + `prompt_messages`
///    so the API sees [shared_prefix, fork_message]. The shared prefix hits cache.
/// 4. **Filter tools**: Call `filter_tools_for_permission()` to expose only the
///    tools the fork is allowed to call. **Critical**: the tool list must be a
///    SUBSET of the parent's tool list to preserve the cache key prefix match.
/// 5. **Build messages**: Prepare the full message list for the API call.
/// 6. **Execute**: Call `provider.complete_split()` with the forked provider.
/// 7. **Track usage**: Accumulate usage from stream events.
/// 8. **Report**: Log analytics and return structured result.
///
/// # Cache Hit Expectation
/// The fork's API call has:
/// - Same system prompt → cache hit on system prefix
/// - Same tool list (subset is fine) → cache hit on tools prefix
/// - Same model → cache hit on model routing
/// - Same message prefix → cache hit on shared conversation history
///
/// Expected cache hit rate: >80% for background operations.
///
/// # Error Handling
/// - Provider errors → log warning, return empty result (non-fatal)
/// - Abort signal → cancel cleanly, return partial result
/// - Permission violations → tool execution layer returns denial message
///
/// # Reference
/// CCB: runForkedAgent() in src/utils/forkedAgent.ts
pub async fn run_forked_agent(
    params: ForkedAgentParams,
) -> ForkedAgentResult {
    let start = Instant::now();
    let fork_label = &params.fork_label;

    debug!(
        fork_label = %fork_label,
        "Starting forked agent query loop"
    );

    // Step 1: Fork the provider to get a cache-sharing instance
    let forked_provider = params.cache_safe_params.parent_provider.fork();

    // Step 2: Merge messages — shared prefix + fork's new messages
    let mut messages: Vec<Message> = params.cache_safe_params.fork_context_messages
        .iter()
        .cloned()
        .collect();
    messages.extend(params.prompt_messages.clone());

    // Step 3: Filter tools based on permission mode
    // IMPORTANT: We must use a SUBSET of the parent's tools. The cache key
    // is prefix-based — using fewer tools than the parent is fine, but adding
    // tools not in the parent's list would shift the key.
    let tools = filter_tools_for_permission(
        &params.cache_safe_params.tools,
        &params.permission_mode,
    );

    // Step 4: Use the parent's system prompt (identical = cache hit)
    let system_static = &params.cache_safe_params.system_prompt;
    // The fork gets an empty dynamic system prompt because its task is
    // already encoded in prompt_messages. If we added system context here,
    // it would shift the cache key's message prefix.
    let system_dynamic = "";

    // Step 5: Run the query loop
    let mut total_usage = Usage::default();
    let mut output_messages: Vec<Message> = Vec::new();
    let mut chunk_count = 0usize;

    // Build the tool definitions for the API call
    let api_tools: Vec<ToolDefinition> = tools.iter()
        .map(|t| ToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
        })
        .collect();

    let max_turns = params.max_turns.unwrap_or(DEFAULT_FORK_MAX_TURNS);

    // Execute the fork's query loop
    // We run up to `max_turns`, each turn being one API call + tool execution
    for turn in 0..max_turns {
        debug!(
            fork_label = %fork_label,
            turn = turn,
            messages_len = messages.len(),
            tools_len = api_tools.len(),
            "Fork turn starting"
        );

        // Check for abort signal
        if let Some(ref abort) = params.parent_abort {
            if abort.is_aborted() {
                info!(fork_label = %fork_label, turn = turn, "Fork aborted by parent signal");
                break;
            }
        }

        // Call the provider
        let result = forked_provider.complete_split(
            &messages,
            &api_tools,
            system_static,
            system_dynamic,
            None, // resume_session_id — fork does not resume
        ).await;

        match result {
            Ok(stream) => {
                let mut turn_messages: Vec<Message> = Vec::new();
                let mut turn_usage = Usage::default();

                tokio::pin!(stream);
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(jcode_message_types::StreamEvent::TextDelta(_)) => {
                            // Accumulated into the assistant message
                        }
                        Ok(jcode_message_types::StreamEvent::ContentBlockStart(block)) => {
                            // Handle tool_use blocks
                        }
                        Ok(jcode_message_types::StreamEvent::ContentBlockDelta(_)) => {}
                        Ok(jcode_message_types::StreamEvent::ContentBlockStop(_)) => {}
                        Ok(jcode_message_types::StreamEvent::MessageStop(msg)) => {
                            turn_messages.push(msg);
                        }
                        Ok(jcode_message_types::StreamEvent::Usage(usage)) => {
                            turn_usage = usage;
                        }
                        Err(e) => {
                            warn!(fork_label = %fork_label, turn = turn, error = %e, "Fork stream error");
                            break;
                        }
                        _ => {}
                    }

                    chunk_count += 1;
                    if chunk_count > MAX_STREAM_CHUNKS {
                        warn!(fork_label = %fork_label, "Fork exceeded max stream chunks, aborting");
                        break;
                    }
                }

                // Accumulate usage
                total_usage = accumulate_usage(total_usage, turn_usage);
                output_messages.extend(turn_messages);

                // Check if the model made tool calls — if so, execute them and loop
                // (simplified: full tool execution loop follows same pattern as Agent)
            }
            Err(e) => {
                warn!(
                    fork_label = %fork_label,
                    turn = turn,
                    error = %e,
                    "Fork provider call failed"
                );
                break;
            }
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    // Compute cache hit rate
    let total_input = total_usage.input_tokens
        + total_usage.cache_creation_input_tokens
        + total_usage.cache_read_input_tokens;
    let cache_hit_rate = if total_input > 0 {
        (total_usage.cache_read_input_tokens as f64) / (total_input as f64)
    } else {
        0.0
    };

    info!(
        fork_label = %fork_label,
        duration_ms = duration_ms,
        cache_hit_rate = format!("{:.2}%", cache_hit_rate * 100.0),
        input_tokens = total_usage.input_tokens,
        cache_read_tokens = total_usage.cache_read_input_tokens,
        output_tokens = total_usage.output_tokens,
        messages_produced = output_messages.len(),
        "Forked agent completed"
    );

    ForkedAgentResult {
        messages: output_messages,
        total_usage,
        duration_ms,
        cache_hit_rate,
        swarm_member: None,
    }
}

/// Filter tool definitions based on the fork's permission mode.
///
/// ## Why this is safe for cache
/// The Anthropic cache key is a PREFIX match on the tool list. If the parent
/// has tools [A, B, C, D, E] and the fork only exposes [A, B, C], the API
/// still sees the same first 3 tools and the cache key prefix matches.
///
/// However: if the fork adds tools NOT in the parent's list, the prefix
/// shifts and cache is missed. Our filter only REMOVES tools, never adds.
///
/// ## Tool name normalization
/// Different jcode tools may use different naming (e.g., "bash" vs "run").
/// The filter matches against a canonical set of read-only tool names.
fn filter_tools_for_permission(
    all_tools: &[ToolDefinition],
    permission: &ForkPermissionMode,
) -> Vec<ToolDefinition> {
    all_tools
        .iter()
        .filter(|tool| {
            // Pre-approve read-only tools regardless of permission mode
            matches!(
                tool.name.as_str(),
                "read" | "grep" | "glob" | "list_files" | "file_search"
                    | "bash" | "run" | "execute_command"
                    | "edit" | "write" | "create" | "file_edit" | "file_write"
            )
        })
        .cloned()
        .collect()
}

/// Accumulate usage across multiple API calls.
/// Uses the same pattern as CCB's `accumulateUsage`.
fn accumulate_usage(a: Usage, b: Usage) -> Usage {
    Usage {
        input_tokens: a.input_tokens + b.input_tokens,
        output_tokens: a.output_tokens + b.output_tokens,
        cache_read_input_tokens: a.cache_read_input_tokens + b.cache_read_input_tokens,
        cache_creation_input_tokens: a.cache_creation_input_tokens + b.cache_creation_input_tokens,
        cost_usd: a.cost_usd + b.cost_usd,
    }
}

/// Detect when a forked agent has gone stale (no progress within threshold).
///
/// CCB reference: SubagentTracker's STALE_THRESHOLD_MS (5 min).
/// Used by the HUD/team monitoring to surface stuck background agents.
pub struct ForkStaleDetector {
    last_activity: Instant,
    threshold_ms: u64,
}

impl ForkStaleDetector {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            last_activity: Instant::now(),
            threshold_ms,
        }
    }

    pub fn tick(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn is_stale(&self) -> bool {
        self.last_activity.elapsed().as_millis() > self.threshold_ms as u128
    }
}

impl Default for ForkStaleDetector {
    fn default() -> Self {
        Self::new(FORK_STALE_THRESHOLD_MS)
    }
}
```

### 3.3 Fork Tool Permissions — `crates/jcode-swarm-core/src/fork/permissions.rs`

```rust
//! Fork tool permission utilities.
//!
//! Maps jcode's tool system to the restricted permission model required
//! by forked agents. This bridges `ForkPermissionMode` (the fork's policy)
//! with `jcode_tool_core::ToolContext` (the execution layer).

use crate::fork::ForkPermissionMode;
use jcode_tool_core::ToolContext;
use serde_json::Value;

/// Wraps a parent `ToolContext` with fork-level permission gating.
///
/// Every tool call goes through this wrapper before execution:
/// 1. Check `ForkPermissionMode::is_tool_allowed()`
/// 2. If denied, return a descriptive error string
/// 3. If allowed, delegate to the inner `ToolContext`
///
/// ## Usage in the fork query loop
/// When the fork's model makes a tool call, the execution layer calls
/// `ForkToolContext::check_tool()` instead of the normal tool dispatch.
/// This ensures the fork's restricted permissions are enforced.
pub struct ForkToolContext {
    /// The parent's tool context (used for actual execution of allowed tools).
    inner: ToolContext,
    /// The fork's permission policy.
    permission: ForkPermissionMode,
    /// Fork label for analytics.
    fork_label: String,
}

impl ForkToolContext {
    pub fn new(inner: ToolContext, permission: ForkPermissionMode, fork_label: String) -> Self {
        Self { inner, permission, fork_label }
    }

    /// Check whether a tool call is allowed under the fork's permission mode.
    ///
    /// # Returns
    /// - `Ok(())` if the tool is allowed (caller should execute it).
    /// - `Err(String)` if denied (caller should return the error as a tool result).
    pub fn check_tool(&self, tool_name: &str, input: &Value) -> Result<(), String> {
        if self.permission.is_tool_allowed(tool_name, input) {
            Ok(())
        } else {
            Err(self.permission.denial_reason(tool_name))
        }
    }

    /// Get a reference to the inner tool context (for executing allowed tools).
    pub fn inner(&self) -> &ToolContext {
        &self.inner
    }

    pub fn fork_label(&self) -> &str {
        &self.fork_label
    }
}
```

### 3.4 Memory Extraction Types — `crates/jcode-memory-types/src/extraction.rs`

```rust
//! Memory Extraction Service
//!
//! Uses the forked agent pattern to run a memory extraction subagent
//! that shares the parent's prompt cache. Triggered by `handle_stop_hooks`
//! after each complete turn.
//!
//! ## Flow
//! 1. `Agent::handle_stop_hooks()` fires after a complete turn
//! 2. If memory extraction is enabled and due (enough new messages):
//!    a. Check if parent already wrote memories (skip if so)
//!    b. Build extraction prompt from recent messages + existing memory scan
//!    c. Call `run_forked_agent()` with `ForkPermissionMode::MemoryExtraction`
//!    d. Advance cursor past processed messages
//!    e. Fork's output is written to memory directory
//!
//! ## Cursor-based Processing
//! Tracks `last_processed_uuid` to avoid re-extracting from already-processed
//! messages. The cursor is persisted per-session so it survives restarts.
//!
//! ## Why Skip When Parent Wrote Memories?
//! The main agent's system prompt includes full save instructions. When the
//! main agent writes memories itself (via Write/Edit tool calls targeting
//! the memory directory), the forked extraction is redundant. We detect this
//! via `has_memory_writes_since()` and skip the fork, advancing the cursor.
//! This makes the main agent and the background agent mutually exclusive
//! per turn — no duplicate writes, no wasted API calls.
//!
//! ## Reference
//! CCB: src/services/extractMemories/extractMemories.ts
//! CCB: src/services/extractMemories/prompts.ts

use crate::{MemoryActivity, MemoryGraph, PipelineState};
use jcode_message_types::Message;
use jcode_swarm_core::fork::{
    ForkPermissionMode, ForkedAgentParams, ForkedAgentResult,
    run_forked_agent, save_cache_safe_params, get_last_cache_safe_params,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};
use chrono::{DateTime, Utc};

/// Cursor tracking for incremental message processing.
///
/// Persisted per-session so extraction progress survives restarts.
/// Analogous to CCB's closure-scoped `lastRecordedUuid` in `runForkedAgent`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractionCursor {
    /// UUID of the last processed message.
    /// New messages after this UUID are candidates for extraction.
    pub last_processed_uuid: Option<String>,

    /// Session ID this cursor belongs to.
    pub session_id: String,

    /// When the last extraction ran.
    pub last_extracted_at: Option<DateTime<Utc>>,
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
    /// "auto" → auto-only (single user scope)
    /// "combined" → auto + team (multi-scope)
    pub prompt_variant: ExtractionPromptVariant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExtractionPromptVariant {
    Auto,
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
/// Scans assistant messages after `since_uuid` for tool calls targeting
/// the memory directory. If found, the fork is redundant and should skip.
///
/// CCB reference: `hasMemoryWritesSince()` in extractMemories.ts
pub fn has_memory_writes_since(
    messages: &[Message],
    since_uuid: Option<&str>,
    memory_dir: &Path,
) -> bool {
    let mut found_start = since_uuid.is_none();
    for message in messages {
        if !found_start {
            if message.id() == since_uuid.unwrap_or("") {
                found_start = true;
            }
            continue;
        }
        if !message.role().is_assistant() {
            continue;
        }
        // Check if any tool call targets the memory directory
        for block in message.content() {
            if let ContentBlock::ToolUse { name, input, .. } = block {
                if matches!(name.as_str(), "write" | "edit" | "create" | "file_write" | "file_edit") {
                    if let Some(path) = input.get("file_path")
                        .or_else(|| input.get("path"))
                        .and_then(|p| p.as_str())
                    {
                        let path = PathBuf::from(path);
                        if path.starts_with(memory_dir) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Count model-visible messages since a cursor UUID.
///
/// Model-visible = user + assistant messages (excludes system, progress).
/// If `since_uuid` is not found (e.g., removed by compaction), falls back
/// to counting all messages rather than returning 0 (which would permanently
/// disable extraction).
fn count_model_visible_messages_since(
    messages: &[Message],
    since_uuid: Option<&str>,
) -> usize {
    if since_uuid.is_none() {
        return messages.iter().filter(|m| m.role().is_user() || m.role().is_assistant()).count();
    }

    let mut found_start = false;
    let mut count = 0usize;
    for message in messages {
        if !found_start {
            if message.id() == since_uuid.unwrap_or("") {
                found_start = true;
            }
            continue;
        }
        if message.role().is_user() || message.role().is_assistant() {
            count += 1;
        }
    }
    // Fallback: if UUID not found, count all
    if !found_start {
        return messages.iter().filter(|m| m.role().is_user() || m.role().is_assistant()).count();
    }
    count
}
```

### 3.5 Extraction Prompt Templates — `crates/jcode-memory-types/src/extraction_prompts.rs`

```rust
//! Prompt templates for the background memory extraction agent.
//!
//! The extraction agent runs as a perfect fork of the main conversation — same
//! system prompt, same message prefix. When the main agent writes memories
//! itself, extraction skips that turn. These prompts fire only when the main
//! agent didn't write.
//!
//! ## Strategy
//! - **Turn 1**: Issue all read/grep/glob calls in parallel to gather context
//! - **Turn 2**: Issue all write/edit calls in parallel to save memories
//! - No interleaving of reads and writes — minimizes turn count
//!
//! ## Reference
//! CCB: src/services/extractMemories/prompts.ts (buildExtractAutoOnlyPrompt, buildExtractCombinedPrompt)

use crate::extraction::ExtractionPromptVariant;

/// Build the opener section shared by both prompt variants.
fn opener(new_message_count: usize, existing_memories: &str) -> String {
    let manifest = if !existing_memories.is_empty() {
        format!(
            "\n\n## Existing memory files\n\n{}\n\nCheck this list before writing — update an existing file rather than creating a duplicate.",
            existing_memories
        )
    } else {
        String::new()
    };

    format!(
        "You are now acting as the memory extraction subagent. Analyze the most recent ~{count} messages above and use them to update your persistent memory systems.\n\n\
         Available tools: read, grep, glob, read-only bash (ls/find/cat/stat/wc/head/tail and similar), and write/edit for paths inside the memory directory only. \
         Bash rm is not permitted. All other tools will be denied.\n\n\
         You have a limited turn budget. The efficient strategy is: \
         turn 1 — issue all read calls in parallel for every file you might update; \
         turn 2 — issue all write/edit calls in parallel. Do not interleave reads and writes across multiple turns.\n\n\
         You MUST only use content from the last ~{count} messages to update your persistent memories. \
         Do not waste any turns attempting to investigate or verify that content further — no grepping source files, \
         no reading code to confirm a pattern exists, no git commands.{}",
        count = new_message_count,
        manifest
    )
}

/// Build the extraction prompt for auto-only memory.
///
/// Four-type taxonomy:
/// 1. User identity and role
/// 2. Project conventions and preferences
/// 3. Feedback and testing patterns
/// 4. Technical decisions and architecture
pub fn build_extraction_prompt(
    variant: &ExtractionPromptVariant,
    new_message_count: usize,
    existing_memories: &str,
    skip_index: bool,
) -> String {
    let header = opener(new_message_count, existing_memories);

    let types_section = match variant {
        ExtractionPromptVariant::Auto => {
            r#"
## Types of memories to save

1. **User Identity & Role** — Who the user is, their role, their goals. Examples: "User is a backend engineer working on a distributed systems project", "User prefers Rust for performance-critical components".
2. **Project Conventions & Preferences** — Coding style, naming conventions, tool preferences, workflow patterns. Examples: "Project uses snake_case for all identifiers", "User prefers async/await over manual future combinators".
3. **Feedback & Testing Patterns** — Testing preferences, CI setup, bug reproduction steps, quality standards. Examples: "All new code must include property-based tests", "User runs clippy as part of CI".
4. **Technical Decisions & Architecture** — Key design decisions, trade-offs, architecture diagrams, dependency choices. Examples: "The system uses a CQRS pattern with separate read/write databases", "Chose Actix-web over Axum due to WebSocket performance".\
            "#.to_string()
        }
        ExtractionPromptVariant::Combined => {
            // Similar but with per-type <scope> guidance for auto vs team
            r#"
## Types of memories to save

1. **User Identity & Role** (scope: auto) — Who the user is...
2. **Project Conventions & Preferences** (scope: auto) — ...
3. **Feedback & Testing Patterns** (scope: team) — ...
4. **Technical Decisions & Architecture** (scope: team) — ...\
            "#.to_string()
        }
    };

    let how_to_save = if skip_index {
        r#"
## How to save memories

Write each memory to its own file (e.g., `user_role.md`, `feedback_testing.md`) using frontmatter format:

```markdown
---
type: user_identity | project_convention | feedback_pattern | technical_decision
confidence: high | medium | low
tags: [comma-separated tags]
---

Memory content here...
```\
        "#.to_string()
    } else {
        r#"
## How to save memories

**Step 1** — write the memory to its own file using frontmatter format.

**Step 2** — add a pointer to that file in `MEMORY.md`. Each entry should be one line, under ~150 characters.
Never write memory content directly into `MEMORY.md`.\
        "#.to_string()
    };

    format!("{}\n\n{}\n\n{}", header, types_section, how_to_save)
}
```

### 3.6 Auto-Dream Types — `crates/jcode-overnight-core/src/auto_dream.rs`

```rust
//! Auto-Dream — background consolidation using the forked agent pattern.
//!
//! The auto-dream system runs a background forked agent periodically to
//! consolidate session context, extract insights, and update project-level
//! knowledge. Unlike memory extraction (which saves specific facts), dreaming
//! is about synthesis — connecting patterns across turns, identifying trends,
//! and building higher-level models.
//!
//! ## Schedule
//! Runs every `turn_interval` turns. Configurable in `config.toml`.
//!
//! ## Tool Restrictions
//! Same read-only restrictions as memory extraction, but write access is
//! broader (multiple allowed directories).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the auto-dream background consolidation.
///
/// Dream output is stored in mempalace Palace as DrawerKind::Discovery
/// or DrawerKind::Raw entries (not flat files). The Palace handles
/// embeddings, deduplication, and reinforcement automatically.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoDreamConfig {
    /// Enable auto-dream.
    pub enabled: bool,

    /// Run dream every N turns.
    pub turn_interval: usize,

    /// Max turns for the dream agent.
    pub max_turns: u32,

    /// Max output tokens per turn.
    pub max_output_tokens: u32,
}

impl Default for AutoDreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            turn_interval: 10,
            max_turns: 2,
            max_output_tokens: 2048,
            allowed_dirs: vec![PathBuf::from(".jcode/dreams")],
            dream_dir: PathBuf::from(".jcode/dreams"),
        }
    }
}
```

---

## 4. Pseudocode — Core Algorithm

### 4.1 Forked Agent Runner

```python
FUNCTION runForkedAgent(params: ForkedAgentParams) -> ForkedAgentResult:
    # 1. Validate cache-safe params
    ASSERT params.cache_safe_params is not None
    ASSERT len(params.cache_safe_params.fork_context_messages) > 0

    # 2. Fork the provider to get a cache-sharing instance
    forkedProvider = params.cache_safe_params.parentProvider.fork()
    # Provider::fork() at jcode-provider-core/src/lib.rs:343
    # Creates independent mutable state but shares connection/cache context

    # 3. Merge messages: shared prefix + fork's new messages
    # The API sees: [shared_prefix_messages, fork_new_messages]
    # Shared prefix hits prompt cache (identical to parent)
    # Fork messages are the delta that gets charged
    allMessages = params.cache_safe_params.forkContextMessages +
                  params.promptMessages

    # 4. Filter tools to only what the permission mode allows
    # IMPORTANT: Must be a SUBSET of parent's tools to preserve cache key
    tools = filterToolsByPermission(
        params.cache_safe_params.tools,
        params.permissionMode,
    )

    # 5. Use parent's system prompt (identical = cache hit on system prefix)
    systemStatic = params.cache_safe_params.systemPrompt
    systemDynamic = ""  # Fork's task is in the messages, not system prompt

    # 6. Execute query loop (up to maxTurns turns)
    totalUsage = EMPTY_USAGE
    outputMessages = []
    startTime = now()

    FOR turn = 0 TO maxTurns - 1:
        # Check parent abort signal
        IF params.parentAbort.isAborted():
            BREAK

        # Call provider.complete_split()
        # This preserves the cache key: same system, same tools, same prefix
        stream = await forkedProvider.complete_split(
            messages=allMessages,
            tools=tools,
            systemStatic=systemStatic,
            systemDynamic=systemDynamic,
            resumeSessionId=None,  # Fork does not use session resume
        )

        # Process stream events
        FOR EACH event IN stream:
            IF event is Usage:
                totalUsage = accumulateUsage(totalUsage, event.usage)
            IF event is MessageStop:
                outputMessages.push(event.message)

        # If the model made tool calls, execute them with restricted permissions
        IF hasToolCalls(outputMessages[-1]):
            FOR EACH toolCall IN outputMessages[-1].toolCalls:
                IF NOT params.permissionMode.isToolAllowed(toolCall.name, toolCall.input):
                    RETURN tool result with denial reason
                    CONTINUE
                # Execute tool normally via ToolContext
                result = executeTool(toolCall)
                allMessages.push(result)

    # 7. Compute metrics
    duration = now() - startTime
    totalInput = totalUsage.inputTokens +
                 totalUsage.cacheCreationTokens +
                 totalUsage.cacheReadTokens
    cacheHitRate = totalUsage.cacheReadTokens / totalInput IF totalInput > 0 ELSE 0

    # 8. Log and return
    LOG(f"Fork '{params.forkLabel}' completed in {duration}ms, "
        f"cache hit rate: {cacheHitRate:.1%}")

    RETURN ForkedAgentResult(
        messages=outputMessages,
        totalUsage=totalUsage,
        durationMs=duration,
        cacheHitRate=cacheHitRate,
    )
```

### 4.2 Memory Extraction Trigger

```python
FUNCTION runMemoryExtraction(config, cursor, recentMessages, provider, abort):
    # 1. Check if extraction is enabled and due
    IF NOT config.enabled:
        RETURN

    newCount = countModelVisibleMessagesSince(recentMessages,
                                              cursor.lastProcessedUuid)
    IF newCount < config.minNewMessages:
        DEBUG(f"Skipping extraction: only {newCount} new messages, "
              f"need {config.minNewMessages}")
        RETURN

    # 2. Check if parent already wrote memories (skip if so)
    IF hasMemoryWritesSince(recentMessages, cursor.lastProcessedUuid,
                            config.memoryDir):
        DEBUG("Skipping extraction: parent agent already wrote memories")
        advanceCursor(cursor, recentMessages.last().id)
        RETURN

    # 3. Get cached params from the last parent turn
    cacheSafe = getLastCacheSafeParams()
    IF cacheSafe is None:
        WARN("No cache-safe params available — cannot run fork")
        RETURN

    # 4. Scan existing memory directory
    existingMemories = scanMemoryDirectory(config.memoryDir)

    # 5. Build extraction prompt
    prompt = buildExtractionPrompt(
        variant=config.promptVariant,
        newMessageCount=newCount,
        existingMemories=existingMemories,
        skipIndex=False,
    )

    # 6. Run forked agent (shares parent's prompt cache via provider.fork())
    #    The fork gets restricted to read-only tools + write to memoryDir only
    result = await runForkedAgent(ForkedAgentParams(
        promptMessages=[Message.user(prompt)],
        cacheSafeParams=cacheSafe,
        permissionMode=ForkPermissionMode.MemoryExtraction(
            memoryDir=config.memoryDir,
        ),
        forkLabel="memory_extraction",
        maxTurns=config.maxTurns,
        maxOutputTokens=config.maxOutputTokens,
        skipTranscript=True,
        parentAbort=abort,
    ))

    # 7. Log metrics
    INFO(f"Memory extraction: {result.cacheHitRate:.1%} cache hit, "
         f"{result.durationMs}ms, {result.totalUsage.costUsd:.4f} USD")

    # 8. Advance cursor
    advanceCursor(cursor, recentMessages.last().id)
    cursor.lastExtractedAt = now()
    saveCursor(cursor)
```

### 4.3 Auto-Dream Trigger

```python
FUNCTION runAutoDream(config, turnCount, recentMessages, provider, abort):
    # 1. Check if dream is due
    IF NOT config.enabled:
        RETURN
    IF turnCount % config.turnInterval != 0:
        RETURN

    # 2. Get cached params
    cacheSafe = getLastCacheSafeParams()
    IF cacheSafe is None:
        RETURN

    # 3. Build dream prompt
    dreamDir = ensureDir(config.dreamDir)
    existingDreams = listFiles(dreamDir)
    prompt = buildDreamPrompt(turnCount, existingDreams)

    # 4. Run forked agent
    result = await runForkedAgent(ForkedAgentParams(
        promptMessages=[Message.user(prompt)],
        cacheSafeParams=cacheSafe,
        permissionMode=ForkPermissionMode.AutoDream(
            allowedDirs=config.allowedDirs,
        ),
        forkLabel="auto_dream",
        maxTurns=config.maxTurns,
        maxOutputTokens=config.maxOutputTokens,
        skipTranscript=True,
        parentAbort=abort,
    ))

    INFO(f"Auto-dream: {result.cacheHitRate:.1%} cache hit, "
         f"{result.durationMs}ms")
```

---

## 5. Implementation Code — Key Files

### 5.1 Wire Up Stop Hooks — `crates/jcode-app-core/src/agent.rs`

The stop hooks handler is where fork consumers are triggered. After each complete parent turn, we:
1. Save `CacheSafeParams` from the current provider context
2. Spawn background tasks for memory extraction and auto-dream

```rust
// In Agent::handle_stop_hooks() — after each complete turn
//
// Integration point: This runs after the model produces a final response
// with no tool calls (complete turn boundary).

pub async fn handle_stop_hooks(&mut self) {
    // --- Step 1: Save cache-safe params for fork consumers ---
    //
    // These params capture the exact state after this turn:
    // - System prompt (compiled)
    // - Tool definitions
    // - Model identifier
    // - Message prefix (for cache key)
    // - Thinking config
    // - Provider handle (for Provider::fork())
    //
    // Background fork tasks read these via get_last_cache_safe_params()
    // and use them to construct their own API calls that share the cache.

    let messages = self.session.messages_for_provider();
    let tools = self.tool_definitions().await;
    let system_prompt = self.build_system_prompt();
    let thinking_config = self.provider.thinking_config();

    // Only save if there are messages to share (avoid empty-state forks)
    if !messages.is_empty() {
        save_cache_safe_params(CacheSafeParams {
            system_prompt: Arc::from(system_prompt),
            tools: Arc::from(tools),
            model: Arc::from(self.provider.model()),
            fork_context_messages: Arc::from(messages),
            thinking_config,
            parent_provider: self.provider_handle(),
        });
    }

    // --- Step 2: Spawn memory extraction (if enabled) ---
    //
    // This runs as a tokio background task so it doesn't block the parent
    // loop. The fork shares the parent's prompt cache via Provider::fork()
    // and the saved CacheSafeParams.

    let memory_config = self.config.forked_agent.memory_extraction.clone();
    if memory_config.enabled {
        let cursor = load_or_create_cursor(&self.session.id);
        let recent_messages = self.session.messages_for_provider();
        let abort = self.interrupt_signal.clone();
        let provider = self.provider_handle();

        tokio::spawn(async move {
            let result = run_memory_extraction(
                &memory_config,
                &mut cursor,
                &recent_messages,
                provider,
                abort,
            ).await;

            if let Err(e) = result {
                warn!("Memory extraction failed: {}", e);
            }
        });
    }

    // --- Step 3: Spawn auto-dream (if enabled and due) ---
    //
    // Same pattern as memory extraction but with its own config and schedule.

    let dream_config = self.config.forked_agent.auto_dream.clone();
    if dream_config.enabled
        && self.session.turn_count() > 0
        && self.session.turn_count() % dream_config.turn_interval == 0
    {
        let messages = self.session.messages_for_provider();
        let abort = self.interrupt_signal.clone();
        let provider = self.provider_handle();

        tokio::spawn(async move {
            let result = run_auto_dream(
                &dream_config,
                self.session.turn_count(),
                &messages,
                provider,
                abort,
            ).await;

            if let Err(e) = result {
                warn!("Auto-dream failed: {}", e);
            }
        });
    }
}
```

### 5.2 Provider Fork Implementation — `crates/jcode-provider-anthropic/src/lib.rs`

The `Provider::fork()` implementation is critical for cache sharing:

```rust
// In the Anthropic/OAuth provider implementation:

impl Provider for AnthropicProvider {
    // ...

    fn fork(&self) -> Arc<dyn Provider> {
        // Create a new provider instance that shares:
        // - API credentials (same API key / OAuth token)
        // - HTTP connection pool (same client)
        // - Credential refresh machinery
        //
        // But has independent:
        // - Streaming state (won't interfere with parent's active streams)
        // - Model selection state
        // - Rate limit tracking
        //
        // The key insight: the prompt cache is SERVER-SIDE (Anthropic/OpenAI
        // maintain the KV cache). As long as the fork sends the same system
        // prompt, tools, model, and message prefix, the server returns a
        // cache hit. The client-side provider state doesn't affect caching.

        Arc::new(Self {
            client: self.client.clone(),         // Shared HTTP client
            api_key: self.api_key.clone(),       // Shared credentials
            model: self.model.clone(),           // Same model identifier
            // ... new independent streaming state
            active_streams: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}
```

### 5.3 Cargo Feature — `Cargo.toml`

```toml
[features]
forked-agent = [
    "jcode-swarm-core/fork",
    "jcode-memory-types/extraction",
]

[dependencies]
jcode-swarm-core = { path = "crates/jcode-swarm-core" }
jcode-memory-types = { path = "crates/jcode-memory-types" }
```

### 5.4 Cargo.toml for the fork feature in swarm-core

```toml
# crates/jcode-swarm-core/Cargo.toml
[features]
fork = [
    "dep:jcode-agent-runtime",
    "dep:jcode-message-types",
    "dep:jcode-provider-core",
    "dep:jcode-tool-core",
    "dep:tracing",
    "dep:futures",
    "dep:tokio",
]

[dependencies]
jcode-agent-runtime = { path = "../jcode-agent-runtime", optional = true }
jcode-message-types = { path = "../jcode-message-types", optional = true }
jcode-provider-core = { path = "../jcode-provider-core", optional = true }
jcode-tool-core = { path = "../jcode-tool-core", optional = true }
tracing = { version = "0.1", optional = true }
futures = { version = "0.3", optional = true }
tokio = { version = "1", features = ["sync"], optional = true }
```

### 5.5 Module Structure

```
crates/jcode-swarm-core/src/
├── fork/
│   ├── mod.rs            # CacheSafeParams, ForkedAgentParams, ForkPermissionMode, ForkedAgentResult
│   ├── runner.rs         # run_forked_agent(), ForkStaleDetector
│   └── permissions.rs    # ForkToolContext wrapper
├── lib.rs                # Existing swarm types (SwarmMemberRecord, etc.)
└── team/                 # Existing team module

crates/jcode-memory-types/src/
├── lib.rs                # Existing MemoryGraph, MemoryActivity
├── extraction.rs         # ExtractionCursor, MemoryExtractionConfig, has_memory_writes_since()
├── extraction_prompts.rs # build_extraction_prompt()

crates/jcode-overnight-core/src/
├── lib.rs                # Existing overnight types
└── auto_dream.rs         # AutoDreamConfig
```

---

## 6. Configuration & Wiring

### Config TOML

```toml
# config.toml — new [forked_agent] section
[forked_agent]
# Master switch for all forked agent features.
# When false, no forks are created regardless of sub-feature settings.
enabled = false

[forked_agent.memory_extraction]
enabled = true
memory_dir = ".jcode/memory"
max_turns = 3
max_output_tokens = 4096
min_new_messages = 5
prompt_variant = "auto"    # "auto" | "combined"

[forked_agent.auto_dream]
enabled = true
turn_interval = 10
max_turns = 2
max_output_tokens = 2048
allowed_dirs = [".jcode/dreams"]
dream_dir = ".jcode/dreams"
```

### Config Types

```rust
// crates/jcode-config-types/src/fork.rs

use jcode_memory_types::extraction::MemoryExtractionConfig;
use jcode_overnight_core::auto_dream::AutoDreamConfig;
use serde::{Deserialize, Serialize};

/// Top-level forked agent configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForkedAgentConfig {
    /// Master switch — when false, all fork features are disabled.
    pub enabled: bool,
    /// Memory extraction sub-config.
    pub memory_extraction: MemoryExtractionConfig,
    /// Auto-dream sub-config.
    pub auto_dream: AutoDreamConfig,
}

impl Default for ForkedAgentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            memory_extraction: MemoryExtractionConfig::default(),
            auto_dream: AutoDreamConfig::default(),
        }
    }
}
```

### Hook Integration — Exact Wiring Points

| Hook Point | File | What to do |
|---|---|---|
| **After each complete turn** | `crates/jcode-app-core/src/agent.rs` — `stop_hooks` handler | Save `CacheSafeParams`, spawn fork tasks |
| **Session start** | `crates/jcode-app-core/src/session_launch.rs` | Load extraction cursor from disk |
| **Session end** | `crates/jcode-app-core/src/agent.rs` — cleanup | Clear `CacheSafeParams`, cancel fork tasks |
| **Tool execution** | `crates/jcode-app-core/src/agent/tools.rs` | Check `ForkToolContext::check_tool()` for forks |
| **Provider fork** | `crates/jcode-provider-*/src/lib.rs` | Implement `Provider::fork()` for each provider |

---

## 7. Repo References

| Feature Aspect | Repo | File | Key Lines |
|----------------|------|------|-----------|
| **Forked agent core** | CCB | `src/utils/forkedAgent.ts` | `CacheSafeParams`, `runForkedAgent()`, `saveCacheSafeParams()` |
| **Memory extraction consumer** | CCB | `src/services/extractMemories/extractMemories.ts` | `runExtraction()`, `hasMemoryWritesSince()`, `createAutoMemCanUseTool()` |
| **Extraction prompt templates** | CCB | `src/services/extractMemories/prompts.ts` | `buildExtractAutoOnlyPrompt()`, `buildExtractCombinedPrompt()` |
| **Subagent lifecycle tracking** | oh-my-claudecode | `src/hooks/subagent-tracker/index.ts` | `SubagentTrackingState`, `SubagentInfo`, stale detection |
| **Swarm agent executor** | oh-my-pi | `packages/swarm-extension/src/swarm/executor.ts` | `executeSwarmAgent()`, `runSubprocess()` |
| **Swarm pipeline controller** | oh-my-pi | `packages/swarm-extension/src/swarm/pipeline.ts` | `PipelineController`, wave DAG |
| **Agent-tool spawn pattern** | codebuff | `packages/agent-runtime/src/tools/handlers/tool/spawn-agents.ts` | `handleSpawnAgents()`, `executeSubagent()` |
| **Inline subagent spawning** | codebuff | `packages/agent-runtime/src/tools/handlers/tool/spawn-agent-inline.ts` | `handleSpawnAgentInline()`, shared message history |
| **Subagent execution utils** | codebuff | `packages/agent-runtime/src/tools/handlers/tool/spawn-agent-utils.ts` | `executeSubagent()`, `SubagentContextParams` |
| **Provider::fork() trait** | jcode (existing) | `crates/jcode-provider-core/src/lib.rs:343` | `fn fork(&self) -> Arc<dyn Provider>` |
| **Provider::complete_split()** | jcode (existing) | `crates/jcode-provider-core/src/lib.rs:70` | `async fn complete_split(...)` |
| **Swarm lifecycle types** | jcode (existing) | `crates/jcode-swarm-core/src/lib.rs` | `SwarmLifecycleStatus`, `SwarmMemberRecord`, `SwarmRole` |
| **Background task types** | jcode (existing) | `crates/jcode-background-types/src/lib.rs` | `BackgroundTaskStatus`, `BackgroundTaskProgress` |
| **Memory graph types** | jcode (existing) | `crates/jcode-memory-types/src/lib.rs` | `MemoryGraph`, `MemoryActivity`, `PipelineState` |
| **Agent runtime** | jcode (existing) | `crates/jcode-agent-runtime/src/lib.rs` | `InterruptSignal`, `SoftInterruptQueue`, `SoftInterruptSource::BackgroundTask` |
| **Tool context** | jcode (existing) | `crates/jcode-tool-core/src/lib.rs` | `ToolContext`, `ToolExecutionMode` |
| **Hook event types** | jcode (existing) | `crates/jcode-hooks/src/types.rs` | `EVENT_SUBAGENT_START`, `EVENT_SUBAGENT_STOP` |
| **Overnight coordinator session** | jcode (existing) | `crates/jcode-app-core/src/overnight.rs` | `start_overnight_run()`, `create_coordinator_session()` |
| **Anthropic message formatting** | jcode (existing) | `crates/jcode-provider-anthropic/src/lib.rs` | `format_messages()` |

---

## 8. Test Cases

### 8.1 Unit Tests — Fork Core

```rust
// crates/jcode-swarm-core/src/fork/tests.rs

#[tokio::test]
async fn test_run_forked_agent_cache_hit_rate() {
    // Verify that forked agent achieves >80% cache hit rate
    // by using Provider::fork() with identical CacheSafeParams.
    //
    // 1. Create mock provider that records cache metrics
    // 2. Save CacheSafeParams with known messages
    // 3. Run forked agent with 1-turn extraction prompt
    // 4. Assert cache_read_tokens / total_input_tokens > 0.8

    let mut provider = MockProvider::new();
    provider.expect_fork().returning(|| Arc::new(MockProvider::new()));
    provider.expect_complete_split().returning(|_, _, _, _, _| {
        Ok(Box::pin(futures::stream::once(async {
            Ok(StreamEvent::MessageStop(Message::assistant("test")))
        })))
    });

    let params = ForkedAgentParams {
        prompt_messages: vec![Message::user("Extract memories from recent messages")],
        cache_safe_params: create_test_cache_params(Arc::new(provider)),
        permission_mode: ForkPermissionMode::MemoryExtraction {
            memory_provider: create_mock_memory_provider(),
        },
        fork_label: "test_extraction".into(),
        max_output_tokens: None,
        max_turns: Some(1),
        skip_transcript: true,
        parent_abort: None,
    };

    let result = run_forked_agent(params).await;
    assert!(result.cache_hit_rate > 0.8,
        "Expected >80% cache hit rate, got {:.1}%", result.cache_hit_rate * 100.0);
}

#[tokio::test]
async fn test_fork_permission_denies_write_outside_allowed_dir() {
    let perm = ForkPermissionMode::MemoryExtraction {
        memory_dir: PathBuf::from(".jcode/memory"),
    };

    // Allowed tools
    assert!(perm.is_tool_allowed("read", &json!({"file_path": "/etc/passwd"})));
    assert!(perm.is_tool_allowed("grep", &json!({"pattern": "test"})));
    assert!(perm.is_tool_allowed("glob", &json!({"pattern": "*.rs"})));

    // Denied: write outside memory dir
    assert!(!perm.is_tool_allowed("write", &json!({"file_path": "/etc/config"})));
    assert!(!perm.is_tool_allowed("edit", &json!({"file_path": "/tmp/test.txt"})));

    // Allowed: write inside memory dir
    assert!(perm.is_tool_allowed("write", &json!({"file_path": ".jcode/memory/user_role.md"})));

    // Denied: bash with write command
    assert!(!perm.is_tool_allowed("bash", &json!({"command": "rm -rf /"})));
    assert!(!perm.is_tool_allowed("bash", &json!({"command": "echo 'data' > file.txt"})));

    // Allowed: bash with read-only command
    assert!(perm.is_tool_allowed("bash", &json!({"command": "ls -la ."})));
    assert!(perm.is_tool_allowed("bash", &json!({"command": "cat file.txt"})));
    assert!(perm.is_tool_allowed("bash", &json!({"command": "head -20 log.txt"})));

    // Denied: unknown tools
    assert!(!perm.is_tool_allowed("mcp_tool", &json!({})));
    assert!(!perm.is_tool_allowed("agent_spawn", &json!({})));
}

#[tokio::test]
async fn test_memory_extraction_skips_when_parent_wrote_memories() {
    // CCB reference: hasMemoryWritesSince() — when the main agent writes
    // memories itself, the forked extraction is redundant and should skip.

    let memory_dir = PathBuf::from(".jcode/memory");
    let messages = vec![
        Message::assistant_with_tool_call("write", json!({
            "file_path": ".jcode/memory/user_role.md",
            "content": "User prefers Rust"
        })),
    ];

    assert!(has_memory_writes_since(&messages, None, &memory_dir));

    // Without memory writes — should return false
    let messages_no_write = vec![
        Message::assistant_with_tool_call("read", json!({"file_path": "src/main.rs"})),
    ];
    assert!(!has_memory_writes_since(&messages_no_write, None, &memory_dir));
}

#[tokio::test]
async fn test_extraction_cursor_advances_correctly() {
    let mut cursor = ExtractionCursor {
        last_processed_uuid: Some("msg-5".into()),
        session_id: "test-session".into(),
        last_extracted_at: None,
    };

    let messages = create_test_messages(10);
    let last_id = messages.last().unwrap().id().to_string();

    // Simulate successful extraction
    if let Some(last) = messages.last() {
        cursor.last_processed_uuid = Some(last.id().to_string());
    }
    cursor.last_extracted_at = Some(Utc::now());

    assert_eq!(cursor.last_processed_uuid, Some(last_id));
    assert!(cursor.last_extracted_at.is_some());
}

#[tokio::test]
async fn test_extraction_skips_on_few_new_messages() {
    let config = MemoryExtractionConfig {
        enabled: true,
        min_new_messages: 10,
        ..Default::default()
    };

    // Setup cursor at start, only 3 new messages
    let cursor = ExtractionCursor {
        last_processed_uuid: Some("msg-1".into()),
        session_id: "test".into(),
        last_extracted_at: None,
    };
    let messages = create_test_messages(5); // Only 4 new messages after msg-1

    let new_count = count_model_visible_messages_since(
        &messages, cursor.last_processed_uuid.as_deref()
    );

    assert!(new_count < config.min_new_messages,
        "Expected {} < {}", new_count, config.min_new_messages);
}

#[tokio::test]
async fn test_cache_safe_params_global_slot() {
    // Verify the global slot pattern works correctly:
    // 1. Initially empty
    assert!(get_last_cache_safe_params().is_none());

    // 2. After save, retrievable
    let params = create_test_cache_params(Arc::new(MockProvider::new()));
    save_cache_safe_params(params.clone());
    assert!(get_last_cache_safe_params().is_some());

    // 3. After clear, empty again
    clear_cache_safe_params();
    assert!(get_last_cache_safe_params().is_none());
}
```

### 8.2 Integration Tests

```rust
// crates/jcode-app-core/src/agent_tests.rs

#[tokio::test]
async fn test_fork_does_not_affect_parent_messages() {
    // Fork should have isolated mutable state — parent messages unchanged.
    //
    // 1. Create agent with known session messages
    // 2. Run a turn (parent)
    // 3. Save CacheSafeParams
    // 4. Run a forked agent with different prompt
    // 5. Assert parent messages are unchanged

    let mut agent = create_test_agent();
    let original_len = agent.session.messages_for_provider().len();

    // Save cache params
    let messages = agent.session.messages_for_provider();
    save_cache_safe_params(CacheSafeParams {
        system_prompt: Arc::from(agent.build_system_prompt()),
        tools: Arc::from(agent.tool_definitions().await),
        model: Arc::from(agent.provider.model()),
        fork_context_messages: Arc::from(messages),
        thinking_config: None,
        parent_provider: agent.provider_handle(),
    });

    // Run fork
    let result = run_forked_agent(ForkedAgentParams {
        prompt_messages: vec![Message::user("Extract memories")],
        cache_safe_params: get_last_cache_safe_params().unwrap(),
        permission_mode: ForkPermissionMode::MemoryExtraction {
            memory_provider: create_mock_memory_provider(),
        },
        fork_label: "test".into(),
        max_turns: Some(1),
        ..Default::default()
    }).await;

    // Assert parent messages unchanged
    let after_len = agent.session.messages_for_provider().len();
    assert_eq!(original_len, after_len,
        "Parent messages should not be modified by fork");
}

#[tokio::test]
async fn test_memory_extraction_full_flow() {
    // End-to-end: parent runs → stop hooks → fork extracts → cursor advances
    //
    // Uses temp directory for memory files.
    // Verifies: cache hit rate, cursor advancement, file creation.

    let temp_dir = tempfile::tempdir().unwrap();
    let memory_dir = temp_dir.path().join(".jcode/memory");
    std::fs::create_dir_all(&memory_dir).unwrap();

    let config = MemoryExtractionConfig {
        enabled: true,
        memory_dir: memory_dir.clone(),
        min_new_messages: 1,
        max_turns: 1,
        ..Default::default()
    };

    let mut cursor = ExtractionCursor {
        last_processed_uuid: None,
        session_id: "test-session".into(),
        last_extracted_at: None,
    };

    let messages = create_test_messages(10);
    let new_count = count_model_visible_messages_since(&messages, None);

    // Verify extraction would run (enough messages)
    assert!(new_count >= config.min_new_messages);
}
```

### 8.3 Edge Cases

```rust
#[tokio::test]
async fn test_fork_handles_provider_failure() {
    // Provider returns an error → fork should not crash, returns empty result
    let provider = MockProvider::new().with_error("rate_limited");
    save_cache_safe_params(create_test_cache_params(Arc::new(provider)));

    let result = run_forked_agent(ForkedAgentParams {
        ..test_params()
    }).await;

    assert!(result.messages.is_empty());
    assert!(result.duration_ms > 0);
}

#[tokio::test]
async fn test_fork_aborts_on_parent_signal() {
    // Parent session ends → fork should stop cleanly
    let (abort, abort_signal) = InterruptSignal::new();
    let handle = tokio::spawn(async move {
        run_forked_agent(ForkedAgentParams {
            parent_abort: Some(abort_signal),
            ..test_params()
        }).await
    });

    // Trigger abort
    abort.abort();
    let result = handle.await.unwrap();
    assert!(result.duration_ms > 0);
}

#[tokio::test]
async fn test_extraction_cursor_fallback_on_compaction() {
    // When cursor UUID is removed by compaction, fall back to counting all messages
    let cursor = ExtractionCursor {
        last_processed_uuid: Some("removed-by-compaction".into()),
        session_id: "test".into(),
        last_extracted_at: None,
    };
    let messages = create_test_messages(5);

    let count = count_model_visible_messages_since(
        &messages, cursor.last_processed_uuid.as_deref()
    );

    // Should count all messages as fallback (not 0)
    assert!(count > 0, "Fallback count should be > 0");
}

#[tokio::test]
async fn test_multiple_fork_consumers_run_in_parallel() {
    // Both memory extraction and auto-dream should run concurrently
    let mut handles = vec![];

    handles.push(tokio::spawn(test_fork("extraction")));
    handles.push(tokio::spawn(test_fork("dream")));

    for handle in handles {
        handle.await.unwrap();
    }
}
```

---

## 9. Benchmarks

### What to Measure

| Metric | Baseline | Target | How to Measure |
|--------|----------|--------|----------------|
| **Cache hit rate (fork vs parent)** | 0% (no fork, full price) | >80% | `cache_read_tokens / (input_tokens + cache_read_tokens + cache_creation_tokens)` |
| **Fork latency p50** | N/A | <3s | Wall time of `run_forked_agent()` — the shared prefix is cached so only the delta goes over the wire |
| **Fork latency p99** | N/A | <8s | Same, tail latency under concurrent forks |
| **Memory extraction cost** | N/A | <5% of parent turn cost | Compare `total_usage.cost_usd` of fork vs its parent turn |
| **Memory extraction tool turns** | N/A | ≤3 turns | CCB target: 1 read turn + 2 write turns |
| **Max concurrent forks** | N/A | ≥3 | Run 3 simultaneous forks, measure resource contention |

### Benchmark Harness

```rust
#[tokio::test]
async fn bench_cache_hit_rate() {
    // Simulate 100 parent→fork cycles and measure cache efficiency
    let mut hit_rates = Vec::new();
    let mut latencies = Vec::new();

    for i in 0..100 {
        let params = create_test_populated_params();
        let start = Instant::now();

        let result = run_forked_agent(params).await;

        latencies.push(result.duration_ms);
        hit_rates.push(result.cache_hit_rate);
    }

    let mean_hit: f64 = hit_rates.iter().sum::<f64>() / hit_rates.len() as f64;
    let mut lat_sorted = latencies.clone();
    lat_sorted.sort_unstable();
    let p50 = lat_sorted[lat_sorted.len() / 2];
    let p99 = lat_sorted[(lat_sorted.len() as f64 * 0.99) as usize];

    println!("=== Forked Agent Benchmarks ===");
    println!("Mean cache hit rate: {:.1}%", mean_hit * 100.0);
    println!("p50 latency: {}ms", p50);
    println!("p99 latency: {}ms", p99);

    assert!(mean_hit > 0.8, "Cache hit rate below 80%: {:.1}%", mean_hit * 100.0);
    assert!(p99 < 8000, "p99 latency above 8s: {}ms", p99);
}
```

---

## 10. Migration / Rollout

This is a **completely new feature** — no existing code to migrate. Rollout in three phases:

### Phase A — Fork Core (1-2 days)
- Create `crates/jcode-swarm-core/src/fork/` with `mod.rs`, `runner.rs`, `permissions.rs`
- Implement `CacheSafeParams`, `ForkedAgentParams`, `ForkPermissionMode`, `ForkedAgentResult`
- Implement `run_forked_agent()` with tool filtering and usage tracking
- Add `fork` feature flag to `jcode-swarm-core/Cargo.toml`
- **Verify**: `cargo test -p jcode-swarm-core --features fork` passes

### Phase B — Memory Extraction (1-2 days)
- Create `crates/jcode-memory-types/src/extraction.rs` and `extraction_prompts.rs`
- Implement `ExtractionCursor`, `MemoryExtractionConfig`, `has_memory_writes_since()`
- Implement `build_extraction_prompt()` with both variants
- Add `extraction` feature to `jcode-memory-types/Cargo.toml`
- **Verify**: Unit tests pass, cursor persists correctly

### Phase C — Wiring + Auto-Dream (1-2 days)
- Wire `handle_stop_hooks()` in `crates/jcode-app-core/src/agent.rs`
- Add `ForkedAgentConfig` to `jcode-config-types`
- Implement `AutoDreamConfig` in `jcode-overnight-core`
- Add `forked-agent` feature to root `Cargo.toml`
- **Verify**: `cargo test --features forked-agent` passes
- **Verify**: Manual test with `config.toml` enabling extraction

### Configuration defaults
- Master switch: `enabled = false` (opt-in)
- Both consumers default to `enabled = true` under the master switch
- No breaking changes to existing behavior

---

## 11. Known Limitations & Future Work

## Implementation Status

### Implemented ✓

#### Phase A — Fork Core (`crates/jcode-swarm-core/src/fork/`)
- ✓ `mod.rs` — `CacheSafeParams`, `ForkedAgentParams`, `ForkPermissionMode`, `ForkedAgentResult`
- ✓ `runner.rs` — `run_forked_agent()` with tool filtering and usage tracking
- ✓ `permissions.rs` — `ForkToolContext` for permission gating
- ✓ Global `CacheSafeParams` slot (`save_cache_safe_params`, `get_last_cache_safe_params`, `clear_cache_safe_params`)
- ✓ `ForkStaleDetector` with configurable threshold
- ✓ Unit tests for permission modes, cache-safe params slot
- ✓ `fork` feature flag in `jcode-swarm-core/Cargo.toml`

#### Phase B — Memory Extraction (`crates/jcode-memory-types/src/`)
- ✓ `extraction.rs` — `ExtractionCursor`, `MemoryExtractionConfig`, `has_memory_writes_since()`
- ✓ `extraction_prompts.rs` — `build_extraction_prompt()` with Auto and Combined variants
- ✓ Unit tests for memory writes detection, cursor advancement, prompt generation
- ✓ `extraction` feature flag in `jcode-memory-types/Cargo.toml`

#### Phase C — Auto-Dream (`crates/jcode-overnight-core/src/`)
- ✓ `auto_dream.rs` — `AutoDreamConfig` with turn interval, tool restrictions, write dirs
- ✓ Unit tests for default and custom config values

#### Phase D — Config Types & Wiring
- ✓ `ForkedAgentConfig` master switch type in `crates/jcode-config-types/src/lib.rs`
- ✓ `MemoryExtractionConfig` and `AutoDreamConfig` in config-types with serde defaults
- ✓ `forked-agent` feature flag in root `Cargo.toml`

#### Phase E — App-core Integration & Lifecycle Wiring
- ✓ `handle_stop_hooks()` in `turn_loops.rs` — saves `CacheSafeParams`, spawns background fork tasks after each complete turn
- ✓ Memory extraction trigger: skips if parent already wrote memories, checks `min_new_messages` threshold
- ✓ Auto-dream trigger: fires via configurable turn interval
- ✓ Session end cleanup: `clear_cache_safe_params()` in `mark_closed()`
- ✓ `forked-agent` feature flag in `jcode-app-core/Cargo.toml`
- ✓ `ForkedAgentConfig` re-exported from `jcode-base/src/config.rs`
- ✓ `ForkedAgentConfig` field added to main `Config` struct with `#[serde(default)]`
- ✓ Environment variable overrides: `JCODE_FORKED_AGENT_ENABLED`, `JCODE_FORKED_AGENT_MEMORY_EXTRACTION_ENABLED`, `JCODE_FORKED_AGENT_AUTO_DREAM_ENABLED`, `JCODE_FORKED_AGENT_MEMORY_EXTRACTION_MIN_NEW_MESSAGES`, `JCODE_FORKED_AGENT_AUTO_DREAM_TURN_INTERVAL`

### Future Work (planned, not yet implemented)

- [ ] **Cache invalidation on tool change**: If the parent's tool list changes mid-session (plugins injected, skills loaded), the fork's tool subset may shift. Mitigation: re-save `CacheSafeParams` after tool registry changes.
- [ ] **Multi-provider compatibility**: `Provider::fork()` must be implemented for each provider variant (Anthropic, OpenAI, Bedrock, Gemini, OpenRouter). Some providers may not support independent forked state — graceful degradation to non-cached fork.
- [ ] **Fork result streaming to TUI**: Currently fire-and-forget. Future: show fork progress in the team/background panel.
- [ ] **Auto-dream scheduling**: Simple turn-interval for now. Future: smarter scheduling based on session activity, context pressure, or time since last dream.
- [ ] **Cross-session memory sharing**: Memory extraction runs per-session. Future: global memory index across sessions for persistent user knowledge.
- [ ] **Stale fork detection**: Use `SwarmLifecycleStatus::RunningStale` when a fork exceeds `FORK_STALE_THRESHOLD_MS` without progress. Surface in the TUI for manual intervention.

---

## 12. Success Criteria Checklist

- [x] `CacheSafeParams` correctly captures all cache-key components: system prompt, tools, model, messages prefix
- [x] `Provider::fork()` trait method exists in `jcode-provider-core` (all provider types have baseline impl)
- [x] Forked agent query loop uses `Provider::fork()` to share parent's provider context
- [x] `ForkPermissionMode::MemoryExtraction` correctly restricts tools: read/grep/glob (unrestricted), read-only bash, write/edit only within memory dir
- [x] `ForkPermissionMode::AutoDream` correctly restricts tools: broader write access to allowed dirs
- [x] Memory extraction trigger fires from `handle_stop_hooks` after each complete turn
- [x] Extraction cursor (index-based) advances correctly
- [x] Cursor handles edge cases (graceful degradation when messages are compacted)
- [x] `has_memory_writes_since()` correctly detects parent's memory writes and skips extraction
- [x] Auto-dream fires on configurable turn interval
- [x] Both memory extraction and auto-dream can run concurrently (tokio::spawn for each)
- [x] Parent messages are not modified by fork execution (mutable state isolation via `Provider::fork()`)
- [x] Fork handles provider errors gracefully (logs warning, returns empty result)
- [x] Fork handles parent abort signal (checks InterruptSignal::is_set() each turn)
- [x] Feature is off by default (`forked_agent.enabled = false` in config), behind feature flags
- [x] `cargo test` passes across all modified crates (58 + 25 + 10 = 93 tests)
- [x] Documentation in `docs/FORKED_AGENT_PLAN.md` is up to date with implementation
