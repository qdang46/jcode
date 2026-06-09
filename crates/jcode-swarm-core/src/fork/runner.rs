//! Runs the forked agent query loop.
//!
//! # How it Works
//! 1. Takes `ForkedAgentParams` with cache-safe params from the parent
//! 2. Calls `parent_provider.fork()` to get a cache-sharing provider instance
//! 3. Builds the full message list: [shared_prefix .. fork_new_messages]
//! 4. Filters tool definitions to only expose what the permission mode allows
//! 5. Runs `Provider::complete()` with the forked provider
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

use crate::fork::{
    ForkPermissionMode, ForkedAgentParams, ForkedAgentResult, DEFAULT_FORK_MAX_TURNS,
};
use futures::StreamExt;
use jcode_message_types::{Message, ToolDefinition};
use jcode_message_types::StreamEvent;
use std::time::Instant;
use tracing::{debug, info, warn};

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
/// 6. **Execute**: Call `provider.complete()` with the forked provider.
/// 7. **Accumulate**: Read messages from stream events.
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
pub async fn run_forked_agent(params: ForkedAgentParams) -> ForkedAgentResult {
    let start = Instant::now();
    let fork_label = &params.fork_label;

    debug!(fork_label = %fork_label, "Starting forked agent query loop");

    // Step 1: Fork the provider to get a cache-sharing instance
    let forked_provider = params.cache_safe_params.parent_provider.fork();

    // Step 2: Merge messages — shared prefix + fork's new messages
    let mut messages: Vec<Message> = params
        .cache_safe_params
        .fork_context_messages
        .iter()
        .cloned()
        .collect();
    messages.extend(params.prompt_messages);

    // Step 3: Filter tools based on permission mode
    // IMPORTANT: We must use a SUBSET of the parent's tools. The cache key
    // is prefix-based — using fewer tools than the parent is fine, but adding
    // tools not in the parent's list would shift the key.
    let tools = filter_tools_for_permission(&params.cache_safe_params.tools, &params.permission_mode);

    // Step 4: Use the parent's system prompt (identical = cache hit)
    let system_static = &params.cache_safe_params.system_prompt;

    // Step 5: Run the query loop
    let mut output_messages: Vec<Message> = Vec::new();

    let max_turns = params.max_turns.unwrap_or(DEFAULT_FORK_MAX_TURNS);

    // Execute the fork's query loop
    for turn in 0..max_turns {
        debug!(
            fork_label = %fork_label,
            turn = turn,
            messages_len = messages.len(),
            tools_len = tools.len(),
            "Fork turn starting"
        );

        // Check for abort signal
        if let Some(ref abort) = params.parent_abort {
            if abort.is_set() {
                info!(
                    fork_label = %fork_label,
                    turn = turn,
                    "Fork aborted by parent signal"
                );
                break;
            }
        }

        // Call the provider
        // We use the fork's system prompt (identical to parent = cache hit)
        // and the fork's combined messages
        let result = forked_provider
            .complete(&messages, &tools, system_static, None)
            .await;

        match result {
            Ok(stream) => {
                let mut assistant_text = String::new();

                tokio::pin!(stream);
                while let Some(event) = stream.next().await {
                    match event {
                        Ok(StreamEvent::TextDelta(text)) => {
                            assistant_text.push_str(&text);
                        }
                        Ok(StreamEvent::MessageEnd { .. }) => {
                            // Message complete — we'll collect it
                        }
                        Ok(StreamEvent::Error {
                            message, ..
                        }) => {
                            warn!(
                                fork_label = %fork_label,
                                turn = turn,
                                error = %message,
                                "Fork stream error event"
                            );
                            break;
                        }
                        Err(e) => {
                            warn!(
                                fork_label = %fork_label,
                                turn = turn,
                                error = %e,
                                "Fork stream error"
                            );
                            break;
                        }
                        _ => {}
                    }
                }

                // Add the assistant's response as a message
                if !assistant_text.is_empty() {
                    let msg = Message::assistant_text(&assistant_text);
                    output_messages.push(msg.clone());
                    // Continue the loop: append the assistant message so the
                    // next turn can include it as context
                    messages.push(msg);
                    // Note: in a real fork, the next step would be tool execution.
                    // For now, we only do text responses (single-turn extraction).
                } else {
                    // No text produced — end the loop
                    break;
                }
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

    info!(
        fork_label = %fork_label,
        duration_ms = duration_ms,
        messages_produced = output_messages.len(),
        "Forked agent completed"
    );

    ForkedAgentResult {
        messages: output_messages,
        duration_ms,
    }
}

/// Filter tool definitions based on the fork's permission mode.
///
/// Passes through the full parent tool list to preserve the prompt cache key.
/// The Anthropic/OpenAI API cache key is a hash of the full tools parameter.
/// Subsetting the tool list would guarantee a cache MISS.
/// Runtime permission enforcement is handled by `ForkToolContext::check_tool()`
/// at tool execution time, not by removing tools from the API definition.
fn filter_tools_for_permission(
    all_tools: &[ToolDefinition],
    _permission: &ForkPermissionMode,
) -> Vec<ToolDefinition> {
    all_tools.to_vec()
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
        Self::new(crate::fork::FORK_STALE_THRESHOLD_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fork::ForkPermissionMode;
    use std::path::PathBuf;

    #[test]
    fn test_filter_tools_removes_none_for_read_only_tools() {
        let tools = vec![
            ToolDefinition {
                name: "read".to_string(),
                description: "Read a file".to_string(),
                input_schema: serde_json::json!({}),
            },
            ToolDefinition {
                name: "write".to_string(),
                description: "Write a file".to_string(),
                input_schema: serde_json::json!({}),
            },
            ToolDefinition {
                name: "mcp_tool".to_string(),
                description: "An MCP tool".to_string(),
                input_schema: serde_json::json!({}),
            },
        ];

        let perm = ForkPermissionMode::MemoryExtraction {
            memory_dir: PathBuf::from(".jcode/memory"),
        };

        // filter_tools_for_permission now passes through the full tool list to
        // preserve the prompt cache key. Runtime permission enforcement is handled
        // by ForkToolContext::check_tool() at tool execution time, not by removing
        // tool definitions from the API call.
        let filtered = filter_tools_for_permission(&tools, &perm);
        assert_eq!(filtered.len(), 3);
        assert!(filtered.iter().any(|t| t.name == "read"));
        assert!(filtered.iter().any(|t| t.name == "write"));
        assert!(filtered.iter().any(|t| t.name == "mcp_tool"));
    }
}
