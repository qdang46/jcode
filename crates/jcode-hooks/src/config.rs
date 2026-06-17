//! Hook configuration types and loader for the v2 hook system.
//!
//! Defines the 28+1 [`HookEvent`] variants, four handler types
//! ([`CommandHandlerConfig`], [`HttpHandlerConfig`], [`AgentHandlerConfig`],
//! [`PluginHandlerConfig`]), global [`HookSettings`], and the top-level
//! [`HooksConfig`] with a 3-layer TOML loader ([`load_hooks_config`]).
//!
//! Configuration is loaded from three layers (lowest to highest priority):
//!   1. `~/.jcode/hooks.toml`          (user-level)
//!   2. `.jcode/hooks.toml`            (project-level, relative to cwd)
//!   3. `$JCODE_HOOKS_CONFIG`          (env-level, path to TOML file)
//!
//! Settings from higher-priority layers override lower ones; event handlers
//! are **appended** across layers.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::matcher::HookMatcher;

// ---------------------------------------------------------------------------
// HookEvent
// ---------------------------------------------------------------------------

/// Complete set of hook lifecycle events (28 standard + `Custom`).
///
/// Each variant maps to a well-defined lifecycle point in the agent runtime.
/// The `Custom(String)` escape hatch allows user-defined event names that are
/// not in the standard set.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    // -- Core tool events (6) --
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    ToolError,
    UserPromptSubmit,
    UserPromptExpansion,

    // -- Session lifecycle (6) --
    SessionStart,
    SessionEnd,
    SessionUpdated,
    SessionDiff,
    SessionError,
    SessionIdle,

    // -- Permission events (4) --
    PermissionRequest,
    PermissionDenied,
    PermissionAsked,
    PermissionReplied,

    // -- Agent & subagent events (5) --
    AgentStart,
    AgentEnd,
    SubagentStart,
    SubagentStop,

    // -- Turn lifecycle events (1) --
    TurnEnd,

    // -- Execution control (1) --
    Stop,

    // -- Compaction events (3) --
    PreCompact,
    PostCompact,
    AutoCompactionControl,

    // -- Task & setup events (3) --
    TaskCreated,
    TaskCompleted,
    Setup,

    // -- File events (1) --
    FileChanged,

    // -- User-defined escape hatch --
    /// Allows user-defined event names not in the standard set.
    /// Configured as `Custom("my_event")` or `"custom:my_event"` in config.
    Custom(String),
}

impl HookEvent {
    /// Parse a hook event name from a free-form string.
    ///
    /// Accepts PascalCase, snake_case, kebab-case, or any mixture thereof.
    /// Matching is case-insensitive. Underscores, hyphens, and spaces are
    /// stripped before comparison.
    ///
    /// Custom events use the `"custom:<name>"` prefix, e.g. `"custom:my_event"`.
    /// The part after the colon is preserved verbatim (no normalization).
    ///
    /// Returns `None` if the input does not match any known variant and does
    /// not carry the `custom:` prefix.
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Handle Custom events before normalization to preserve the custom name.
        let lower = trimmed.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("custom:") {
            let name = trimmed[7..].trim().to_string();
            // If nothing after "custom:", store an empty name.
            return Some(Self::Custom(if name.is_empty() {
                rest.trim().to_string()
            } else {
                name
            }));
        }
        if lower == "custom" {
            return Some(Self::Custom(String::new()));
        }

        // Normalize: strip common delimiters, lowercase.
        let normalized: String = trimmed
            .chars()
            .filter(|c| *c != '_' && *c != '-' && *c != ' ')
            .collect::<String>()
            .to_ascii_lowercase();

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
            "turnend" => Some(Self::TurnEnd),
            "stop" => Some(Self::Stop),
            "precompact" => Some(Self::PreCompact),
            "postcompact" => Some(Self::PostCompact),
            "autocompactioncontrol" => Some(Self::AutoCompactionControl),
            "taskcreated" => Some(Self::TaskCreated),
            "taskcompleted" => Some(Self::TaskCompleted),
            "setup" => Some(Self::Setup),
            "filechanged" => Some(Self::FileChanged),
            _ => None,
        }
    }

    /// Whether this event can block execution (deny/ask/allow precedence).
    ///
    /// Blocking events: `PreToolUse`, `UserPromptSubmit`, `PermissionRequest`,
    /// `PermissionAsked`, `AgentStart`, `Stop`, `PreCompact`.
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            Self::PreToolUse
                | Self::UserPromptSubmit
                | Self::PermissionRequest
                | Self::PermissionAsked
                | Self::AgentStart
                | Self::Stop
                | Self::PreCompact // TurnEnd is an observer event — never blocks
        )
    }

    /// PascalCase display name (e.g. `"PreToolUse"`).
    ///
    /// For `Custom(name)` returns the stored name as-is.
    pub fn display_name(&self) -> &str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::ToolError => "ToolError",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::UserPromptExpansion => "UserPromptExpansion",
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::SessionUpdated => "SessionUpdated",
            Self::SessionDiff => "SessionDiff",
            Self::SessionError => "SessionError",
            Self::SessionIdle => "SessionIdle",
            Self::PermissionRequest => "PermissionRequest",
            Self::PermissionDenied => "PermissionDenied",
            Self::PermissionAsked => "PermissionAsked",
            Self::PermissionReplied => "PermissionReplied",
            Self::AgentStart => "AgentStart",
            Self::AgentEnd => "AgentEnd",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::TurnEnd => "TurnEnd",
            Self::Stop => "Stop",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::AutoCompactionControl => "AutoCompactionControl",
            Self::TaskCreated => "TaskCreated",
            Self::TaskCompleted => "TaskCompleted",
            Self::Setup => "Setup",
            Self::FileChanged => "FileChanged",
            Self::Custom(name) => name,
        }
    }

    /// Uppercase form suitable for env-var keys
    /// (e.g. `"PRETOOLUSE"` for `JCODE_SKIP_EVENT_PRETOOLUSE`).
    pub fn name_uppercase(&self) -> String {
        self.display_name().to_ascii_uppercase()
    }

    /// Return all 29 standard variants (excluding `Custom`).
    pub fn all_standard() -> Vec<Self> {
        vec![
            Self::PreToolUse,
            Self::PostToolUse,
            Self::PostToolUseFailure,
            Self::ToolError,
            Self::UserPromptSubmit,
            Self::UserPromptExpansion,
            Self::SessionStart,
            Self::SessionEnd,
            Self::SessionUpdated,
            Self::SessionDiff,
            Self::SessionError,
            Self::SessionIdle,
            Self::PermissionRequest,
            Self::PermissionDenied,
            Self::PermissionAsked,
            Self::PermissionReplied,
            Self::AgentStart,
            Self::AgentEnd,
            Self::SubagentStart,
            Self::SubagentStop,
            Self::TurnEnd,
            Self::Stop,
            Self::PreCompact,
            Self::PostCompact,
            Self::AutoCompactionControl,
            Self::TaskCreated,
            Self::TaskCompleted,
            Self::Setup,
            Self::FileChanged,
        ]
    }
}

impl fmt::Display for HookEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

// ---------------------------------------------------------------------------
// Matcher pattern helpers
// ---------------------------------------------------------------------------

/// Parse a matcher pattern string from a config file into a [`HookMatcher`].
///
/// Syntax:
/// - `"*"` -- matches every tool / target ([`HookMatcher::Wildcard`])
/// - `"/^Bash/"` -- regex delimited by `/` ([`HookMatcher::Regex`])
/// - `"Bash|Write|Edit"` -- pipe-separated list ([`HookMatcher::Multi`])
/// - anything else -- exact match ([`HookMatcher::Exact`])
pub fn parse_matcher_pattern(s: &str) -> HookMatcher {
    let trimmed = s.trim();
    if trimmed == "*" {
        return HookMatcher::Wildcard;
    }
    if trimmed.starts_with('/') && trimmed.ends_with('/') && trimmed.len() > 2 {
        return HookMatcher::Regex(trimmed[1..trimmed.len() - 1].to_string());
    }
    if trimmed.contains('|') {
        let parts: Vec<String> = trimmed.split('|').map(|p| p.trim().to_string()).collect();
        return HookMatcher::Multi(parts);
    }
    HookMatcher::Exact(trimmed.to_string())
}

/// Serde helper: deserialize an optional matcher string into `Option<HookMatcher>`.
fn deserialize_optional_matcher<'de, D>(deserializer: D) -> Result<Option<HookMatcher>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.map(|s| parse_matcher_pattern(&s)))
}

/// Serde helper: serialize `Option<HookMatcher>` back to a pattern string.
fn serialize_optional_matcher<S>(
    value: &Option<HookMatcher>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        None => serializer.serialize_none(),
        Some(m) => {
            let s = match m {
                HookMatcher::Wildcard => "*".to_string(),
                HookMatcher::Exact(v) => v.clone(),
                HookMatcher::Multi(parts) => parts.join("|"),
                HookMatcher::Regex(pat) => format!("/{}/", pat),
            };
            serializer.serialize_some(&s)
        }
    }
}

// ---------------------------------------------------------------------------
// Handler configuration structs
// ---------------------------------------------------------------------------

/// Shell command handler (bash / powershell).
///
/// Receives [`HookInput`](super::types::HookInput) as JSON on stdin, writes
/// [`HookOutput`](super::types::HookOutput) as JSON on stdout.
///
/// Exit codes: 0 = continue, 1 = failure, 2 = block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandHandlerConfig {
    /// Whether this handler is active.
    pub enabled: bool,
    /// The shell command to execute.
    pub command: String,
    /// Per-handler timeout override in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Extra environment variables passed to the child process.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
    /// Working directory for the child process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Matcher pattern limiting which targets this handler applies to.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_matcher",
        serialize_with = "serialize_optional_matcher"
    )]
    pub matcher: Option<HookMatcher>,
    /// Conditional expression (e.g. `"tool_name=Bash"`).
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
}

impl Default for CommandHandlerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            command: String::new(),
            timeout_secs: None,
            env: HashMap::new(),
            cwd: None,
            matcher: None,
            if_: None,
        }
    }
}

/// HTTP/REST handler.
///
/// Sends the [`HookInput`](super::types::HookInput) as a JSON body to the
/// configured URL.  Expects a JSON [`HookOutput`](super::types::HookOutput)
/// response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpHandlerConfig {
    /// Whether this handler is active.
    pub enabled: bool,
    /// Target URL.
    pub url: String,
    /// HTTP method (GET, POST, PUT, DELETE, PATCH).
    #[serde(default = "default_http_method")]
    pub method: String,
    /// Per-handler timeout in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Extra HTTP headers (values may contain `${VAR}` env interpolation).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    /// Optional static body (overrides the default JSON-serialized HookInput).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
    /// Matcher pattern.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_matcher",
        serialize_with = "serialize_optional_matcher"
    )]
    pub matcher: Option<HookMatcher>,
    /// Conditional expression.
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
}

fn default_http_method() -> String {
    "POST".to_string()
}

impl Default for HttpHandlerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            url: String::new(),
            method: default_http_method(),
            timeout_secs: None,
            headers: HashMap::new(),
            body: None,
            matcher: None,
            if_: None,
        }
    }
}

/// Inline agent handler.
///
/// Dispatches the hook to a jcode sub-agent identified by `agent_id`.
/// The agent receives the hook input as context and its response is parsed
/// as [`HookOutput`](super::types::HookOutput).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentHandlerConfig {
    /// Whether this handler is active.
    pub enabled: bool,
    /// Agent ID or name registered in jcode's agent registry.
    pub agent_id: String,
    /// Optional system-prompt override for the hook agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Timeout in seconds (default 120 s for agent tasks).
    #[serde(default = "default_agent_timeout")]
    pub timeout_secs: u64,
    /// Whether to block until the agent completes (default true).
    #[serde(default = "default_true")]
    pub wait_for_completion: bool,
    /// Matcher pattern.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_matcher",
        serialize_with = "serialize_optional_matcher"
    )]
    pub matcher: Option<HookMatcher>,
    /// Conditional expression.
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
}

fn default_agent_timeout() -> u64 {
    120
}
fn default_true() -> bool {
    true
}

impl Default for AgentHandlerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_id: String::new(),
            system_prompt: None,
            timeout_secs: default_agent_timeout(),
            wait_for_completion: true,
            matcher: None,
            if_: None,
        }
    }
}

/// External plugin/script handler.
///
/// Runs a standalone executable that receives [`HookInput`](super::types::HookInput)
/// on stdin and returns [`HookOutput`](super::types::HookOutput) on stdout,
/// following the same exit-code protocol as command hooks (0/1/2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginHandlerConfig {
    /// Whether this handler is active.
    pub enabled: bool,
    /// Path to the plugin executable.
    pub path: String,
    /// CLI arguments passed to the plugin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Plugin timeout in seconds.
    #[serde(default = "default_plugin_timeout")]
    pub timeout_secs: u64,
    /// Optional semantic version requirement (e.g. `">=1.0.0"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Matcher pattern.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_matcher",
        serialize_with = "serialize_optional_matcher"
    )]
    pub matcher: Option<HookMatcher>,
    /// Conditional expression.
    #[serde(default, rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
}

fn default_plugin_timeout() -> u64 {
    30
}

impl Default for PluginHandlerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: String::new(),
            args: Vec::new(),
            timeout_secs: default_plugin_timeout(),
            version: None,
            matcher: None,
            if_: None,
        }
    }
}

// ---------------------------------------------------------------------------
// HookHandlerConfig enum
// ---------------------------------------------------------------------------

/// Discriminated union of the four handler types.
///
/// In TOML config files each entry carries a `type` field that selects the
/// variant (`"command"`, `"http"`, `"agent"`, `"plugin"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HookHandlerConfig {
    /// Shell command handler.
    Command(CommandHandlerConfig),
    /// HTTP/REST handler.
    Http(HttpHandlerConfig),
    /// Inline agent handler.
    Agent(AgentHandlerConfig),
    /// External plugin handler.
    Plugin(PluginHandlerConfig),
}

impl Default for HookHandlerConfig {
    fn default() -> Self {
        Self::Command(CommandHandlerConfig::default())
    }
}

// ---------------------------------------------------------------------------
// HookSettings
// ---------------------------------------------------------------------------

/// Global hooks settings (the `[settings]` table in hooks.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSettings {
    /// Default timeout for all hooks in seconds (1--300, default 30).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Maximum number of hooks executed concurrently per event (default 10).
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,
    /// Log-only mode -- hooks are resolved but never executed.
    #[serde(default)]
    pub dry_run: bool,
    /// If `true`, a hook failure is treated as a block (fail-closed).
    /// If `false` (default), failures are logged and execution continues.
    #[serde(default)]
    pub fail_closed: bool,
}

fn default_timeout_secs() -> u64 {
    30
}
fn default_max_concurrency() -> usize {
    10
}

impl Default for HookSettings {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout_secs(),
            max_concurrency: default_max_concurrency(),
            dry_run: false,
            fail_closed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// HooksConfig
// ---------------------------------------------------------------------------

/// Top-level hooks configuration.
///
/// Loaded from TOML files via [`load_hooks_config`].  The `events` map uses
/// PascalCase event names as keys (e.g. `"PreToolUse"`) and a vector of
/// handler configs as values.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HooksConfig {
    /// Global settings.
    pub settings: HookSettings,
    /// Event handlers keyed by event name.
    #[serde(default)]
    pub events: HashMap<String, Vec<HookHandlerConfig>>,
}

// Custom Deserialize to support both `event` and `events` TOML keys.
impl<'de> Deserialize<'de> for HooksConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            settings: HookSettings,
            #[serde(default)]
            events: HashMap<String, Vec<HookHandlerConfig>>,
            #[serde(default)]
            event: HashMap<String, Vec<HookHandlerConfig>>,
        }

        let raw = Raw::deserialize(deserializer)?;

        // Merge: `events` takes priority; entries from `event` are appended.
        let mut events = raw.events;
        for (key, handlers) in raw.event {
            events.entry(key).or_default().extend(handlers);
        }

        Ok(HooksConfig {
            settings: raw.settings,
            events,
        })
    }
}

impl HooksConfig {
    /// Merge another config into `self`.
    ///
    /// - **Settings**: the incoming config's values override `self` field by
    ///   field (i.e. the higher-priority layer wins).
    /// - **Events**: handlers from the incoming config are **appended** to the
    ///   existing list for each event name.
    pub fn merge(&mut self, other: HooksConfig) {
        // Settings: other wins.
        self.settings.timeout_secs = other.settings.timeout_secs;
        self.settings.max_concurrency = other.settings.max_concurrency;
        self.settings.dry_run = other.settings.dry_run;
        self.settings.fail_closed = other.settings.fail_closed;

        // Events: append handlers.
        for (event_name, new_handlers) in other.events {
            self.events
                .entry(event_name)
                .or_default()
                .extend(new_handlers);
        }
    }

    /// Return `true` if no event handlers are configured.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() || self.events.values().all(Vec::is_empty)
    }
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Load hooks configuration from the 3-layer TOML hierarchy, respecting the
/// `DISABLE_JCODE_HOOKS` kill-switch.
///
/// Layers (lowest to highest priority):
///   1. `~/.jcode/hooks.toml`
///   2. `<cwd>/.jcode/hooks.toml`
///   3. Path in `$JCODE_HOOKS_CONFIG`
///
/// Returns a default (empty) config when the kill-switch env var is set or
/// when no config files are found.
pub fn load_hooks_config() -> HooksConfig {
    // Kill-switch: honour DISABLE_JCODE_HOOKS.
    if std::env::var("DISABLE_JCODE_HOOKS").is_ok() {
        eprintln!("[hooks] disabled via DISABLE_JCODE_HOOKS env var");
        return HooksConfig::default();
    }

    let mut merged = HooksConfig::default();

    // Layer 1 -- user-level (~/.jcode/hooks.toml)
    if let Some(path) = user_hooks_config_path() {
        if let Some(config) = load_hooks_config_from_path(&path) {
            merged.merge(config);
        }
    }

    // Layer 2 -- project-level (<cwd>/.jcode/hooks.toml)
    if let Some(path) = project_hooks_config_path() {
        if let Some(config) = load_hooks_config_from_path(&path) {
            merged.merge(config);
        }
    }

    // Layer 3 -- env-level ($JCODE_HOOKS_CONFIG)
    if let Some(path) = env_hooks_config_path() {
        if let Some(config) = load_hooks_config_from_path(&path) {
            merged.merge(config);
        }
    }

    merged
}

/// `$HOME/.jcode/hooks.toml`
fn user_hooks_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".jcode").join("hooks.toml"))
}

/// `<cwd>/.jcode/hooks.toml`
fn project_hooks_config_path() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|cwd| cwd.join(".jcode").join("hooks.toml"))
}

/// Path from the `JCODE_HOOKS_CONFIG` environment variable.
fn env_hooks_config_path() -> Option<PathBuf> {
    std::env::var("JCODE_HOOKS_CONFIG")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Read and parse a single TOML config file.
///
/// Returns `None` when the file does not exist or cannot be parsed (errors are
/// logged at `warn` level).
fn load_hooks_config_from_path(path: &std::path::Path) -> Option<HooksConfig> {
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<HooksConfig>(&content) {
            Ok(config) => Some(config),
            Err(e) => {
                eprintln!("Failed to parse hooks config {}: {}", path.display(), e);
                None
            }
        },
        Err(e) => {
            eprintln!("Failed to read hooks config {}: {}", path.display(), e);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy v1-to-v2 bridge
// ---------------------------------------------------------------------------

/// Convert legacy v1 hook command strings (from `config.toml [hooks]`) into
/// v2 [`CommandHandlerConfig`] entries, keyed by the corresponding v2 event name.
///
/// The v1 config had 5 hooks with simple shell command strings:
///
/// | v1 field              | v2 event        | Blocking? | Notes |
/// |-----------------------|-----------------|-----------|-------|
/// | `pre_tool`            | `PreToolUse`    | Yes       | timeout from `pre_tool_timeout_ms` |
/// | `post_tool`           | `PostToolUse`   | No        |       |
/// | `turn_end`            | `TurnEnd`       | No        |       |
/// | `session_start`       | `SessionStart`  | No        |       |
/// | `session_end`         | `SessionEnd`    | No        |       |
///
/// Returns an empty `Vec` when all inputs are `None`.
pub fn legacy_v1_to_v2_handlers(
    turn_end: Option<String>,
    session_start: Option<String>,
    session_end: Option<String>,
    pre_tool: Option<String>,
    pre_tool_timeout_ms: Option<u64>,
    post_tool: Option<String>,
) -> Vec<(String, Vec<HookHandlerConfig>)> {
    let mut entries: Vec<(String, Vec<HookHandlerConfig>)> = Vec::new();

    if let Some(cmd) = turn_end.filter(|s| !s.is_empty()) {
        entries.push((
            "TurnEnd".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: cmd,
                ..Default::default()
            })],
        ));
    }

    if let Some(cmd) = session_start.filter(|s| !s.is_empty()) {
        entries.push((
            "SessionStart".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: cmd,
                ..Default::default()
            })],
        ));
    }

    if let Some(cmd) = session_end.filter(|s| !s.is_empty()) {
        entries.push((
            "SessionEnd".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: cmd,
                ..Default::default()
            })],
        ));
    }

    if let Some(cmd) = pre_tool.filter(|s| !s.is_empty()) {
        let timeout = pre_tool_timeout_ms
            .map(|ms| (ms.max(1) / 1000).max(1))
            .filter(|&s| s > 0)
            .or(Some(30));
        entries.push((
            "PreToolUse".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: cmd,
                timeout_secs: timeout,
                ..Default::default()
            })],
        ));
    }

    if let Some(cmd) = post_tool.filter(|s| !s.is_empty()) {
        // v1 post_tool fired on both success and error. v2 splits into
        // PostToolUse (success) and PostToolUseFailure (error), so we
        // register the legacy command for both events.
        let cmd_clone = cmd.clone();
        entries.push((
            "PostToolUse".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: cmd,
                ..Default::default()
            })],
        ));
        entries.push((
            "PostToolUseFailure".to_string(),
            vec![HookHandlerConfig::Command(CommandHandlerConfig {
                command: cmd_clone,
                ..Default::default()
            })],
        ));
    }

    entries
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- HookEvent::parse ---------------------------------------------------

    #[test]
    fn parse_pascal_case() {
        assert_eq!(HookEvent::parse("PreToolUse"), Some(HookEvent::PreToolUse));
        assert_eq!(
            HookEvent::parse("PostToolUse"),
            Some(HookEvent::PostToolUse)
        );
        assert_eq!(
            HookEvent::parse("FileChanged"),
            Some(HookEvent::FileChanged)
        );
        assert_eq!(
            HookEvent::parse("AutoCompactionControl"),
            Some(HookEvent::AutoCompactionControl)
        );
    }

    #[test]
    fn parse_snake_case() {
        assert_eq!(
            HookEvent::parse("pre_tool_use"),
            Some(HookEvent::PreToolUse)
        );
        assert_eq!(
            HookEvent::parse("post_tool_use_failure"),
            Some(HookEvent::PostToolUseFailure)
        );
        assert_eq!(
            HookEvent::parse("user_prompt_submit"),
            Some(HookEvent::UserPromptSubmit)
        );
    }

    #[test]
    fn parse_kebab_case() {
        assert_eq!(
            HookEvent::parse("pre-tool-use"),
            Some(HookEvent::PreToolUse)
        );
        assert_eq!(
            HookEvent::parse("session-idle"),
            Some(HookEvent::SessionIdle)
        );
    }

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(HookEvent::parse("PRETOOLUSE"), Some(HookEvent::PreToolUse));
        assert_eq!(HookEvent::parse("pretooluse"), Some(HookEvent::PreToolUse));
        assert_eq!(HookEvent::parse("PreToolUse"), Some(HookEvent::PreToolUse));
        assert_eq!(HookEvent::parse("STOP"), Some(HookEvent::Stop));
    }

    #[test]
    fn parse_with_spaces() {
        assert_eq!(
            HookEvent::parse("Pre Tool Use"),
            Some(HookEvent::PreToolUse)
        );
    }

    #[test]
    fn parse_all_29_standard_variants() {
        let cases = &[
            ("PreToolUse", HookEvent::PreToolUse),
            ("PostToolUse", HookEvent::PostToolUse),
            ("PostToolUseFailure", HookEvent::PostToolUseFailure),
            ("ToolError", HookEvent::ToolError),
            ("UserPromptSubmit", HookEvent::UserPromptSubmit),
            ("UserPromptExpansion", HookEvent::UserPromptExpansion),
            ("SessionStart", HookEvent::SessionStart),
            ("SessionEnd", HookEvent::SessionEnd),
            ("SessionUpdated", HookEvent::SessionUpdated),
            ("SessionDiff", HookEvent::SessionDiff),
            ("SessionError", HookEvent::SessionError),
            ("SessionIdle", HookEvent::SessionIdle),
            ("PermissionRequest", HookEvent::PermissionRequest),
            ("PermissionDenied", HookEvent::PermissionDenied),
            ("PermissionAsked", HookEvent::PermissionAsked),
            ("PermissionReplied", HookEvent::PermissionReplied),
            ("AgentStart", HookEvent::AgentStart),
            ("AgentEnd", HookEvent::AgentEnd),
            ("SubagentStart", HookEvent::SubagentStart),
            ("SubagentStop", HookEvent::SubagentStop),
            ("TurnEnd", HookEvent::TurnEnd),
            ("Stop", HookEvent::Stop),
            ("PreCompact", HookEvent::PreCompact),
            ("PostCompact", HookEvent::PostCompact),
            ("AutoCompactionControl", HookEvent::AutoCompactionControl),
            ("TaskCreated", HookEvent::TaskCreated),
            ("TaskCompleted", HookEvent::TaskCompleted),
            ("Setup", HookEvent::Setup),
            ("FileChanged", HookEvent::FileChanged),
        ];
        for (input, expected) in cases {
            assert_eq!(
                HookEvent::parse(input),
                Some(expected.clone()),
                "Failed to parse '{}'",
                input
            );
        }
    }

    #[test]
    fn parse_custom_with_colon() {
        assert_eq!(
            HookEvent::parse("custom:my_event"),
            Some(HookEvent::Custom("my_event".to_string())),
        );
        assert_eq!(
            HookEvent::parse("Custom:my-event"),
            Some(HookEvent::Custom("my-event".to_string())),
        );
    }

    #[test]
    fn parse_custom_case_insensitive_prefix() {
        assert_eq!(
            HookEvent::parse("CUSTOM:foo"),
            Some(HookEvent::Custom("foo".to_string())),
        );
    }

    #[test]
    fn parse_custom_bare() {
        assert_eq!(
            HookEvent::parse("custom"),
            Some(HookEvent::Custom(String::new())),
        );
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(HookEvent::parse("NoSuchEvent"), None);
        assert_eq!(HookEvent::parse(""), None);
        assert_eq!(HookEvent::parse("   "), None);
    }

    // -- HookEvent::is_blocking ---------------------------------------------

    #[test]
    fn blocking_events() {
        let blocking = &[
            HookEvent::PreToolUse,
            HookEvent::UserPromptSubmit,
            HookEvent::PermissionRequest,
            HookEvent::PermissionAsked,
            HookEvent::AgentStart,
            HookEvent::Stop,
            HookEvent::PreCompact,
        ];
        for ev in blocking {
            assert!(ev.is_blocking(), "{:?} should be blocking", ev);
        }
    }

    #[test]
    fn non_blocking_events() {
        let non_blocking = &[
            HookEvent::PostToolUse,
            HookEvent::PostToolUseFailure,
            HookEvent::ToolError,
            HookEvent::UserPromptExpansion,
            HookEvent::SessionStart,
            HookEvent::SessionEnd,
            HookEvent::SessionUpdated,
            HookEvent::SessionDiff,
            HookEvent::SessionError,
            HookEvent::SessionIdle,
            HookEvent::PermissionDenied,
            HookEvent::PermissionReplied,
            HookEvent::AgentEnd,
            HookEvent::SubagentStart,
            HookEvent::SubagentStop,
            HookEvent::TurnEnd,
            HookEvent::PostCompact,
            HookEvent::AutoCompactionControl,
            HookEvent::TaskCreated,
            HookEvent::TaskCompleted,
            HookEvent::Setup,
            HookEvent::FileChanged,
            HookEvent::Custom("anything".to_string()),
        ];
        for ev in non_blocking {
            assert!(!ev.is_blocking(), "{:?} should NOT be blocking", ev);
        }
    }

    // -- HookEvent helpers --------------------------------------------------

    #[test]
    fn display_name_standard() {
        assert_eq!(HookEvent::PreToolUse.display_name(), "PreToolUse");
        assert_eq!(HookEvent::Stop.display_name(), "Stop");
    }

    #[test]
    fn display_name_custom() {
        assert_eq!(
            HookEvent::Custom("my_hook".to_string()).display_name(),
            "my_hook"
        );
    }

    #[test]
    fn name_uppercase() {
        assert_eq!(HookEvent::PreToolUse.name_uppercase(), "PRETOOLUSE");
        assert_eq!(
            HookEvent::AutoCompactionControl.name_uppercase(),
            "AUTOCOMPACTIONCONTROL"
        );
    }

    #[test]
    fn all_standard_has_29_variants() {
        assert_eq!(HookEvent::all_standard().len(), 29);
    }

    #[test]
    fn display_trait() {
        assert_eq!(format!("{}", HookEvent::PreToolUse), "PreToolUse");
        assert_eq!(format!("{}", HookEvent::Custom("foo".to_string())), "foo");
    }

    // -- parse_matcher_pattern -----------------------------------------------

    #[test]
    fn matcher_wildcard() {
        assert_eq!(parse_matcher_pattern("*"), HookMatcher::Wildcard);
    }

    #[test]
    fn matcher_exact() {
        assert_eq!(
            parse_matcher_pattern("Bash"),
            HookMatcher::Exact("Bash".to_string())
        );
    }

    #[test]
    fn matcher_multi() {
        assert_eq!(
            parse_matcher_pattern("Bash|Write|Edit"),
            HookMatcher::Multi(vec![
                "Bash".to_string(),
                "Write".to_string(),
                "Edit".to_string()
            ])
        );
    }

    #[test]
    fn matcher_regex() {
        assert_eq!(
            parse_matcher_pattern("/^Bash/"),
            HookMatcher::Regex("^Bash".to_string())
        );
    }

    // -- CommandHandlerConfig defaults ---------------------------------------

    #[test]
    fn command_handler_default() {
        let cfg = CommandHandlerConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.command.is_empty());
        assert!(cfg.timeout_secs.is_none());
        assert!(cfg.env.is_empty());
        assert!(cfg.cwd.is_none());
        assert!(cfg.matcher.is_none());
        assert!(cfg.if_.is_none());
    }

    // -- HooksConfig merge --------------------------------------------------

    #[test]
    fn merge_settings_override() {
        let mut base = HooksConfig::default();
        base.settings.timeout_secs = 10;

        let mut override_cfg = HooksConfig::default();
        override_cfg.settings.timeout_secs = 60;
        override_cfg.settings.dry_run = true;

        base.merge(override_cfg);
        assert_eq!(base.settings.timeout_secs, 60);
        assert!(base.settings.dry_run);
    }

    #[test]
    fn merge_events_append() {
        let mut base = HooksConfig::default();
        base.events
            .entry("PreToolUse".to_string())
            .or_default()
            .push(HookHandlerConfig::Command(CommandHandlerConfig {
                command: "hook_a".to_string(),
                ..Default::default()
            }));

        let mut other = HooksConfig::default();
        other
            .events
            .entry("PreToolUse".to_string())
            .or_default()
            .push(HookHandlerConfig::Command(CommandHandlerConfig {
                command: "hook_b".to_string(),
                ..Default::default()
            }));

        base.merge(other);
        assert_eq!(base.events["PreToolUse"].len(), 2);
    }

    #[test]
    fn merge_new_event_key() {
        let mut base = HooksConfig::default();
        let mut other = HooksConfig::default();
        other
            .events
            .entry("SessionStart".to_string())
            .or_default()
            .push(HookHandlerConfig::Http(HttpHandlerConfig {
                url: "http://localhost/hook".to_string(),
                ..Default::default()
            }));

        base.merge(other);
        assert!(base.events.contains_key("SessionStart"));
    }

    // -- TOML round-trip ----------------------------------------------------

    #[test]
    fn toml_deserialize_settings() {
        let toml = r#"
[settings]
timeout_secs = 15
max_concurrency = 5
dry_run = true
fail_closed = true
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.settings.timeout_secs, 15);
        assert_eq!(config.settings.max_concurrency, 5);
        assert!(config.settings.dry_run);
        assert!(config.settings.fail_closed);
    }

    #[test]
    fn toml_deserialize_command_handler() {
        let toml = r#"
[[events.PreToolUse]]
type = "command"
command = "check.sh"
enabled = true
timeout_secs = 5
matcher = "Bash|Write|Edit"
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.events.len(), 1);
        let handlers = &config.events["PreToolUse"];
        assert_eq!(handlers.len(), 1);
        match &handlers[0] {
            HookHandlerConfig::Command(cmd) => {
                assert_eq!(cmd.command, "check.sh");
                assert!(cmd.enabled);
                assert_eq!(cmd.timeout_secs, Some(5));
                assert_eq!(
                    cmd.matcher,
                    Some(HookMatcher::Multi(vec![
                        "Bash".to_string(),
                        "Write".to_string(),
                        "Edit".to_string()
                    ]))
                );
            }
            other => panic!("Expected Command variant, got {:?}", other),
        }
    }

    #[test]
    fn toml_deserialize_http_handler() {
        let toml = r#"
[[events.SessionEnd]]
type = "http"
url = "http://localhost:9090/hooks/session-end"
method = "POST"
timeout_secs = 5
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        let handlers = &config.events["SessionEnd"];
        assert_eq!(handlers.len(), 1);
        match &handlers[0] {
            HookHandlerConfig::Http(http) => {
                assert_eq!(http.url, "http://localhost:9090/hooks/session-end");
                assert_eq!(http.method, "POST");
                assert_eq!(http.timeout_secs, Some(5));
            }
            other => panic!("Expected Http variant, got {:?}", other),
        }
    }

    #[test]
    fn toml_deserialize_agent_handler() {
        let toml = r#"
[[events.AgentStart]]
type = "agent"
agent_id = "prompt_injector"
timeout_secs = 60
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        match &config.events["AgentStart"][0] {
            HookHandlerConfig::Agent(agent) => {
                assert_eq!(agent.agent_id, "prompt_injector");
                assert_eq!(agent.timeout_secs, 60);
            }
            other => panic!("Expected Agent variant, got {:?}", other),
        }
    }

    #[test]
    fn toml_deserialize_plugin_handler() {
        let toml = r#"
[[events.FileChanged]]
type = "plugin"
path = "/usr/local/bin/file_watcher"
args = ["--verbose"]
timeout_secs = 10
matcher = "/\\.(rs|toml)$/"
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        match &config.events["FileChanged"][0] {
            HookHandlerConfig::Plugin(plugin) => {
                assert_eq!(plugin.path, "/usr/local/bin/file_watcher");
                assert_eq!(plugin.args, vec!["--verbose".to_string()]);
                assert_eq!(
                    plugin.matcher,
                    Some(HookMatcher::Regex("\\.(rs|toml)$".to_string()))
                );
            }
            other => panic!("Expected Plugin variant, got {:?}", other),
        }
    }

    #[test]
    fn toml_event_key_alias() {
        // The `event` key (singular) should also work.
        let toml = r#"
[[event.PreToolUse]]
type = "command"
command = "legacy.toml"
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.events["PreToolUse"].len(), 1);
    }

    #[test]
    fn toml_multiple_handlers_per_event() {
        let toml = r#"
[[events.PreToolUse]]
type = "command"
command = "check_a.sh"

[[events.PreToolUse]]
type = "http"
url = "http://localhost/hooks"

[[events.PreToolUse]]
type = "command"
command = "check_b.sh"
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.events["PreToolUse"].len(), 3);
    }

    #[test]
    fn toml_if_field() {
        let toml = r#"
[[events.PreToolUse]]
type = "command"
command = "conditional.sh"
if = "tool_name=Bash"
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        match &config.events["PreToolUse"][0] {
            HookHandlerConfig::Command(cmd) => {
                assert_eq!(cmd.if_.as_deref(), Some("tool_name=Bash"));
            }
            other => panic!("Expected Command variant, got {:?}", other),
        }
    }

    #[test]
    fn toml_default_settings() {
        let toml = r#"
[[events.Stop]]
type = "command"
command = "noop"
"#;
        let config: HooksConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.settings.timeout_secs, 30);
        assert_eq!(config.settings.max_concurrency, 10);
        assert!(!config.settings.dry_run);
        assert!(!config.settings.fail_closed);
    }

    // -- HooksConfig::is_empty ----------------------------------------------

    #[test]
    fn is_empty_true_by_default() {
        let config = HooksConfig::default();
        assert!(config.is_empty());
    }

    #[test]
    fn is_empty_false_with_handlers() {
        let mut config = HooksConfig::default();
        config
            .events
            .entry("PreToolUse".to_string())
            .or_default()
            .push(HookHandlerConfig::default());
        assert!(!config.is_empty());
    }

    // -- HookEvent serde round-trip -----------------------------------------

    #[test]
    fn hook_event_serde_round_trip() {
        let events = HookEvent::all_standard();
        assert_eq!(events.len(), 29, "expected 29 standard variants");
        for ev in &events {
            let json = serde_json::to_string(ev).unwrap();
            let deserialized: HookEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(*ev, deserialized);
        }
        // Custom
        let custom = HookEvent::Custom("my_thing".to_string());
        let json = serde_json::to_string(&custom).unwrap();
        let deserialized: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(custom, deserialized);
    }

    // -- legacy_v1_to_v2_handlers -------------------------------------------

    #[test]
    fn bridge_all_none_returns_empty() {
        let entries = legacy_v1_to_v2_handlers(None, None, None, None, None, None);
        assert!(entries.is_empty());
    }

    #[test]
    fn bridge_turn_end_only() {
        let entries =
            legacy_v1_to_v2_handlers(Some("turn_end.sh".into()), None, None, None, None, None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "TurnEnd");
        assert_eq!(entries[0].1.len(), 1);
    }

    #[test]
    fn bridge_empty_string_filters_out() {
        let entries =
            legacy_v1_to_v2_handlers(Some("".into()), Some("".into()), None, None, None, None);
        assert!(entries.is_empty());
    }

    #[test]
    fn bridge_post_tool_maps_to_both_events() {
        let entries =
            legacy_v1_to_v2_handlers(None, None, None, None, None, Some("notify.sh".into()));
        assert_eq!(entries.len(), 2);
        let event_names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
        assert!(event_names.contains(&"PostToolUse"));
        assert!(event_names.contains(&"PostToolUseFailure"));
        // Both entries should have the same command string
        let cmds: Vec<&str> = entries
            .iter()
            .map(|(_, handlers)| {
                if let HookHandlerConfig::Command(cmd) = &handlers[0] {
                    cmd.command.as_str()
                } else {
                    panic!("expected Command variant");
                }
            })
            .collect();
        assert_eq!(
            cmds[0], cmds[1],
            "both PostToolUse and PostToolUseFailure should have the same command"
        );
    }

    #[test]
    fn bridge_pre_tool_timeout_conversion() {
        // 500ms -> 1s (min clamp)
        let entries =
            legacy_v1_to_v2_handlers(None, None, None, Some("check.sh".into()), Some(500), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "PreToolUse");
        if let HookHandlerConfig::Command(cmd) = &entries[0].1[0] {
            assert_eq!(cmd.timeout_secs, Some(1));
        } else {
            panic!("expected Command variant");
        }

        // 3000ms -> 3s
        let entries =
            legacy_v1_to_v2_handlers(None, None, None, Some("check.sh".into()), Some(3000), None);
        if let HookHandlerConfig::Command(cmd) = &entries[0].1[0] {
            assert_eq!(cmd.timeout_secs, Some(3));
        } else {
            panic!("expected Command variant");
        }

        // None -> default 30s
        let entries =
            legacy_v1_to_v2_handlers(None, None, None, Some("check.sh".into()), None, None);
        if let HookHandlerConfig::Command(cmd) = &entries[0].1[0] {
            assert_eq!(cmd.timeout_secs, Some(30));
        } else {
            panic!("expected Command variant");
        }
    }

    #[test]
    fn bridge_session_start_and_end() {
        let entries = legacy_v1_to_v2_handlers(
            None,
            Some("start.sh".into()),
            Some("end.sh".into()),
            None,
            None,
            None,
        );
        assert_eq!(entries.len(), 2);
        let mut map: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        for (name, handlers) in &entries {
            map.insert(name, "");
            if let HookHandlerConfig::Command(cmd) = &handlers[0] {
                assert!(!cmd.command.is_empty());
            }
        }
        assert!(map.contains_key("SessionStart"));
        assert!(map.contains_key("SessionEnd"));
    }

    #[test]
    fn bridge_all_fields_populated() {
        let entries = legacy_v1_to_v2_handlers(
            Some("turn_end.sh".into()),
            Some("start.sh".into()),
            Some("end.sh".into()),
            Some("pre_tool.sh".into()),
            Some(5000),
            Some("post_tool.sh".into()),
        );
        // Expect: TurnEnd, SessionStart, SessionEnd, PreToolUse, PostToolUse, PostToolUseFailure = 6
        assert_eq!(entries.len(), 6);
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        for expected in &[
            "TurnEnd",
            "SessionStart",
            "SessionEnd",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
        ] {
            assert!(names.contains(expected), "missing {expected}");
        }
    }
}
