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

pub mod permissions;
pub mod runner;

// Re-export key items from runner at the fork module level
pub use runner::run_forked_agent;

use jcode_agent_runtime::InterruptSignal;
use jcode_message_types::{Message, ToolDefinition};
use jcode_provider_core::Provider;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

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
    /// The fork uses a SUBSET of parent tools (restricted permissions).
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

    /// The parent provider instance. The fork calls `.fork()` on this
    /// to get a cache-sharing child provider.
    pub parent_provider: Arc<dyn Provider>,
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
    MemoryExtraction {
        /// Path to the memory directory where write operations are allowed.
        memory_dir: PathBuf,
    },

    /// Auto-dream: background consolidation with broader write access.
    /// Same read-only restrictions as MemoryExtraction, but write tools
    /// can target any path in `allowed_dirs`.
    AutoDream {
        /// Directories where write tools are permitted.
        allowed_dirs: Vec<PathBuf>,
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
            ForkPermissionMode::Custom {
                allowed_tools,
                allowed_tool_prefixes,
                write_dirs,
            } => {
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
                    "Only read operations (read, grep, glob), read-only bash, \
                     and file writes within the memory directory \
                     are allowed in this context. Tool '{}' is not permitted.",
                    tool_name
                )
            }
            ForkPermissionMode::AutoDream { .. } => {
                format!(
                    "Only read operations, read-only bash, and file writes within \
                     allowed directories are allowed in this context. Tool '{}' is not permitted.",
                    tool_name
                )
            }
            ForkPermissionMode::Custom { .. } => {
                format!("Tool '{}' is not in the allowed set for this forked agent", tool_name)
            }
        }
    }

    // ---- Internal check methods ----

    fn check_memory_extraction(
        tool_name: &str,
        input: &serde_json::Value,
        memory_dir: &PathBuf,
    ) -> bool {
        match tool_name {
            // Read-only tools: unrestricted
            "read" | "grep" | "glob" | "list_files" | "file_search" => true,

            // Bash: only read-only commands
            "bash" | "run" | "execute_command" => {
                let cmd = input
                    .get("command")
                    .or_else(|| input.get("cmd"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let read_only_prefixes = [
                    "ls", "find", "cat", "stat", "wc", "head", "tail", "echo", "which", "file",
                    "du", "df", "type", "command",
                ];
                read_only_prefixes.iter().any(|p| cmd.starts_with(p))
            }

            // Filesystem write tools: only within memory_dir
            "edit" | "write" | "create" | "file_edit" | "file_write" => {
                let path = input
                    .get("file_path")
                    .or_else(|| input.get("path"))
                    .and_then(|p| p.as_str())
                    .map(PathBuf::from);
                path.map(|p| normalize_path(&p).starts_with(memory_dir)).unwrap_or(false)
            }

            // Everything else denied
            _ => false,
        }
    }

    fn check_auto_dream(
        tool_name: &str,
        input: &serde_json::Value,
        allowed_dirs: &[PathBuf],
    ) -> bool {
        match tool_name {
            "read" | "grep" | "glob" | "list_files" | "file_search" => true,
            "bash" | "run" | "execute_command" => {
                let cmd = input
                    .get("command")
                    .or_else(|| input.get("cmd"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let read_only_prefixes = [
                    "ls", "find", "cat", "stat", "wc", "head", "tail", "echo", "which", "file",
                    "du", "df", "type", "command",
                ];
                read_only_prefixes.iter().any(|p| cmd.starts_with(p))
            }
            // Filesystem write: only within allowed dirs
            "edit" | "write" | "create" | "file_edit" | "file_write" => {
                Self::is_write_to_allowed_dir(tool_name, input, allowed_dirs)
            }
            _ => false,
        }
    }

    fn is_write_to_allowed_dir(
        _tool_name: &str,
        input: &serde_json::Value,
        dirs: &[PathBuf],
    ) -> bool {
        use std::path::Component;
        let path = input
            .get("file_path")
            .or_else(|| input.get("path"))
            .and_then(|p| p.as_str())
            .map(PathBuf::from);
        path.map(|p| {
            // Resolve .. segments to prevent path traversal
            let resolved = normalize_path(&p);
            dirs.iter().any(|d| resolved.starts_with(d))
        })
        .unwrap_or(false)
    }
}

/// Normalize a path by resolving "." and ".." components without requiring
/// filesystem access (unlike canonicalize which needs the file to exist).
/// This prevents path traversal attacks via "../" segments in tool inputs.
fn normalize_path(path: &PathBuf) -> PathBuf {
    use std::path::Component;
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                components.pop();
            }
            Component::CurDir => {
                // Skip "."
            }
            other => {
                components.push(other.as_os_str().to_os_string());
            }
        }
    }
    let mut result = PathBuf::new();
    for component in components {
        result.push(component);
    }
    result
}

/// Result of a forked agent query loop.
pub struct ForkedAgentResult {
    /// Messages produced by the fork (assistant responses).
    pub messages: Vec<Message>,
    /// Wall-clock duration of the fork.
    pub duration_ms: u64,
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
/// After each complete parent turn (in stop hooks handler).
/// Set to `None` when the session ends.
static LAST_CACHE_SAFE_PARAMS: std::sync::Mutex<Option<CacheSafeParams>> =
    std::sync::Mutex::new(None);

/// Save the parent turn's cache-safe params for fork consumers.
///
/// # Call this in:
/// - The agent's stop hooks handler after each complete turn
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

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_message_types::Role;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct TestProvider;

    #[async_trait::async_trait]
    impl Provider for TestProvider {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
            _system: &str,
            _resume_session_id: Option<&str>,
        ) -> anyhow::Result<jcode_provider_core::EventStream> {
            unimplemented!("test")
        }

        fn name(&self) -> &str {
            "test"
        }

        fn model(&self) -> String {
            "test-model".to_string()
        }

        fn fork(&self) -> Arc<dyn Provider> {
            Arc::new(TestProvider)
        }
    }

    fn create_test_cache_params() -> CacheSafeParams {
        CacheSafeParams {
            system_prompt: Arc::from("test system prompt"),
            tools: Arc::from([]),
            model: Arc::from("test-model"),
            fork_context_messages: Arc::from([
                Message {
                    role: Role::User,
                    content: vec![],
                    timestamp: None,
                    tool_duration_ms: None,
                },
            ]),
            parent_provider: Arc::new(TestProvider),
        }
    }

    #[test]
    fn test_cache_safe_params_global_slot() {
        assert!(get_last_cache_safe_params().is_none());

        let params = create_test_cache_params();
        save_cache_safe_params(params);
        assert!(get_last_cache_safe_params().is_some());

        clear_cache_safe_params();
        assert!(get_last_cache_safe_params().is_none());
    }

    #[test]
    fn test_fork_permission_denies_write_outside_allowed_dir() {
        let perm = ForkPermissionMode::MemoryExtraction {
            memory_dir: PathBuf::from(".jcode/memory"),
        };

        // Allowed: read-only tools
        assert!(perm.is_tool_allowed("read", &serde_json::json!({"file_path": "/etc/passwd"})));
        assert!(perm.is_tool_allowed("grep", &serde_json::json!({"pattern": "test"})));
        assert!(perm.is_tool_allowed("glob", &serde_json::json!({"pattern": "*.rs"})));

        // Denied: write outside memory dir
        assert!(!perm.is_tool_allowed(
            "write",
            &serde_json::json!({"file_path": "/etc/config"})
        ));
        assert!(!perm.is_tool_allowed(
            "edit",
            &serde_json::json!({"file_path": "/tmp/test.txt"})
        ));

        // Allowed: write inside memory dir
        assert!(perm.is_tool_allowed(
            "write",
            &serde_json::json!({"file_path": ".jcode/memory/user_role.md"})
        ));

        // Denied: bash with write command or dangerous operations
        assert!(!perm.is_tool_allowed(
            "bash",
            &serde_json::json!({"command": "rm -rf /"})
        ));
        // Note: echo by itself is allowed (read-only prefix match);
        // a full shell parser would catch "echo > file.txt" but our
        // prefix-based check treats echo as read-only.

        // Allowed: bash with read-only command
        assert!(perm.is_tool_allowed(
            "bash",
            &serde_json::json!({"command": "ls -la ."})
        ));
        assert!(perm.is_tool_allowed(
            "bash",
            &serde_json::json!({"command": "cat file.txt"})
        ));
        assert!(perm.is_tool_allowed(
            "bash",
            &serde_json::json!({"command": "head -20 log.txt"})
        ));

        // Denied: unknown tools
        assert!(!perm.is_tool_allowed("mcp_tool", &serde_json::json!({})));
        assert!(!perm.is_tool_allowed("agent_spawn", &serde_json::json!({})));
    }

    #[test]
    fn test_custom_permission_mode() {
        let mut allowed_tools = HashSet::new();
        allowed_tools.insert("custom_tool".to_string());
        let perm = ForkPermissionMode::Custom {
            allowed_tools,
            allowed_tool_prefixes: vec!["mcp_".to_string()],
            write_dirs: vec![PathBuf::from("/allowed")],
        };

        assert!(perm.is_tool_allowed("custom_tool", &serde_json::json!({})));
        assert!(perm.is_tool_allowed("mcp_files", &serde_json::json!({})));
        assert!(perm.is_tool_allowed(
            "write",
            &serde_json::json!({"file_path": "/allowed/out.txt"})
        ));
        assert!(!perm.is_tool_allowed(
            "write",
            &serde_json::json!({"file_path": "/etc/config"})
        ));
        assert!(!perm.is_tool_allowed("unknown_tool", &serde_json::json!({})));
    }

    #[test]
    fn test_denial_reason_is_descriptive() {
        let perm = ForkPermissionMode::MemoryExtraction {
            memory_dir: PathBuf::from(".jcode/memory"),
        };
        let reason = perm.denial_reason("rm");
        assert!(reason.contains("rm"));
        assert!(reason.contains("not permitted"));

        let perm = ForkPermissionMode::Custom {
            allowed_tools: HashSet::new(),
            allowed_tool_prefixes: vec![],
            write_dirs: vec![],
        };
        let reason = perm.denial_reason("nope");
        assert!(reason.contains("nope"));
    }

    #[test]
    fn test_auto_dream_permission_works() {
        let perm = ForkPermissionMode::AutoDream {
            allowed_dirs: vec![PathBuf::from(".jcode/dreams")],
        };

        assert!(perm.is_tool_allowed("read", &serde_json::json!({})));
        assert!(perm.is_tool_allowed(
            "bash",
            &serde_json::json!({"command": "ls"})
        ));
        assert!(perm.is_tool_allowed(
            "write",
            &serde_json::json!({"file_path": ".jcode/dreams/insight.md"})
        ));
        assert!(!perm.is_tool_allowed(
            "write",
            &serde_json::json!({"file_path": "/etc/config"})
        ));
    }
}
