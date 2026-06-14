//! Declarative agent definitions.
//!
//! An `AgentDefinition` is the schema that describes a sub-agent: its model
//! preferences, the tools it's allowed to call, the agents it can spawn,
//! the prompts it ships, and how its output flows back to its parent.
//!
//! Definitions are loaded from TOML files in three locations (highest
//! priority first):
//!
//!   1. `.jcode/agents/<id>.toml` (project-local, committed to repo)
//!   2. `~/.jcode/agents/<id>.toml` (user-global)
//!   3. Embedded built-in agents bundled with the binary
//!
//! ## Design constraints
//!
//! - Definitions are **declarative TOML**, not Rust code, so users can
//!   add agents without recompiling the binary.
//! - `model` is **not required**: agents inherit the session's current
//!   model unless they explicitly opt into tier slots or override.
//! - `tool_names` is a whitelist — agents start with NO tools by
//!   default and must list every tool they need. This is a security
//!   property: a poorly-defined agent can't escalate by accident.
//! - `spawnable_agents` is also a whitelist for the same reason.
//!
//! ## Adapted from Codebuff
//!
//! Field names track Codebuff's `AgentDefinition` (snake_case Rust →
//! camelCase TS) so prior art is reusable. Differences:
//!
//! - No `model` field as required string — replaced by tier + override.
//! - No `providerOptions` — jcode's session has a single provider.
//! - `handle_steps` is a future addition (programmatic agents arrive in
//!   Phase 2); for now agents are pure prompted.

use crate::output::OutputMode;
use crate::permission::PermissionMode;
use crate::reasoning::ReasoningEffort;
use crate::tier::ModelTier;

use serde::{Deserialize, Serialize};

/// Default version assigned when a definition omits `version`.
pub const DEFAULT_AGENT_VERSION: &str = "0.1.0";

/// Declarative description of one agent.
///
/// Intentionally `Clone` so the runtime can hand each spawn its own copy
/// without locking the registry. Definitions are small (a few KB at most).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    // -----------------------------------------------------------------
    // Identity
    // -----------------------------------------------------------------
    /// Unique agent id. Lowercase letters, digits, hyphens. e.g. `file-picker`.
    /// Must be unique within the registry — duplicate ids are a load error.
    pub id: String,

    /// Human-readable name shown in TUI / logs. e.g. `"Fletcher the File Fetcher"`.
    pub display_name: String,

    /// Publisher / namespace id when this agent is shared across projects.
    /// Optional for local agents; required if the agent is published to a
    /// future agent registry.
    #[serde(default)]
    pub publisher: Option<String>,

    /// Semver-ish version. Defaults to `DEFAULT_AGENT_VERSION`.
    #[serde(default = "default_version")]
    pub version: String,

    // -----------------------------------------------------------------
    // Model selection
    // -----------------------------------------------------------------
    /// Optional tier slot to prefer when running this agent. The slot is
    /// resolved against `JCODE_ROUTING_<TIER>` env vars at run time.
    /// Falls back to the session's current model if unset.
    ///
    /// See `tier.rs` for the full resolution algorithm.
    #[serde(default)]
    pub prefer_tier: Option<ModelTier>,

    /// Optional explicit model id override. Highest priority — beats
    /// `prefer_tier` and the session default. Use sparingly; hardcoding
    /// model ids makes the agent file non-portable across providers.
    #[serde(default)]
    pub model_override: Option<String>,

    /// Optional reasoning effort to forward to the provider request.
    /// Defaults are model-specific; runtime fills in a sensible default
    /// when this field is `None`.
    #[serde(default)]
    pub reasoning: Option<ReasoningEffort>,

    // -----------------------------------------------------------------
    // Tools and sub-agents
    // -----------------------------------------------------------------
    /// Allowlist of tool names this agent may call. Empty list = no tools.
    /// Whitelist semantics are deliberate — agents shouldn't have access
    /// to tools they don't need.
    #[serde(default)]
    pub tool_names: Vec<String>,

    /// Optional denylist of tool names this agent may NOT call, even if
    /// they appear in `tool_names`. Takes precedence over `tool_names`.
    /// Useful for inheriting a broad whitelist while blocking specific
    /// dangerous tools (e.g. allow all except `bash`).
    ///
    /// Empty list = no additional denials (default).
    #[serde(default)]
    pub disallowed_tools: Vec<String>,

    /// Allowlist of agent ids this agent may `spawn_agents` / `spawn_agent_inline`.
    /// Empty list = no spawning. Use the local agent id (e.g. `file-picker`)
    /// or the future `publisher/agent@version` form for shared agents.
    #[serde(default)]
    pub spawnable_agents: Vec<String>,

    // -----------------------------------------------------------------
    // Prompts
    // -----------------------------------------------------------------
    /// System prompt for this agent. Background, persona, mandates.
    /// Mutually exclusive with `inherit_parent_system_prompt = true`
    /// (which means "use the parent's system prompt instead, for cache
    /// prefix sharing").
    #[serde(default)]
    pub system_prompt: String,

    /// Instructions inserted after each user message. The most common
    /// place to shape agent behavior — terser than `system_prompt`,
    /// changes per turn allowed.
    #[serde(default)]
    pub instructions_prompt: Option<String>,

    /// Optional reminder inserted at every agent step. Use sparingly —
    /// strong models follow `instructions_prompt` reliably; this is for
    /// weaker models or agents that need a per-step nudge.
    #[serde(default)]
    pub step_prompt: Option<String>,

    /// Spawner-side prompt: when and why a parent agent should spawn this
    /// agent. Used in `spawn_agents` tool documentation so the parent's
    /// LLM picks the right sub-agent.
    #[serde(default)]
    pub spawner_prompt: Option<String>,

    // -----------------------------------------------------------------
    // Context / cache behavior
    // -----------------------------------------------------------------
    /// When true, child agent uses the parent's `system_prompt` instead
    /// of its own. This is the **prompt cache prefix-sharing trick** —
    /// editor / reviewer agents typically set this to `true` so the
    /// expensive system prompt is cache-hit rather than re-sent.
    ///
    /// Mutually exclusive with a non-empty `system_prompt`.
    #[serde(default)]
    pub inherit_parent_system_prompt: bool,

    /// When true, child agent receives the parent's full message history.
    /// Default false — most sub-agents work better with a clean slate
    /// (file-picker doesn't need to see edit chatter).
    #[serde(default)]
    pub include_message_history: bool,

    // -----------------------------------------------------------------
    // Permissions
    // -----------------------------------------------------------------
    /// Optional permission mode override for this agent's tool execution.
    /// When set, the agent runs under this permission mode instead of the
    /// session-global mode (set via CLI `--permission-mode` or cycled in
    /// the TUI).
    ///
    /// Useful for:
    /// - Restricting sub-agents: reviewer runs in `Plan` (read-only).
    /// - Elevating leaf agents: `basher` runs in `AcceptEdits`.
    /// - Background agents: CI runner uses `DontAsk`.
    ///
    /// If `None`, the agent inherits the session's current permission mode.
    /// See `permission.rs` for the full mode descriptions.
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,

    /// Optional maximum number of agentic turns this agent may execute
    /// before being stopped. Prevents runaway agents from consuming
    /// unbounded tokens/time.
    ///
    /// If `None`, the agent has no per-agent turn limit (the session
    /// global limit still applies).
    #[serde(default)]
    pub max_turns: Option<u32>,

    // -----------------------------------------------------------------
    // Output
    // -----------------------------------------------------------------
    /// How the agent's output is delivered to the parent. Default
    /// `LastMessage`.
    #[serde(default)]
    pub output_mode: OutputMode,

    /// JSON schema for `StructuredOutput` mode. Validated when the agent
    /// calls `set_output`. Stored as raw JSON value because we don't
    /// pull a JSON-schema crate yet — Phase 3 will add proper validation.
    #[serde(default)]
    pub output_schema: Option<serde_json::Value>,

    /// Optional display color for the agent in the TUI.
    /// Used to color the agent badge/indicator (red, blue, green, yellow, purple, orange, pink, cyan).
    #[serde(default)]
    pub color: Option<String>,
}

fn default_version() -> String {
    DEFAULT_AGENT_VERSION.to_string()
}

/// Validation errors produced when an agent definition violates its
/// invariants. Displayed to users when a TOML file fails to load.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum DefinitionError {
    #[error("agent id `{0}` is invalid: must be non-empty, lowercase ASCII alphanumeric or hyphen")]
    InvalidId(String),

    #[error(
        "agent `{id}` has both `inherit_parent_system_prompt = true` and a non-empty `system_prompt`. Set one or the other."
    )]
    SystemPromptConflict { id: String },

    #[error("agent `{id}` has `output_mode = structured_output` but `output_schema` is missing")]
    StructuredOutputMissingSchema { id: String },

    #[error("agent `{id}` references itself in `spawnable_agents`")]
    SelfSpawn { id: String },

    #[error("agent `{id}` lists tool `{tool}` more than once in `tool_names`")]
    DuplicateTool { id: String, tool: String },

    #[error("agent `{id}` lists agent `{spawn}` more than once in `spawnable_agents`")]
    DuplicateSpawnable { id: String, spawn: String },
}

/// Errors returned when cross-referencing an agent against the runtime
/// tool/agent universe (i.e. checking that `tool_names` actually exist).
///
/// These are **separate from `DefinitionError`** because the runtime
/// universe isn't known at TOML-load time — it depends on feature flags,
/// MCP server connections, and the resolved agent registry. Callers
/// invoke `validate_tool_references` / `validate_spawn_references` at
/// agent spawn time.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ReferenceError {
    #[error("agent `{id}` references unknown tool(s): {unknown}. Available tools: {available}")]
    UnknownTools {
        id: String,
        unknown: String,
        available: String,
    },

    #[error(
        "agent `{id}` references unknown sub-agent(s): {unknown}. Available agents: {available}"
    )]
    UnknownSpawnableAgents {
        id: String,
        unknown: String,
        available: String,
    },
}

impl AgentDefinition {
    /// Validate id format + cross-field invariants. Returns `Ok(())` when
    /// the definition is well-formed.
    pub fn validate(&self) -> Result<(), DefinitionError> {
        // 1. id format
        if !is_valid_id(&self.id) {
            return Err(DefinitionError::InvalidId(self.id.clone()));
        }

        // 2. system_prompt vs inherit_parent_system_prompt mutual exclusion
        if self.inherit_parent_system_prompt && !self.system_prompt.is_empty() {
            return Err(DefinitionError::SystemPromptConflict {
                id: self.id.clone(),
            });
        }

        // 3. structured_output requires schema
        if matches!(self.output_mode, OutputMode::StructuredOutput) && self.output_schema.is_none()
        {
            return Err(DefinitionError::StructuredOutputMissingSchema {
                id: self.id.clone(),
            });
        }

        // 4. cannot spawn self
        if self.spawnable_agents.iter().any(|s| s == &self.id) {
            return Err(DefinitionError::SelfSpawn {
                id: self.id.clone(),
            });
        }

        // 5. no duplicate tool names
        let mut seen_tools = std::collections::HashSet::new();
        for tool in &self.tool_names {
            if !seen_tools.insert(tool.clone()) {
                return Err(DefinitionError::DuplicateTool {
                    id: self.id.clone(),
                    tool: tool.clone(),
                });
            }
        }

        // 6. no duplicate spawnable agent ids
        let mut seen_spawn = std::collections::HashSet::new();
        for spawn in &self.spawnable_agents {
            if !seen_spawn.insert(spawn.clone()) {
                return Err(DefinitionError::DuplicateSpawnable {
                    id: self.id.clone(),
                    spawn: spawn.clone(),
                });
            }
        }

        Ok(())
    }

    /// Resolve the concrete model id to use for one invocation of this agent.
    /// Convenience wrapper around `tier::resolve_model`.
    pub fn resolve_model(&self, current_session_model: &str) -> String {
        crate::tier::resolve_model(
            self.model_override.as_deref(),
            self.prefer_tier,
            current_session_model,
        )
    }

    /// Check that every entry in `tool_names` exists in the caller-provided
    /// universe of tool names. Returns the list of unknown tools when any
    /// fail. Caller decides whether unknown tools are fatal (likely yes
    /// for production agents, no for under-development agents).
    ///
    /// Empty `tool_names` always validates — agents with no tools are
    /// legal (e.g. pure-prompt summarizer).
    pub fn validate_tool_references<I, S>(&self, available: I) -> Result<(), ReferenceError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let available: std::collections::HashSet<String> = available
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let unknown: Vec<&String> = self
            .tool_names
            .iter()
            .filter(|name| !available.contains(name.as_str()))
            .collect();
        if unknown.is_empty() {
            return Ok(());
        }
        let mut sorted_unknown: Vec<&String> = unknown;
        sorted_unknown.sort();
        let mut sorted_available: Vec<&String> = available.iter().collect();
        sorted_available.sort();
        Err(ReferenceError::UnknownTools {
            id: self.id.clone(),
            unknown: sorted_unknown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            available: sorted_available
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        })
    }

    /// Check that every entry in `spawnable_agents` exists in the caller-
    /// provided universe of agent ids. Returns unknown agents when any
    /// fail. Same semantics as `validate_tool_references`.
    pub fn validate_spawn_references<I, S>(&self, available: I) -> Result<(), ReferenceError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let available: std::collections::HashSet<String> = available
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let unknown: Vec<&String> = self
            .spawnable_agents
            .iter()
            .filter(|name| !available.contains(name.as_str()))
            .collect();
        if unknown.is_empty() {
            return Ok(());
        }
        let mut sorted_unknown: Vec<&String> = unknown;
        sorted_unknown.sort();
        let mut sorted_available: Vec<&String> = available.iter().collect();
        sorted_available.sort();
        Err(ReferenceError::UnknownSpawnableAgents {
            id: self.id.clone(),
            unknown: sorted_unknown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            available: sorted_available
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        })
    }
}

/// Agent ids are intentionally restrictive: lowercase ASCII letters, digits,
/// and hyphens. No leading/trailing hyphen. Mirrors Codebuff's id rule and
/// avoids cross-platform path issues when ids become file names.
fn is_valid_id(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    if id.starts_with('-') || id.ends_with('-') {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_definition(id: &str) -> AgentDefinition {
        AgentDefinition {
            id: id.to_string(),
            display_name: format!("Display for {id}"),
            publisher: None,
            version: DEFAULT_AGENT_VERSION.to_string(),
            prefer_tier: None,
            model_override: None,
            reasoning: None,
            tool_names: Vec::new(),
            disallowed_tools: Vec::new(),
            spawnable_agents: Vec::new(),
            system_prompt: String::new(),
            instructions_prompt: None,
            step_prompt: None,
            spawner_prompt: None,
            inherit_parent_system_prompt: false,
            include_message_history: false,
            permission_mode: None,
            max_turns: None,
            output_mode: OutputMode::LastMessage,
            output_schema: None,
        }
    }

    #[test]
    fn id_validation_rejects_uppercase() {
        let mut d = minimal_definition("File-Picker");
        d.id = "File-Picker".to_string();
        assert!(matches!(d.validate(), Err(DefinitionError::InvalidId(_))));
    }

    #[test]
    fn id_validation_rejects_underscore() {
        let mut d = minimal_definition("file_picker");
        d.id = "file_picker".to_string();
        assert!(matches!(d.validate(), Err(DefinitionError::InvalidId(_))));
    }

    #[test]
    fn id_validation_rejects_leading_hyphen() {
        let mut d = minimal_definition("ok");
        d.id = "-bad".to_string();
        assert!(matches!(d.validate(), Err(DefinitionError::InvalidId(_))));
    }

    #[test]
    fn id_validation_accepts_normal_kebab() {
        let d = minimal_definition("file-picker-max");
        assert!(d.validate().is_ok());
    }

    #[test]
    fn inherit_and_system_prompt_conflict() {
        let mut d = minimal_definition("editor");
        d.inherit_parent_system_prompt = true;
        d.system_prompt = "should be empty".to_string();
        assert!(matches!(
            d.validate(),
            Err(DefinitionError::SystemPromptConflict { .. })
        ));
    }

    #[test]
    fn inherit_alone_is_fine() {
        let mut d = minimal_definition("editor");
        d.inherit_parent_system_prompt = true;
        d.system_prompt = String::new();
        assert!(d.validate().is_ok());
    }

    #[test]
    fn structured_output_requires_schema() {
        let mut d = minimal_definition("judge");
        d.output_mode = OutputMode::StructuredOutput;
        d.output_schema = None;
        assert!(matches!(
            d.validate(),
            Err(DefinitionError::StructuredOutputMissingSchema { .. })
        ));
    }

    #[test]
    fn structured_output_with_schema_ok() {
        let mut d = minimal_definition("judge");
        d.output_mode = OutputMode::StructuredOutput;
        d.output_schema = Some(serde_json::json!({"type": "object"}));
        assert!(d.validate().is_ok());
    }

    #[test]
    fn self_spawn_detected() {
        let mut d = minimal_definition("editor");
        d.spawnable_agents.push("editor".to_string());
        assert!(matches!(
            d.validate(),
            Err(DefinitionError::SelfSpawn { .. })
        ));
    }

    #[test]
    fn duplicate_tool_detected() {
        let mut d = minimal_definition("editor");
        d.tool_names.push("read".to_string());
        d.tool_names.push("read".to_string());
        assert!(matches!(
            d.validate(),
            Err(DefinitionError::DuplicateTool { .. })
        ));
    }

    #[test]
    fn duplicate_spawnable_detected() {
        let mut d = minimal_definition("editor");
        d.spawnable_agents.push("file-picker".to_string());
        d.spawnable_agents.push("file-picker".to_string());
        assert!(matches!(
            d.validate(),
            Err(DefinitionError::DuplicateSpawnable { .. })
        ));
    }

    #[test]
    fn resolve_model_uses_session_default_when_no_overrides() {
        let d = minimal_definition("any");
        assert_eq!(d.resolve_model("claude-sonnet"), "claude-sonnet");
    }

    #[test]
    fn resolve_model_uses_override() {
        let mut d = minimal_definition("any");
        d.model_override = Some("forced-model".to_string());
        assert_eq!(d.resolve_model("ignored"), "forced-model");
    }

    // -----------------------------------------------------------------
    // TOML round-trip — exercises serde defaults and field coverage
    // -----------------------------------------------------------------
    #[test]
    fn toml_minimal_loads_with_defaults() {
        let src = r#"
            id = "file-picker"
            display_name = "Fletcher"
        "#;
        let d: AgentDefinition = toml::from_str(src).expect("parse");
        d.validate().expect("validate");
        assert_eq!(d.id, "file-picker");
        assert_eq!(d.version, DEFAULT_AGENT_VERSION);
        assert_eq!(d.output_mode, OutputMode::LastMessage);
        assert!(d.tool_names.is_empty());
        assert!(d.spawnable_agents.is_empty());
        assert!(!d.inherit_parent_system_prompt);
    }

    #[test]
    fn toml_full_definition_loads() {
        let src = r#"
            id = "editor"
            display_name = "Code Editor"
            version = "1.2.0"
            publisher = "jcode"
            prefer_tier = "thinking"
            reasoning = "high"
            tool_names = ["str_replace", "write_file"]
            spawnable_agents = ["file-picker"]
            inherit_parent_system_prompt = true
            include_message_history = true
            output_mode = "all_messages"
            instructions_prompt = "Implement the requested change."
            step_prompt = "Continue editing."
            spawner_prompt = "Use this agent for code edits."
        "#;
        let d: AgentDefinition = toml::from_str(src).expect("parse");
        d.validate().expect("validate");
        assert_eq!(d.id, "editor");
        assert_eq!(d.version, "1.2.0");
        assert_eq!(d.publisher.as_deref(), Some("jcode"));
        assert_eq!(d.prefer_tier, Some(ModelTier::Thinking));
        assert_eq!(d.reasoning, Some(ReasoningEffort::High));
        assert_eq!(d.tool_names, vec!["str_replace", "write_file"]);
        assert!(d.inherit_parent_system_prompt);
        assert_eq!(d.output_mode, OutputMode::AllMessages);
    }

    #[test]
    fn toml_disallowed_tools_parses_and_defaults() {
        // Explicit value
        let src = r#"
            id = "restricted"
            display_name = "Restricted Agent"
            tool_names = ["read", "write_file", "bash"]
            disallowed_tools = ["bash"]
        "#;
        let d: AgentDefinition = toml::from_str(src).expect("parse");
        d.validate().expect("validate");
        assert_eq!(d.disallowed_tools, vec!["bash"]);
        assert_eq!(d.tool_names, vec!["read", "write_file", "bash"]);
        // disallowed_tools takes precedence: bash is listed in tool_names
        // but also in disallowed_tools, so the effective allowlist is
        // tool_names minus disallowed_tools = ["read", "write_file"].
        let effective: Vec<&str> = d
            .tool_names
            .iter()
            .filter(|t| !d.disallowed_tools.contains(t))
            .map(|s| s.as_str())
            .collect();
        assert_eq!(effective, vec!["read", "write_file"]);

        // Omitted field defaults to empty
        let src2 = r#"
            id = "open"
            display_name = "Open Agent"
            tool_names = ["bash"]
        "#;
        let d2: AgentDefinition = toml::from_str(src2).expect("parse");
        assert!(d2.disallowed_tools.is_empty());
    }

    #[test]
    fn toml_unknown_field_is_silently_ignored() {
        let src = r#"
            id = "ok"
            display_name = "ok"
            unknown_future_field = "value"
        "#;
        let def = toml::from_str::<AgentDefinition>(src)
            .expect("unknown fields should be ignored for forward compat");
        assert_eq!(def.id, "ok");
        assert_eq!(def.display_name, "ok");
    }

    // -----------------------------------------------------------------
    // Cross-reference validation (Phase 0.4)
    // -----------------------------------------------------------------
    #[test]
    fn validate_tool_references_passes_when_all_known() {
        let mut d = minimal_definition("editor");
        d.tool_names = vec!["read".to_string(), "write_file".to_string()];
        d.validate_tool_references(["read", "write_file", "str_replace"])
            .expect("all tools known");
    }

    #[test]
    fn validate_tool_references_fails_with_unknown_tools() {
        let mut d = minimal_definition("editor");
        d.tool_names = vec!["read".to_string(), "magic".to_string()];
        let err = d
            .validate_tool_references(["read", "write_file"])
            .expect_err("magic is unknown");
        match err {
            ReferenceError::UnknownTools {
                id,
                unknown,
                available,
            } => {
                assert_eq!(id, "editor");
                assert_eq!(unknown, "magic");
                assert!(available.contains("read"));
                assert!(available.contains("write_file"));
            }
            other => panic!("expected UnknownTools, got {:?}", other),
        }
    }

    #[test]
    fn validate_tool_references_empty_tool_names_always_ok() {
        let d = minimal_definition("ask");
        // tool_names is empty by default; supplying empty universe is also fine.
        d.validate_tool_references(Vec::<String>::new())
            .expect("empty tool list always valid");
    }

    #[test]
    fn validate_spawn_references_passes_when_all_known() {
        let mut d = minimal_definition("base");
        d.spawnable_agents = vec!["file-picker".to_string(), "editor".to_string()];
        d.validate_spawn_references(["file-picker", "editor", "reviewer"])
            .expect("all known");
    }

    #[test]
    fn validate_spawn_references_fails_with_unknown_agents() {
        let mut d = minimal_definition("base");
        d.spawnable_agents = vec!["file-picker".to_string(), "ghost".to_string()];
        let err = d
            .validate_spawn_references(["file-picker", "editor"])
            .expect_err("ghost unknown");
        match err {
            ReferenceError::UnknownSpawnableAgents {
                id,
                unknown,
                available: _,
            } => {
                assert_eq!(id, "base");
                assert_eq!(unknown, "ghost");
            }
            other => panic!("expected UnknownSpawnableAgents, got {:?}", other),
        }
    }

    #[test]
    fn validate_references_unknown_list_is_sorted_and_comma_joined() {
        let mut d = minimal_definition("agent");
        d.tool_names = vec!["zeta".to_string(), "alpha".to_string(), "mid".to_string()];
        let err = d
            .validate_tool_references(Vec::<&str>::new())
            .expect_err("none known");
        match err {
            ReferenceError::UnknownTools { unknown, .. } => {
                assert_eq!(unknown, "alpha, mid, zeta", "alphabetical order");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn toml_max_turns_parses() {
        let src = r#"
            id = "test"
            display_name = "Test"
            max_turns = 50
        "#;
        let d: AgentDefinition = toml::from_str(src).expect("parse");
        assert_eq!(d.max_turns, Some(50));
    }

    #[test]
    fn toml_max_turns_none_when_absent() {
        let src = r#"
            id = "test"
            display_name = "Test"
        "#;
        let d: AgentDefinition = toml::from_str(src).expect("parse");
        assert_eq!(d.max_turns, None);
    }
}
