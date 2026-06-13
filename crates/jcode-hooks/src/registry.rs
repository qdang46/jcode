//! HookRegistry - manages hook registration and lookup by event type
//!
//! Provides efficient lookup of hooks filtered by event type and
//! matcher pattern against the current execution context.

use std::collections::HashMap;

use crate::config::{HookEvent, HookHandlerConfig, HooksConfig};
use crate::matcher::{matches, HookMatcher, MatcherContext};

/// Context passed to hooks for matching decisions.
///
/// Contains all information about the current execution context
/// that hooks can use to determine if they should run.
#[derive(Debug, Clone)]
pub struct HookContext {
    /// Session identifier
    pub session_id: String,
    /// Path to the session transcript file
    pub transcript_path: String,
    /// Current working directory
    pub cwd: String,
    /// Name of the hook event being triggered
    pub hook_event_name: String,
    /// Optional agent ID
    pub agent_id: Option<String>,
    /// Optional agent type
    pub agent_type: Option<String>,
    /// Optional tool name being executed
    pub tool_name: Option<String>,
    /// Optional tool input (serialized JSON)
    pub tool_input: Option<serde_json::Value>,
    /// Optional tool use ID
    pub tool_use_id: Option<String>,
    /// Optional permission mode
    pub permission_mode: Option<String>,
    /// Optional model name (e.g. "claude-sonnet-4-20250514")
    pub model: Option<String>,
    /// Optional user prompt text
    pub prompt: Option<String>,
    /// Optional system prompt text
    pub system_prompt: Option<String>,
    /// Optional current transcript/context size in bytes (used by PreCompact)
    pub current_size_bytes: Option<u64>,
    /// Optional task identifier (used by TaskCreated/TaskCompleted)
    pub task_id: Option<String>,
    /// Optional file path (used by FileChanged)
    pub file_path: Option<String>,
    /// Optional stop reason type (used by Stop)
    pub stop_type: Option<String>,
}

impl HookContext {
    /// Create a new empty HookContext
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

    /// Create a new HookContext for a tool-related event
    pub fn for_tool(tool_name: String, session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "PreToolUse".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: Some(tool_name),
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

    pub fn for_session_start(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "SessionStart".to_string(),
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

    pub fn for_session_end(session_id: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "SessionEnd".to_string(),
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

    pub fn for_permission_request(
        tool_name: String,
        session_id: String,
        permission_mode: String,
    ) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "PermissionRequest".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: Some(tool_name),
            tool_input: None,
            tool_use_id: None,
            permission_mode: Some(permission_mode),
            model: None,
            prompt: None,
            system_prompt: None,
            current_size_bytes: None,
            task_id: None,
            file_path: None,
            stop_type: None,
        }
    }

    pub fn for_permission_denied(session_id: String, permission_mode: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "PermissionDenied".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            permission_mode: Some(permission_mode),
            model: None,
            prompt: None,
            system_prompt: None,
            current_size_bytes: None,
            task_id: None,
            file_path: None,
            stop_type: None,
        }
    }

    /// Create a HookContext for a PermissionAsked event.
    ///
    /// Fired when a permission request is presented to the user. This is a
    /// blocking event — hooks can pre-approve (return "allow") to skip the
    /// user prompt entirely.
    pub fn for_permission_asked(
        action: String,
        session_id: String,
        permission_mode: String,
        request_id: String,
    ) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "PermissionAsked".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: Some(action),
            tool_input: None,
            tool_use_id: Some(request_id),
            permission_mode: Some(permission_mode),
            model: None,
            prompt: None,
            system_prompt: None,
            current_size_bytes: None,
            task_id: None,
            file_path: None,
            stop_type: None,
        }
    }

    /// Create a HookContext for a PermissionReplied event.
    ///
    /// Fired after a permission decision is recorded (approve or deny).
    /// This is an observational event — hooks cannot change the outcome.
    pub fn for_permission_replied(request_id: String, session_id: String, approved: bool) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "PermissionReplied".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: None,
            tool_input: Some(serde_json::json!({ "approved": approved })),
            tool_use_id: Some(request_id),
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

    pub fn for_tool_error(tool_name: String, session_id: String, error: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "ToolError".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: Some(tool_name),
            tool_input: Some(serde_json::json!({ "error": error })),
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

    /// Create a HookContext for a PreCompact event
    pub fn for_pre_compact(session_id: String, cwd: String, current_size_bytes: u64) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "PreCompact".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            permission_mode: None,
            model: None,
            prompt: None,
            system_prompt: None,
            current_size_bytes: Some(current_size_bytes),
            task_id: None,
            file_path: None,
            stop_type: None,
        }
    }

    /// Create a HookContext for a PostCompact event
    pub fn for_post_compact(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "PostCompact".to_string(),
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

    /// Create a HookContext for an AutoCompactionControl event
    pub fn for_auto_compaction_control(
        session_id: String,
        cwd: String,
        auto_compaction_enabled: bool,
        compaction_count: usize,
        avg_saved_bytes: u64,
    ) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "AutoCompactionControl".to_string(),
            agent_id: None,
            agent_type: None,
            tool_name: None,
            tool_input: Some(serde_json::json!({
                "auto_compaction_enabled": auto_compaction_enabled,
                "compaction_count": compaction_count,
                "avg_saved_bytes": avg_saved_bytes,
            })),
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

    /// Create a HookContext for a Stop event
    pub fn for_stop(session_id: String, cwd: String, stop_type: Option<String>) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "Stop".to_string(),
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
            stop_type,
        }
    }

    /// Create a HookContext for an AgentStart event
    pub fn for_agent_start(
        session_id: String,
        cwd: String,
        agent_id: Option<String>,
        agent_type: Option<String>,
    ) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "AgentStart".to_string(),
            agent_id,
            agent_type,
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

    /// Create a HookContext for an AgentEnd event
    pub fn for_agent_end(session_id: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "AgentEnd".to_string(),
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

    /// Create a HookContext for a SubagentStart event
    pub fn for_subagent_start(
        session_id: String,
        agent_id: Option<String>,
        agent_type: Option<String>,
    ) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "SubagentStart".to_string(),
            agent_id,
            agent_type,
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

    /// Create a HookContext for a SubagentStop event
    pub fn for_subagent_stop(
        session_id: String,
        agent_id: Option<String>,
        agent_type: Option<String>,
    ) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd: String::new(),
            hook_event_name: "SubagentStop".to_string(),
            agent_id,
            agent_type,
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

    /// Create a HookContext for a SessionUpdated event
    pub fn for_session_updated(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "SessionUpdated".to_string(),
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

    /// Create a HookContext for a SessionDiff event
    pub fn for_session_diff(session_id: String, cwd: String, file_path: Option<String>) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "SessionDiff".to_string(),
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
            file_path,
            stop_type: None,
        }
    }

    /// Create a HookContext for a SessionError event
    pub fn for_session_error(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "SessionError".to_string(),
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

    /// Create a HookContext for a SessionIdle event
    pub fn for_session_idle(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "SessionIdle".to_string(),
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

    /// Create a HookContext for a TurnEnd event
    ///
    /// Fired at the end of each agent turn with turn outcome metadata.
    pub fn for_turn_end(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            transcript_path: String::new(),
            cwd,
            hook_event_name: "TurnEnd".to_string(),
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

    /// Build a MatcherContext for use with the hook matcher
    ///
    /// Uses tool_name as the primary target for pattern matching.
    /// If additional context text is needed (e.g., full command for Bash),
    /// use `with_context()` instead.
    pub fn matcher_context(&self) -> MatcherContext<'_> {
        MatcherContext::new(self.tool_name.as_deref().unwrap_or(""))
    }

    /// Build a MatcherContext with additional context text
    pub fn matcher_context_with_context<'a>(&'a self, context: &'a str) -> MatcherContext<'a> {
        MatcherContext::with_context(self.tool_name.as_deref().unwrap_or(""), context)
    }
}

/// Registry of hooks organized by event type.
///
/// Provides lookup of hooks by event type and filtering by matcher pattern.
#[derive(Debug, Clone)]
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<HookHandlerConfig>>,
}

impl HookRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            hooks: HashMap::new(),
        }
    }

    /// Create a registry from a HooksConfig
    ///
    /// Converts the flat config entries into event-keyed vectors.
    pub fn from_config(config: HooksConfig) -> Self {
        let mut registry = Self::new();

        // HooksConfig.events maps event names to a Vec of handler configs
        for (event_name, handlers) in config.events.into_iter() {
            // Parse the event name to get the HookEvent enum value
            let event = if let Some(event) = HookEvent::parse(&event_name) {
                event
            } else {
                HookEvent::Custom(event_name)
            };
            registry.hooks.entry(event).or_default().extend(handlers);
        }

        registry
    }

    /// Get all hooks for a specific event type
    pub fn get_hooks(&self, event: &HookEvent) -> &[HookHandlerConfig] {
        self.hooks.get(event).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get hooks matching the given event and context criteria.
    ///
    /// Returns handlers whose matcher (if any) matches the tool_name
    /// in the provided context. All 4 matcher types are supported:
    /// - Exact: matches a single tool name exactly
    /// - Multi: matches any of several tool names  
    /// - Regex: matches tool name via regex pattern
    /// - Wildcard: matches any tool name
    pub fn get_matching(
        &self,
        event: &HookEvent,
        context: &HookContext,
    ) -> Vec<&HookHandlerConfig> {
        self.get_hooks(event)
            .iter()
            .filter(|handler| {
                // Skip handlers that have an `if_` condition that evaluates to false
                if let Some(condition) = self.get_handler_condition(handler) {
                    if !self.evaluate_condition(condition, context) {
                        return false;
                    }
                }

                // Get the matcher for this handler
                if let Some(matcher) = self.get_handler_matcher(handler) {
                    // Build matcher context - include command for regex matching
                    let ctx = context.matcher_context();
                    matches(matcher, &ctx)
                } else {
                    // No matcher means wildcard - always match
                    true
                }
            })
            .collect()
    }

    /// Get the matcher from a handler configuration
    ///
    fn get_handler_matcher<'a>(&self, handler: &'a HookHandlerConfig) -> Option<&'a HookMatcher> {
        match handler {
            HookHandlerConfig::Command(cmd) => cmd.matcher.as_ref(),
            HookHandlerConfig::Http(http) => http.matcher.as_ref(),
            HookHandlerConfig::Agent(agent) => agent.matcher.as_ref(),
            HookHandlerConfig::Plugin(plugin) => plugin.matcher.as_ref(),
        }
    }

    /// Get the condition (`if_`) from a handler configuration
    fn get_handler_condition<'a>(&self, handler: &'a HookHandlerConfig) -> Option<&'a str> {
        match handler {
            HookHandlerConfig::Command(cmd) => cmd.if_.as_deref(),
            HookHandlerConfig::Http(http) => http.if_.as_deref(),
            HookHandlerConfig::Agent(agent) => agent.if_.as_deref(),
            HookHandlerConfig::Plugin(plugin) => plugin.if_.as_deref(),
        }
    }

    /// Evaluate a condition against the context
    ///
    /// Conditions are shell-like expressions that can check context fields.
    fn evaluate_condition(&self, condition: &str, context: &HookContext) -> bool {
        // Simple condition evaluation
        // Format: "field=value" or "field!=value"
        if let Some((field, value)) = condition.split_once('=') {
            let field = field.trim();
            let value = value.trim();
            match field {
                "tool_name" => context.tool_name.as_deref() == Some(value),
                "agent_type" => context.agent_type.as_deref() == Some(value),
                "permission_mode" => context.permission_mode.as_deref() == Some(value),
                _ => true,
            }
        } else if let Some((field, value)) = condition.split_once("!=") {
            let field = field.trim();
            let value = value.trim();
            match field {
                "tool_name" => context.tool_name.as_deref() != Some(value),
                "agent_type" => context.agent_type.as_deref() != Some(value),
                "permission_mode" => context.permission_mode.as_deref() != Some(value),
                _ => true,
            }
        } else {
            // Unknown condition format - allow by default
            true
        }
    }

    /// Check if the registry is empty (no hooks registered)
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty() || self.hooks.values().all(Vec::is_empty)
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CommandHandlerConfig;

    #[test]
    fn test_new_registry_is_empty() {
        let registry = HookRegistry::new();
        assert!(registry.is_empty());
    }

    #[test]
    fn test_from_empty_config() {
        let config = HooksConfig::default();
        let registry = HookRegistry::from_config(config);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_get_hooks_returns_empty_for_unknown_event() {
        let registry = HookRegistry::new();
        let hooks = registry.get_hooks(&HookEvent::PreToolUse);
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_from_config_with_single_event() {
        let mut config = HooksConfig::default();
        config.events.insert(
            "pre_tool_use".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: "test_command".to_string(),
                ..Default::default()
            })],
        );

        let registry = HookRegistry::from_config(config);
        let hooks = registry.get_hooks(&HookEvent::PreToolUse);
        assert_eq!(hooks.len(), 1);
        assert!(
            matches!(&hooks[0], HookHandlerConfig::Command(cmd) if cmd.command == "test_command")
        );
    }

    #[test]
    fn test_from_config_with_custom_event() {
        let mut config = HooksConfig::default();
        config.events.insert(
            "custom:my_event".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: "custom_handler".to_string(),
                ..Default::default()
            })],
        );

        let registry = HookRegistry::from_config(config);
        let hooks = registry.get_hooks(&HookEvent::Custom("my_event".to_string()));
        assert_eq!(hooks.len(), 1);
    }

    #[test]
    fn test_hook_context_for_tool() {
        let context = HookContext::for_tool(
            "Bash".to_string(),
            "session-123".to_string(),
            "/project".to_string(),
        );

        assert_eq!(context.session_id, "session-123");
        assert_eq!(context.cwd, "/project");
        assert_eq!(context.hook_event_name, "PreToolUse");
        assert_eq!(context.tool_name, Some("Bash".to_string()));
    }

    #[test]
    fn test_hook_context_matcher_context() {
        let context = HookContext::for_tool(
            "Bash".to_string(),
            "session-123".to_string(),
            "/project".to_string(),
        );

        let ctx = context.matcher_context();
        assert_eq!(ctx.target, "Bash");
        assert!(ctx.context.is_none());
    }

    #[test]
    fn test_hook_context_matcher_context_with_context() {
        let context = HookContext::for_tool(
            "Bash".to_string(),
            "session-123".to_string(),
            "/project".to_string(),
        );

        let ctx = context.matcher_context_with_context("git commit -m 'test'");
        assert_eq!(ctx.target, "Bash");
        assert_eq!(ctx.context, Some("git commit -m 'test'"));
    }

    #[test]
    fn test_get_matching_returns_all_for_wildcard() {
        let mut config = HooksConfig::default();
        config.events.insert(
            "pre_tool_use".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: "test_command".to_string(),
                ..Default::default()
            })],
        );

        let registry = HookRegistry::from_config(config);
        let context = HookContext::for_tool(
            "Bash".to_string(),
            "session-123".to_string(),
            "/project".to_string(),
        );

        // Should return 1 handler (matches all since no matcher)
        let matching = registry.get_matching(&HookEvent::PreToolUse, &context);
        assert_eq!(matching.len(), 1);
    }

    #[test]
    fn test_get_matching_filters_by_event() {
        let mut config = HooksConfig::default();
        config.events.insert(
            "post_tool_use".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: "post_handler".to_string(),
                ..Default::default()
            })],
        );

        let registry = HookRegistry::from_config(config);
        let context = HookContext::for_tool(
            "Bash".to_string(),
            "session-123".to_string(),
            "/project".to_string(),
        );

        // Should return empty for pre_tool_use (only post_tool_use configured)
        let matching = registry.get_matching(&HookEvent::PreToolUse, &context);
        assert!(matching.is_empty());

        // Should return 1 for post_tool_use
        let matching = registry.get_matching(&HookEvent::PostToolUse, &context);
        assert_eq!(matching.len(), 1);
    }

    #[test]
    fn test_from_config_with_multiple_handlers_per_event() {
        let mut config = HooksConfig::default();
        config.events.insert(
            "pre_tool_use".to_string(),
            vec![
                HookHandlerConfig::Command(CommandHandlerConfig {
                    command: "first".to_string(),
                    ..Default::default()
                }),
                HookHandlerConfig::Command(CommandHandlerConfig {
                    command: "second".to_string(),
                    ..Default::default()
                }),
            ],
        );

        let registry = HookRegistry::from_config(config);
        let hooks = registry.get_hooks(&HookEvent::PreToolUse);
        assert_eq!(hooks.len(), 2);
    }

    #[test]
    fn test_new_context_fields_default_to_none() {
        let context = HookContext::new("s1", "/t", "/cwd", "Test");
        assert!(context.model.is_none());
        assert!(context.prompt.is_none());
        assert!(context.system_prompt.is_none());
        assert!(context.current_size_bytes.is_none());
        assert!(context.task_id.is_none());
        assert!(context.file_path.is_none());
        assert!(context.stop_type.is_none());
    }

    #[test]
    fn test_for_pre_compact_sets_size() {
        let context = HookContext::for_pre_compact("s1".to_string(), "/cwd".to_string(), 1024);
        assert_eq!(context.hook_event_name, "PreCompact");
        assert_eq!(context.current_size_bytes, Some(1024));
    }

    #[test]
    fn test_for_stop_sets_stop_type() {
        let context = HookContext::for_stop(
            "s1".to_string(),
            "/cwd".to_string(),
            Some("end_turn".to_string()),
        );
        assert_eq!(context.hook_event_name, "Stop");
        assert_eq!(context.stop_type, Some("end_turn".to_string()));
    }

    #[test]
    fn test_for_agent_start_sets_agent_fields() {
        let context = HookContext::for_agent_start(
            "s1".to_string(),
            "/cwd".to_string(),
            Some("agent-1".to_string()),
            Some("coder".to_string()),
        );
        assert_eq!(context.hook_event_name, "AgentStart");
        assert_eq!(context.agent_id, Some("agent-1".to_string()));
        assert_eq!(context.agent_type, Some("coder".to_string()));
    }

    #[test]
    fn test_agent_handler_matcher_and_condition() {
        use crate::config::AgentHandlerConfig;

        let mut config = HooksConfig::default();
        config.events.insert(
            "agent_start".to_string(),
            vec![HookHandlerConfig::Agent(AgentHandlerConfig {
                agent_id: "my_agent".to_string(),
                matcher: Some(HookMatcher::Exact("coder".to_string())),
                if_: Some("agent_type=coder".to_string()),
                ..Default::default()
            })],
        );

        let registry = HookRegistry::from_config(config);
        let context = HookContext::for_agent_start(
            "s1".to_string(),
            "/cwd".to_string(),
            None,
            Some("coder".to_string()),
        );

        // The matcher checks tool_name which is None, so it won't match "coder"
        // But the condition checks agent_type=coder which matches
        // Since matcher doesn't match (tool_name is None != "coder"), result is empty
        let matching = registry.get_matching(&HookEvent::AgentStart, &context);
        assert!(matching.is_empty());
    }

    #[test]
    fn test_plugin_handler_in_registry() {
        use crate::config::PluginHandlerConfig;

        let mut config = HooksConfig::default();
        config.events.insert(
            "pre_tool_use".to_string(),
            vec![HookHandlerConfig::Plugin(PluginHandlerConfig {
                path: "/usr/bin/plugin".to_string(),
                ..Default::default()
            })],
        );

        let registry = HookRegistry::from_config(config);
        let hooks = registry.get_hooks(&HookEvent::PreToolUse);
        assert_eq!(hooks.len(), 1);
        assert!(matches!(&hooks[0], HookHandlerConfig::Plugin(p) if p.path == "/usr/bin/plugin"));
    }
}
