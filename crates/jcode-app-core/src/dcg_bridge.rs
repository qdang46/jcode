//! Bridge between jcode's permission system and `dcg-core`'s
//! permission-modes API.
//!
//! jcode classifies *high-level intent strings* (e.g. `"read"`, `"memory"`,
//! `"todowrite"`). `dcg-core` evaluates *low-level tool calls*
//! (`ToolCall::Bash`, `ToolCall::Read`, …). This module is the adapter that
//! lets jcode delegate the "auto-allow vs requires-permission" decision to
//! `dcg-core::Engine` while preserving jcode's own queue / TUI / notification
//! plumbing.
//!
//! # Wiring
//!
//! `SafetySystem::classify(action)` calls into [`classify_via_dcg`] which:
//!
//! 1. Maps the action name to a [`dcg_core::ToolCall`] and an effect set.
//! 2. Calls [`dcg_core::Engine::evaluate`] with the configured [`Mode`].
//! 3. Returns `AutoAllowed` for `Decision::Allow`, otherwise
//!    `RequiresPermission`.
//!
//! ## What changes vs. the old `AUTO_ALLOWED` table
//!
//! - The hard-coded list is gone. Whether an action auto-allows now depends
//!   on the **mode** (`Plan`/`AcceptEdits`/`Default`/`BypassPermissions`/
//!   `DontAsk`/`Auto`) and the action's **effect classification**, not on a
//!   string match.
//! - Read-only tools (`read`, `glob`, `grep`, `ls`, `codesearch`, plus the
//!   `*_search` variants and todo / memory readers) carry only
//!   [`Effect::Read`] / [`Effect::Fs`] and therefore auto-allow under
//!   `Plan`, `Default`, `Auto`, `AcceptEdits`, `BypassPermissions`.
//! - Write-shaped tools (`todowrite`, `memory`, etc.) carry
//!   [`Effect::Write`] + [`Effect::Fs`]: auto-allow under `AcceptEdits`,
//!   `Default`, `Auto`, `BypassPermissions`; **deny under `Plan`** (which is
//!   read-only); prompt under `DontAsk` only if explicitly allow-listed.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use dcg_core::{Decision, Effect, Engine, EngineConfig, Mode, Session, ToolCall};
use jcode_agent_runtime::PermissionMode;
use jcode_hooks::{DispatchConfig, HookContext, HookEvent, HookInputBuilder, HookRegistry};

pub use crate::yolo_classifier::YoloClassifier;

/// Globally configured permission mode. Set once during CLI startup, read
/// from every `SafetySystem::classify` call.
///
/// Defaults to `Mode::Default` so behavior matches the old hard-coded
/// AUTO_ALLOWED list as closely as possible (read-only tools auto-allow,
/// everything else requires permission via `fallthrough_allows == true`
/// for `Default` plus our effect mapping below).
static GLOBAL_MODE: LazyLock<Mutex<Mode>> = LazyLock::new(|| Mutex::new(Mode::Default));

/// Per-process [`dcg_core::Engine`]. Built lazily on first `classify` call.
/// jcode runs with a single engine because protected paths and the working
/// dir are stable for the lifetime of the process.
static ENGINE: LazyLock<Engine> = LazyLock::new(|| {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    Engine::new(
        EngineConfig::builder()
            .working_dir(cwd.clone())
            .protected_paths(default_protected_paths())
            .build(),
    )
});

/// Per-process [`dcg_core::Session`]. Used by `Engine::evaluate` for
/// allow-once cache and deny counters. jcode's existing
/// `PermissionRequest` queue handles the human-prompt flow, so the
/// `Session` stays jcode-internal: it never crosses out to the user.
static SESSION: LazyLock<Mutex<Session>> = LazyLock::new(|| {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    Mutex::new(Session::with_working_dir(cwd))
});

/// Paths that should always escalate to a prompt regardless of mode
/// (matches the conservative defaults used by Claude Code).
fn default_protected_paths() -> Vec<String> {
    vec![
        "~/.ssh".to_string(),
        "~/.aws".to_string(),
        "~/.config/gh".to_string(),
        ".git".to_string(),
        ".env".to_string(),
    ]
}

/// Convert a [`PermissionMode`] (from `jcode-agent-runtime`) into the
/// corresponding [`dcg_core::Mode`]. The two enums mirror each other
/// exactly; this function is the canonical bridge.
///
/// We cannot implement `From<PermissionMode> for Mode` due to the orphan
/// rule (both types live in foreign crates). This free function serves
/// the same purpose.
#[must_use]
pub fn permission_mode_to_dcg(pm: PermissionMode) -> Mode {
    match pm {
        PermissionMode::Default => Mode::Default,
        PermissionMode::AcceptEdits => Mode::AcceptEdits,
        PermissionMode::Plan => Mode::Plan,
        PermissionMode::DontAsk => Mode::DontAsk,
        PermissionMode::BypassPermissions => Mode::BypassPermissions,
        PermissionMode::Auto => Mode::Auto,
    }
}

/// Per-session permission mode overrides. When a subagent is spawned with
/// a specific `permission_mode` from its `AgentDefinition`, it is stored
/// here keyed by the child session id. `classify_for_agent` checks this
/// map before falling back to the global mode.
static SESSION_MODES: LazyLock<Mutex<HashMap<String, Mode>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Per-session tool allow-list. When the user approves a specific pending
/// permission via the TUI dialog ("y"), the action name is recorded here for
/// the session. Subsequent calls to `classify_for_session` for that action
/// return `Allow` without re-prompting. The entry is dropped when the session
/// is cleared or when the user picks "Deny" (the map is wiped for that tool).
///
/// "Always allow for session" (`a`) sets a wildcard entry that allows every
/// tool for that session, scoped to the session — never global.
static SESSION_ALLOWED_ACTIONS: LazyLock<Mutex<HashMap<String, HashSet<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Set the global permission mode. Called from the CLI / config layer at
/// process startup. Subsequent `classify` calls observe the new mode.
pub fn set_mode(mode: Mode) {
    if let Ok(mut guard) = GLOBAL_MODE.lock() {
        *guard = mode;
    }
}

/// Tools considered too dangerous to leave in the allow-list when entering Auto mode.
/// Stripped from session allow-list on transition to Auto.
fn is_dangerous_allow_rule(tool: &str) -> bool {
    matches!(
        tool,
        "bash"
            | "write"
            | "edit"
            | "hashline_edit"
            | "patch"
            | "multiedit"
            | "apply_patch"
            | "webfetch"
            | "subagent"
    )
}

/// Strip dangerous permissions from the current session when entering a restricted mode.
pub fn strip_dangerous_permissions_for_mode(session_id: &str, target_mode: Mode) {
    if target_mode == Mode::Auto
        && let Ok(mut guard) = SESSION_ALLOWED_ACTIONS.lock()
        && let Some(actions) = guard.get_mut(session_id)
    {
        let before = actions.len();
        actions.retain(|a| !is_dangerous_allow_rule(a));
        let stripped = before - actions.len();
        if stripped > 0 {
            crate::logging::info(&format!(
                "[permission] Stripped {stripped} dangerous rule(s) for session {session_id} on Auto enter"
            ));
        }
    }
}

/// Risk level for permission explainer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "LOW"),
            RiskLevel::Medium => write!(f, "MEDIUM"),
            RiskLevel::High => write!(f, "HIGH"),
        }
    }
}

/// Rule-based permission explainer — classifies tool calls by risk level.
/// Mirrors CCB's permissionExplainer.ts.
pub fn explain_tool_call(tool_name: &str, input: &serde_json::Value) -> (RiskLevel, String) {
    match tool_name {
        "read" | "glob" | "grep" | "ffs" | "ls" | "codesearch" => (
            RiskLevel::Low,
            "Read-only operation, no side effects.".to_string(),
        ),
        "bash" => {
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.contains("rm -rf") || cmd.contains("mkfs") || cmd.contains("dd ") {
                (
                    RiskLevel::High,
                    "Destructive command: can delete data or format drives.".to_string(),
                )
            } else if cmd.contains("git push")
                || cmd.contains("git merge")
                || cmd.contains("git rebase")
            {
                (
                    RiskLevel::High,
                    "Git mutating operation: modifies remote history.".to_string(),
                )
            } else if cmd.contains("curl")
                || cmd.contains("wget")
                || cmd.contains("pip install")
                || cmd.contains("npm install")
                || cmd.contains("cargo install")
            {
                (
                    RiskLevel::Medium,
                    "Downloads external code: review before running.".to_string(),
                )
            } else if cmd.contains("sudo") || cmd.contains("chmod") || cmd.contains("chown") {
                (
                    RiskLevel::High,
                    "Privileged operation: modifies system permissions.".to_string(),
                )
            } else {
                (
                    RiskLevel::Medium,
                    "Runs a command on your system.".to_string(),
                )
            }
        }
        "edit" | "hashline_edit" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.contains("/.ssh/") || path.contains("/.aws/") || path.contains("/.config/") {
                (
                    RiskLevel::High,
                    format!("Modifies sensitive file: {}", path),
                )
            } else {
                (RiskLevel::Medium, format!("Edits file: {}", path))
            }
        }
        "write" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if path.contains("/.ssh/") || path.contains("/.aws/") || path.contains("/.config/") {
                (
                    RiskLevel::High,
                    format!("Writes to sensitive path: {}", path),
                )
            } else {
                (RiskLevel::Medium, format!("Writes file: {}", path))
            }
        }
        "webfetch" | "websearch" => (
            RiskLevel::Medium,
            "Downloads content from the internet.".to_string(),
        ),
        "subagent" | "bg" | "batch" => (
            RiskLevel::Medium,
            "Spawns a background process.".to_string(),
        ),
        _ => (RiskLevel::Low, "Standard tool operation.".to_string()),
    }
}

/// One-shot channel for pending permission responses.
/// When tool execution needs user approval, it creates a oneshot channel,
/// stores the sender here, and awaits the receiver.
/// The TUI dialog handler sends the response through the sender.
static PERMISSION_RESPONSE: LazyLock<Mutex<Option<tokio::sync::oneshot::Sender<bool>>>> =
    LazyLock::new(|| Mutex::new(None));

/// Denial tracking state — after N consecutive denials, show warning.
static DENIAL_COUNTS: LazyLock<Mutex<HashMap<String, (u32, u32)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const MAX_CONSECUTIVE_DENIALS: u32 = 3;
const MAX_TOTAL_DENIALS: u32 = 20;

/// Record a denial for the given session.
pub fn record_denial(session_id: &str) {
    if let Ok(mut guard) = DENIAL_COUNTS.lock() {
        let entry = guard.entry(session_id.to_string()).or_insert((0, 0));
        entry.0 += 1; // consecutive
        entry.1 += 1; // total
    }
}

/// Record an approval — resets consecutive counter.
pub fn record_approval(session_id: &str) {
    if let Ok(mut guard) = DENIAL_COUNTS.lock() {
        let entry = guard.entry(session_id.to_string()).or_insert((0, 0));
        entry.0 = 0; // reset consecutive
    }
}

/// Check if denial limits are exceeded for the given session.
pub fn denial_limit_exceeded(session_id: &str) -> Option<&'static str> {
    if let Ok(guard) = DENIAL_COUNTS.lock()
        && let Some(&(consec, total)) = guard.get(session_id)
    {
        if consec >= MAX_CONSECUTIVE_DENIALS {
            return Some("You've denied 3 times. Consider reviewing carefully.");
        }
        if total >= MAX_TOTAL_DENIALS {
            return Some("You've denied 20 times. Consider switching to AcceptEdits mode.");
        }
    }
    None
}

/// Get the denial count for display in the dialog.
pub fn denial_count(session_id: &str) -> (u32, u32) {
    if let Ok(guard) = DENIAL_COUNTS.lock() {
        guard.get(session_id).copied().unwrap_or((0, 0))
    } else {
        (0, 0)
    }
}

/// Wait for the user to respond to a pending permission request.
/// Returns `Ok(true)` if approved (tool should proceed).
/// Returns `Ok(false)` if denied (tool should fail).
pub async fn await_permission_response() -> anyhow::Result<bool> {
    let rx = {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut guard) = PERMISSION_RESPONSE.lock() {
            *guard = Some(tx);
        }
        rx
    };
    match rx.await {
        Ok(true) => Ok(true),
        Ok(false) => Ok(false),
        Err(_) => Err(anyhow::anyhow!("Permission request cancelled")),
    }
}

/// Signal the pending permission request with the user's decision.
/// Called from the TUI dialog handler.
pub fn signal_permission_response(approved: bool) {
    if let Ok(mut guard) = PERMISSION_RESPONSE.lock()
        && let Some(tx) = guard.take()
    {
        let _ = tx.send(approved);
    }
}

/// Return the currently configured permission mode.
#[must_use]
pub fn current_mode() -> Mode {
    GLOBAL_MODE
        .lock()
        .map(|guard| *guard)
        .unwrap_or(Mode::Default)
}

/// Cycle to the next permission mode in the defined order.
///
/// Order: Default → AcceptEdits → Plan → Auto → DontAsk → BypassPermissions → Default
pub fn cycle_mode() -> Mode {
    let mut guard = GLOBAL_MODE.lock().unwrap_or_else(|e| e.into_inner());
    let next = match *guard {
        Mode::Default => Mode::AcceptEdits,
        Mode::AcceptEdits => Mode::Plan,
        Mode::Plan => Mode::Auto,
        Mode::Auto => Mode::DontAsk,
        Mode::DontAsk => Mode::BypassPermissions,
        Mode::BypassPermissions => Mode::Default,
    };
    *guard = next;
    next
}

/// Set permission mode from a string (e.g., from CLI args or config).
/// Returns true if the string was a valid mode and the mode was changed.
pub fn set_mode_from_str(s: &str) -> bool {
    let mode = match s.trim().to_ascii_lowercase().as_str() {
        "default" => Mode::Default,
        "accept-edits" => Mode::AcceptEdits,
        "plan" => Mode::Plan,
        "auto" => Mode::Auto,
        "dont-ask" => Mode::DontAsk,
        "bypass-permissions" => Mode::BypassPermissions,
        _ => return false,
    };
    set_mode(mode);
    true
}

/// Try to consume an allow-once code, returning `true` if the code was valid
/// and the associated action is now approved.
///
/// Allow-once codes are SHA-256 derived, 6-hex-char codes with a 24-hour TTL.
/// They are generated by `dcg-core::Session::generate_allow_once_code` when a
/// `Decision::Prompt` is returned, and consumed here after user approval.
///
/// Length and shape are validated **before** any hash work to prevent
/// pathological inputs (e.g. multi-megabyte argv from `jcode permission allow`)
/// from blocking the global `SESSION` mutex.
pub fn consume_allow_once(code: &str) -> bool {
    if !is_valid_allow_once_code(code) {
        return false;
    }
    SESSION
        .lock()
        .ok()
        .map(|mut session| session.consume_allow_once(code))
        .unwrap_or(false)
}

/// Validate the shape of an allow-once code without touching the session.
/// Public so the CLI subcommand and the TUI dialog can apply the same gate
/// before hashing.
pub fn is_valid_allow_once_code(code: &str) -> bool {
    code.len() == 6 && code.chars().all(|c| c.is_ascii_hexdigit())
}

/// Record a user "Approve for this tool" decision in the per-session allow
/// list. Future calls to `classify_for_session` for the same `action` on the
/// same `session_id` will return `Allow` without re-prompting.
pub fn approve_session_action(session_id: &str, action: &str) {
    if let Ok(mut guard) = SESSION_ALLOWED_ACTIONS.lock() {
        guard
            .entry(session_id.to_string())
            .or_default()
            .insert(action.to_string());
    }
}

/// Approve every tool for the given session. The "Always allow" path.
pub fn approve_session_all(session_id: &str) {
    if let Ok(mut guard) = SESSION_ALLOWED_ACTIONS.lock() {
        let entry = guard.entry(session_id.to_string()).or_default();
        entry.clear();
        entry.insert("*".to_string());
    }
}

/// True if the session allow-list allows `action` (either the exact action
/// or a wildcard `*`).
pub fn session_allows_action(session_id: &str, action: &str) -> bool {
    SESSION_ALLOWED_ACTIONS
        .lock()
        .ok()
        .and_then(|guard| guard.get(session_id).cloned())
        .map(|set| set.contains("*") || set.contains(action))
        .unwrap_or(false)
}

/// List all actions currently in the session allow-list.
pub fn session_allowed_actions_list(session_id: &str) -> Vec<String> {
    SESSION_ALLOWED_ACTIONS
        .lock()
        .ok()
        .and_then(|guard| guard.get(session_id).cloned())
        .map(|set| {
            let mut v: Vec<String> = set.into_iter().collect();
            v.sort();
            v
        })
        .unwrap_or_default()
}

/// Remove a specific action from the session allow-list.
pub fn clear_session_allowed_action(session_id: &str, action: &str) {
    if let Ok(mut guard) = SESSION_ALLOWED_ACTIONS.lock()
        && let Some(actions) = guard.get_mut(session_id)
    {
        actions.remove(action);
    }
}

/// Convert a `Mode` to a human-readable string for TUI display.
pub fn mode_to_str(mode: Mode) -> &'static str {
    match mode {
        Mode::Default => "default",
        Mode::AcceptEdits => "accept-edits",
        Mode::Plan => "plan",
        Mode::Auto => "auto",
        Mode::DontAsk => "dont-ask",
        Mode::BypassPermissions => "bypass-permissions",
    }
}

/// Store a per-session permission mode override. Called when a subagent
/// is spawned with an explicit `permission_mode` from its agent
/// definition.
pub fn set_session_mode(session_id: &str, mode: Mode) {
    if let Ok(mut guard) = SESSION_MODES.lock() {
        guard.insert(session_id.to_string(), mode);
    }
}

/// Remove the per-session permission mode override for a session that
/// has finished. Prevents unbounded growth of the map.
pub fn clear_session_mode(session_id: &str) {
    if let Ok(mut guard) = SESSION_MODES.lock() {
        guard.remove(session_id);
    }
    // Also drop any per-session allow-list entries for this session so a
    // long-lived process doesn't accumulate stale approved-tool sets.
    if let Ok(mut guard) = SESSION_ALLOWED_ACTIONS.lock() {
        guard.remove(session_id);
    }
}

/// Return the per-session mode override, if any.
#[must_use]
pub fn session_mode(session_id: &str) -> Option<Mode> {
    SESSION_MODES
        .lock()
        .ok()
        .and_then(|guard| guard.get(session_id).copied())
}

/// RAII guard that clears a per-session permission mode on drop.
///
/// Use this instead of manual `set_session_mode` / `clear_session_mode`
/// pairs to guarantee cleanup even when the subagent exits via early
/// return or error path.
pub struct SessionModeGuard {
    session_id: String,
}

impl SessionModeGuard {
    /// Set the per-session mode and return a guard that will clear it on
    /// drop. If `mode` is `None`, no override is set and the guard is a
    /// no-op on drop (but still safe to hold).
    #[must_use]
    pub fn new(session_id: &str, mode: Option<Mode>) -> Self {
        if let Some(mode) = mode {
            set_session_mode(session_id, mode);
        }
        Self {
            session_id: session_id.to_string(),
        }
    }
}

impl Drop for SessionModeGuard {
    fn drop(&mut self) {
        clear_session_mode(&self.session_id);
    }
}

/// Classify an action using the agent-specific permission mode when
/// provided, falling back to the global mode otherwise.
///
/// This is the entry point that respects per-agent permission overrides.
/// Call sites that know the agent's `PermissionMode` (e.g. subagent tool
/// execution) should use this instead of [`classify`].
#[must_use]
pub fn classify_for_agent(
    action: &str,
    agent_permission_mode: Option<PermissionMode>,
) -> BridgeDecision {
    let mode = agent_permission_mode
        .map(permission_mode_to_dcg)
        .unwrap_or_else(current_mode);
    classify_with_mode(action, mode)
}

/// Classify an action using the per-session mode override when one exists
/// for `session_id`, falling back to the global mode otherwise.
///
/// This is the session-aware variant of [`classify`]. Call sites that
/// know the session id (e.g. tool execution within a subagent) should
/// prefer this over the global [`classify`] so that per-session
/// permission overrides set via [`set_session_mode`] are honoured.
///
/// The per-session tool allow-list (populated by user approvals in the
/// TUI dialog) is checked first; if the action is on the list, the
/// call short-circuits to `Allow` without running the engine.
#[must_use]
pub fn classify_for_session(action: &str, session_id: &str) -> BridgeDecision {
    if session_allows_action(session_id, action) {
        return BridgeDecision::Allow;
    }

    // Check tool-level allow/deny/ask rules from execution policy config.
    // These handle bare tool names like "WebSearch" in deny/ask/allow lists.
    // Bash commands with patterns (e.g., "Bash(ls *)") are matched at the
    // command level via classify_command's execution_policy::evaluate().
    if let Some(decision) = crate::execution_policy::evaluate_tool(action) {
        use crate::execution_policy::PolicyDecision;
        return match decision {
            PolicyDecision::Allow { .. } => BridgeDecision::Allow,
            PolicyDecision::Deny {
                reason,
                alternatives,
                ..
            } => BridgeDecision::Deny {
                reason,
                alternatives,
            },
            PolicyDecision::Prompt {
                reason,
                allow_once_code,
                alternatives,
            } => BridgeDecision::Prompt {
                reason,
                allow_once_code,
                alternatives,
            },
        };
    }

    let mode = session_mode(session_id).unwrap_or_else(current_mode);
    classify_with_mode(action, mode)
}

/// Three-state outcome from the bridge. jcode's `SafetySystem` collapses
/// `Allow` to `ActionTier::AutoAllowed` and `Prompt`/`Deny` to
/// `ActionTier::RequiresPermission` — but exposing the full set here
/// lets future call sites (e.g. CLI hooks, MCP servers) react to a hard
/// `Deny` without surfacing a permission prompt the user can never
/// usefully approve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeDecision {
    /// dcg-core allowed the action under the current mode.
    Allow,
    /// dcg-core wants a human prompt; jcode should queue a
    /// `PermissionRequest` with the given reason and allow-once code.
    Prompt {
        /// Short human-readable explanation of why the prompt was raised.
        reason: String,
        /// Single-use code (6 hex chars) that scopes the eventual approval
        /// to the exact action in this session.
        allow_once_code: String,
        /// Suggested safer alternatives (e.g. "use `git stash` first").
        alternatives: Vec<String>,
    },
    /// dcg-core denied outright (e.g. `Plan` mode + write effect).
    Deny {
        /// Short human-readable explanation of why the call was denied.
        reason: String,
        /// Suggested safer alternatives.
        alternatives: Vec<String>,
    },
}

/// Classify a jcode action via dcg-core. The caller is responsible for
/// translating the result into its own `ActionTier` / `PermissionResult`
/// vocabulary.
#[must_use]
pub fn classify(action: &str) -> BridgeDecision {
    classify_with_mode(action, current_mode())
}

/// Same as [`classify`] but with an explicit mode override (mainly for
/// tests).
#[must_use]
pub fn classify_with_mode(action: &str, mode: Mode) -> BridgeDecision {
    let lower = action.to_lowercase();

    // Phase-A behavior preservation:
    //
    // `dcg-core` Phase A does not yet expose a rule layer, so
    // `Mode::Default::fallthrough_allows()` returns `true` — meaning every
    // unmatched call would auto-allow. That regresses jcode's legacy
    // AUTO_ALLOWED-based classify, which only auto-allows a fixed set of
    // read-only / stateful-safe intents. Until dcg-core Phase 2 wires
    // pack-rule evaluation in, we keep the legacy gate inline for the
    // `Default` and `Auto` modes. The advanced modes (`Plan`,
    // `AcceptEdits`, `DontAsk`) defer to
    // `Engine::evaluate` because their pre-check semantics are well
    // defined without rule data.
    if matches!(mode, Mode::Default | Mode::Auto) {
        // Legacy auto-allowed tools always allow in both Default and Auto.
        if is_legacy_auto_allowed(&lower) {
            return BridgeDecision::Allow;
        }

        // For Mode::Auto, non-legacy tools go through YOLO classifier.
        if mode == Mode::Auto {
            let (tool, effects) = action_to_tool_call(&lower);
            let effect_strings: Vec<String> = effects
                .iter()
                .map(|e| match e {
                    Effect::Read => "Read".to_string(),
                    Effect::Write => "Write".to_string(),
                    Effect::Spawn => "Spawn".to_string(),
                    Effect::Fs => "Fs".to_string(),
                    Effect::Irreversible => "Irreversible".to_string(),
                    Effect::Network => "Network".to_string(),
                    // CredentialAccess and PrivilegeEscalation are dcg-core Phase B
                    other => format!("{:?}", other),
                })
                .collect();

            let classifier = YoloClassifier::get_or_init();
            return classifier.evaluate(&lower, &format!("{:?}", tool), &effect_strings);
        }

        return BridgeDecision::Prompt {
            reason: format!(
                "Tool action '{}' requires permission in current mode {:?}",
                lower,
                current_mode()
            ),
            allow_once_code: String::new(),
            alternatives: vec![],
        };
    }

    // BypassPermissions mode short-circuits to Allow — no engine evaluation.
    if matches!(mode, Mode::BypassPermissions) {
        return BridgeDecision::Allow;
    }

    let (tool, effects) = action_to_tool_call(&lower);

    // Engine::evaluate takes &mut Session; we serialize on the global
    // mutex. classify is now called from validate_tool_allowed which
    // fires on every tool execution — but only when the mode is *not*
    // Default or Auto (which short-circuit through the legacy gate above
    // for known tools). For the hot path in Default/Auto the lock is
    // never touched. For Plan/AcceptEdits/DontAsk the lock is held only
    // for the ~microsecond it takes dcg-core to evaluate a single call.
    //
    // Future optimization: memoize (action, mode) -> BridgeDecision for
    // the duration of a turn to avoid re-evaluating when the same tool
    // is called repeatedly.
    let decision = match SESSION.lock() {
        Ok(mut session) => ENGINE.evaluate(&mut session, &tool, mode, &effects),
        // If the session mutex is poisoned we fall back to "needs prompt"
        // which is the safest choice for jcode's queue/TUI flow.
        Err(_) => {
            return BridgeDecision::Prompt {
                reason: "Session poisoned".into(),
                allow_once_code: String::new(),
                alternatives: vec![],
            };
        }
    };

    match decision {
        Decision::Allow => BridgeDecision::Allow,
        Decision::Prompt {
            reason,
            allow_once_code,
            alternatives,
        } => BridgeDecision::Prompt {
            reason,
            allow_once_code,
            alternatives,
        },
        Decision::Deny {
            reason,
            alternatives,
        } => BridgeDecision::Deny {
            reason,
            alternatives,
        },
    }
}

/// Dispatch permission-related hooks after a bridge classification.
///
/// This is the integration point between dcg-core's permission decision and
/// the jcode hooks v2 system. It fires the appropriate hook event based on the
/// [`BridgeDecision`] so that user-configured hooks can observe or override
/// permission outcomes.
///
/// # Behavior
///
/// - [`BridgeDecision::Prompt`]: Dispatches `PermissionRequest` hooks. If any
///   hook returns a **deny** decision, this function returns `true` (meaning
///   the caller should treat the request as blocked). Otherwise returns
///   `false` (proceed with the normal prompt flow).
/// - [`BridgeDecision::Deny`]: Dispatches `PermissionDenied` hooks as an
///   **observational** event (fire-and-forget). Always returns `false` since
///   the decision is already a denial.
/// - [`BridgeDecision::Allow`]: No-op, returns `false`.
///
/// # Errors
///
/// Hook dispatch failures are logged to stderr but never propagated. A
/// failing hook never blocks or changes the permission outcome.
pub async fn dispatch_permission_hooks(
    action: &str,
    decision: BridgeDecision,
    session_id: &str,
    cwd: &str,
) -> bool {
    match decision {
        BridgeDecision::Allow => return false,
        BridgeDecision::Prompt { .. } | BridgeDecision::Deny { .. } => {}
    }

    let config = jcode_hooks::load_hooks_config();
    if config.is_empty() {
        return false;
    }

    let registry = HookRegistry::from_config(config.clone());

    let (event, mut context) = match decision {
        BridgeDecision::Prompt { .. } => (
            HookEvent::PermissionRequest,
            HookContext::new(session_id, "", cwd, "PermissionRequest"),
        ),
        BridgeDecision::Deny { .. } => (
            HookEvent::PermissionDenied,
            HookContext::new(session_id, "", cwd, "PermissionDenied"),
        ),
        BridgeDecision::Allow => unreachable!(),
    };
    let mode_name = format!("{:?}", current_mode());
    context.tool_name = Some(action.to_string());
    context.permission_mode = Some(mode_name.clone());

    let handlers = registry.get_matching(&event, &context);
    if handlers.is_empty() {
        return false;
    }

    let input = HookInputBuilder::new()
        .session(session_id, cwd)
        .event(event.display_name())
        .permission(&mode_name, "", action)
        .build();

    let dispatch_config = DispatchConfig::from_settings(&config.settings);
    let stats = jcode_hooks::dispatch_hooks(&event, &input, &handlers, &dispatch_config).await;

    // For PermissionRequest: return true if any hook denied (blocks the prompt).
    // For PermissionDenied: fire-and-forget, always return false.
    if matches!(decision, BridgeDecision::Prompt { .. }) {
        stats.any_denied()
    } else {
        false
    }
}

/// Dispatch `PermissionAsked` hooks when a permission request is presented to
/// the user.
///
/// This is a **blocking** event — hooks can return `"allow"` to pre-approve
/// the permission (skipping the user prompt) or `"deny"` to block it.
///
/// # Returns
///
/// `true` if any hook pre-approved the permission (the caller should treat
/// the request as auto-approved). `false` otherwise (proceed with normal
/// prompt flow, or a hook denied).
pub async fn dispatch_permission_asked_hooks(
    action: &str,
    request_id: &str,
    session_id: &str,
    cwd: &str,
) -> bool {
    let config = jcode_hooks::load_hooks_config();
    if config.is_empty() {
        return false;
    }

    let registry = HookRegistry::from_config(config.clone());
    let mode_name = format!("{:?}", current_mode());

    let context = HookContext::for_permission_asked(
        action.to_string(),
        session_id.to_string(),
        mode_name.clone(),
        request_id.to_string(),
    );

    let event = HookEvent::PermissionAsked;
    let handlers = registry.get_matching(&event, &context);
    if handlers.is_empty() {
        return false;
    }

    let input = HookInputBuilder::new()
        .session(session_id, cwd)
        .event(event.display_name())
        .permission(&mode_name, request_id, action)
        .build();

    let dispatch_config = DispatchConfig::from_settings(&config.settings);
    let stats = jcode_hooks::dispatch_hooks(&event, &input, &handlers, &dispatch_config).await;

    // Return true if any hook explicitly allowed (pre-approve).
    stats.allowed > 0
}

/// Dispatch `PermissionReplied` hooks after a permission decision is recorded.
///
/// This is an **observational** event — hooks cannot change the outcome.
/// Fire-and-forget: failures are logged but never propagated.
pub async fn dispatch_permission_replied_hooks(
    request_id: &str,
    session_id: &str,
    approved: bool,
    via: &str,
) {
    let config = jcode_hooks::load_hooks_config();
    if config.is_empty() {
        return;
    }

    let registry = HookRegistry::from_config(config.clone());

    let mut context = HookContext::for_permission_replied(
        request_id.to_string(),
        session_id.to_string(),
        approved,
    );
    // Populate permission_decision so hooks can see the outcome.
    context.permission_mode = Some(via.to_string());

    let event = HookEvent::PermissionReplied;
    let handlers = registry.get_matching(&event, &context);
    if handlers.is_empty() {
        return;
    }

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let input = HookInputBuilder::new()
        .session(session_id, &cwd)
        .event(event.display_name())
        .permission(via, request_id, "")
        .build();
    // Populate permission_decision in the input.
    let mut input = input;
    input.permission_decision = Some(if approved { "approved" } else { "denied" }.to_string());

    let dispatch_config = DispatchConfig::from_settings(&config.settings);
    let _ = jcode_hooks::dispatch_hooks(&event, &input, &handlers, &dispatch_config).await;
}

/// Centralized list of action names that auto-allowed under jcode's
/// legacy `AUTO_ALLOWED` table. Used by the `Default` / `Auto` mode path.
/// Kept in lockstep with [`action_to_tool_call`] so the two views never
/// drift.
fn is_legacy_auto_allowed(action_lower: &str) -> bool {
    READ_ONLY_ACTIONS.contains(&action_lower) || STATEFUL_SAFE_ACTIONS.contains(&action_lower)
}

/// Read-only intents (used to populate the legacy AUTO_ALLOWED list).
const READ_ONLY_ACTIONS: &[&str] = &[
    "read",
    "glob",
    "ffs glob",
    "grep",
    "ffs grep",
    "ls",
    "codesearch",
    "conversation_search",
    "session_search",
    "todoread",
    "ffs outline",
    "ffs find",
    "ffs callers",
    "ffs callees",
    "ffs refs",
    "ffs flow",
    "ffs dispatch",
    "ffs symbol",
];

/// Stateful but non-destructive intents — write to jcode-managed scratch
/// state, never to user files.
const STATEFUL_SAFE_ACTIONS: &[&str] = &["memory", "todo", "todowrite"];

/// Map a lowercased jcode action name to a `(ToolCall, Effects)` pair.
///
/// `dcg-core::ToolCall` only has `Bash | Edit | Write | Read | Network`
/// variants, so we approximate jcode's higher-level action vocabulary:
///
/// - **Read-only** intents (`read`, `glob`, `grep`, `ls`, `codesearch`,
///   `*_search`, `todoread`) → `ToolCall::Read` with `[Read, Fs]`.
/// - **Write-stateful** intents (`memory`, `todo`, `todowrite`) →
///   `ToolCall::Write` with `[Write, Fs]`. This deliberately uses
///   `ToolCall::Write` (not `Bash`) so `Mode::AcceptEdits` auto-allows
///   them, matching Claude Code's "edits are auto-OK" semantics.
/// - **Shell-like** intents (`bash`, `shell`, `run_terminal_cmd`,
///   `execute_command`) → `ToolCall::Bash` with `[Spawn, Write,
///   Irreversible]`.
/// - Anything else → `ToolCall::Bash` (conservative) with `[Write,
///   Irreversible]`, mirroring the legacy
///   `ActionTier::RequiresPermission` fall-through.
///
/// The placeholder `PathBuf` for `Read`/`Write` does not influence the
/// Phase-A engine because protected-path checks operate on a known list,
/// not on the call's path. Phase 2 (pack rules) will need a richer
/// classify-with-context entry point.
fn action_to_tool_call(action_lower: &str) -> (ToolCall, Vec<Effect>) {
    use Effect::{Fs, Irreversible, Read, Spawn, Write};

    // Placeholder path: the real path is not known at classify time and
    // Phase-A engine only consults protected_paths against
    // path_in_protected, which we leave conservative-false here.
    let placeholder = PathBuf::from(".");

    if READ_ONLY_ACTIONS.contains(&action_lower) {
        return (ToolCall::read(placeholder), vec![Read, Fs]);
    }

    if STATEFUL_SAFE_ACTIONS.contains(&action_lower) {
        return (ToolCall::write(placeholder), vec![Write, Fs]);
    }

    // Bash / shell-like actions — surface to dcg-core as a real
    // `ToolCall::Bash` so future Phase-2 pack rules can match them.
    if matches!(
        action_lower,
        "bash" | "shell" | "run_terminal_cmd" | "execute_command"
    ) {
        // Empty command string keeps Phase-A evaluation (mode + protected
        // paths) accurate without claiming a specific command — the real
        // command is only known once the agent issues it. Phase 2 will
        // need a richer wiring point.
        return (ToolCall::bash(""), vec![Spawn, Write, Irreversible]);
    }

    // MCP tool actions: mcp__serverName__toolName
    // Three matching levels:
    //   mcp__github          → matches ALL tools from github server
    //   mcp__github__*        → wildcard, same as above
    //   mcp__github__create_pull_request → exact tool
    if action_lower.starts_with("mcp__") {
        let parts: Vec<&str> = action_lower.split("__").collect();
        if parts.len() >= 2 {
            // MCP tools carry Read + Write + Spawn effects since they can
            // read/write data and spawn background processes.
            // Path is unknown at classify time — use placeholder.
            return (ToolCall::read(placeholder), vec![Read, Write, Spawn]);
        }
    }

    // Conservative default for unknown / future tools. We still surface a
    // ToolCall::Bash so the engine treats it as command-shaped rather
    // than file-shaped.
    (ToolCall::bash(action_lower), vec![Write, Irreversible])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In `Default` mode, the legacy AUTO_ALLOWED tools must still
    /// auto-allow so existing jcode workflows are not regressed.
    #[test]
    fn default_mode_auto_allows_legacy_read_tools() {
        for action in [
            "read",
            "glob",
            "ffs glob",
            "grep",
            "ffs grep",
            "ls",
            "memory",
            "todo",
            "todowrite",
            "todoread",
            "conversation_search",
            "session_search",
            "codesearch",
        ] {
            assert_eq!(
                classify_with_mode(action, Mode::Default),
                BridgeDecision::Allow,
                "{action} must auto-allow in Default mode"
            );
        }
    }

    /// Under `Plan` mode, read-only actions still allow but write-shaped
    /// stateful tools must deny — that's the whole point of plan mode.
    #[test]
    fn plan_mode_denies_write_shaped_tools() {
        assert_eq!(
            classify_with_mode("read", Mode::Plan),
            BridgeDecision::Allow,
            "read must allow in Plan"
        );
        assert!(
            matches!(
                classify_with_mode("todowrite", Mode::Plan),
                BridgeDecision::Deny { .. }
            ),
            "todowrite must deny in Plan"
        );
        assert!(
            matches!(
                classify_with_mode("memory", Mode::Plan),
                BridgeDecision::Deny { .. }
            ),
            "memory writes must deny in Plan"
        );
    }

    /// `BypassPermissions` is the escape hatch: every action allows.
    #[test]
    fn bypass_mode_allows_everything() {
        for action in ["read", "todowrite", "shell", "made_up_tool"] {
            assert_eq!(
                classify_with_mode(action, Mode::BypassPermissions),
                BridgeDecision::Allow,
                "{action} must allow in Bypass"
            );
        }
    }

    /// Unknown actions in `Default` mode must Prompt, matching the legacy
    /// `AUTO_ALLOWED`-based behavior where anything not in the safe list
    /// surfaced as `RequiresPermission`.
    #[test]
    fn default_mode_prompts_for_unknown_actions() {
        for action in [
            "bash",
            "edit",
            "write",
            "create_pull_request",
            "send_email",
            "future_destructive_tool",
        ] {
            assert!(
                matches!(
                    classify_with_mode(action, Mode::Default),
                    BridgeDecision::Prompt { .. }
                ),
                "{action} must require permission in Default mode"
            );
        }
    }

    /// Case-insensitivity matches the legacy `to_lowercase()` behavior.
    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(
            classify_with_mode("READ", Mode::Default),
            BridgeDecision::Allow
        );
        assert!(matches!(
            classify_with_mode("Bash", Mode::Default),
            BridgeDecision::Prompt { .. }
        ));
    }

    #[test]
    fn set_and_read_back_mode() {
        let original = current_mode();
        set_mode(Mode::Plan);
        assert_eq!(current_mode(), Mode::Plan);
        // Restore so other tests aren't affected by ordering.
        set_mode(original);
    }

    #[test]
    fn permission_mode_converts_to_dcg_mode() {
        use jcode_agent_runtime::permission::PermissionMode as PM;

        assert_eq!(permission_mode_to_dcg(PM::Default), Mode::Default);
        assert_eq!(permission_mode_to_dcg(PM::AcceptEdits), Mode::AcceptEdits);
        assert_eq!(permission_mode_to_dcg(PM::Plan), Mode::Plan);
        assert_eq!(permission_mode_to_dcg(PM::DontAsk), Mode::DontAsk);
        assert_eq!(
            permission_mode_to_dcg(PM::BypassPermissions),
            Mode::BypassPermissions
        );
        assert_eq!(permission_mode_to_dcg(PM::Auto), Mode::Auto);
    }

    #[test]
    fn classify_for_agent_uses_agent_mode_when_set() {
        use jcode_agent_runtime::permission::PermissionMode as PM;

        // todowrite auto-allows in AcceptEdits but denies in Plan
        assert_eq!(
            classify_for_agent("todowrite", Some(PM::AcceptEdits)),
            BridgeDecision::Allow,
            "todowrite must allow in AcceptEdits"
        );
        assert!(
            matches!(
                classify_for_agent("todowrite", Some(PM::Plan)),
                BridgeDecision::Deny { .. }
            ),
            "todowrite must deny in Plan"
        );
    }

    #[test]
    fn classify_for_agent_falls_back_to_global_when_none() {
        let original = current_mode();
        set_mode(Mode::BypassPermissions);
        assert_eq!(
            classify_for_agent("made_up_tool", None),
            BridgeDecision::Allow,
            "falls back to global BypassPermissions mode"
        );
        set_mode(original);
    }

    #[test]
    fn session_mode_set_and_clear() {
        let sid = "test_session_mode_123";
        assert!(session_mode(sid).is_none());
        set_session_mode(sid, Mode::Plan);
        assert_eq!(session_mode(sid), Some(Mode::Plan));
        clear_session_mode(sid);
        assert!(session_mode(sid).is_none());
    }

    #[test]
    fn cycle_mode_cycles_through_all_modes() {
        let original = current_mode();
        // Start from a known state
        set_mode(Mode::Default);
        assert_eq!(cycle_mode(), Mode::AcceptEdits);
        assert_eq!(cycle_mode(), Mode::Plan);
        assert_eq!(cycle_mode(), Mode::Auto);
        assert_eq!(cycle_mode(), Mode::DontAsk);
        assert_eq!(cycle_mode(), Mode::BypassPermissions);
        assert_eq!(cycle_mode(), Mode::Default);
        // Restore
        set_mode(original);
    }

    #[test]
    fn mode_to_str_returns_lowercase_kebab() {
        assert_eq!(mode_to_str(Mode::Default), "default");
        assert_eq!(mode_to_str(Mode::AcceptEdits), "accept-edits");
        assert_eq!(mode_to_str(Mode::Plan), "plan");
        assert_eq!(mode_to_str(Mode::Auto), "auto");
        assert_eq!(mode_to_str(Mode::DontAsk), "dont-ask");
        assert_eq!(mode_to_str(Mode::BypassPermissions), "bypass-permissions");
    }

    #[test]
    fn set_mode_from_str_accepts_valid_modes() {
        assert!(set_mode_from_str("default"));
        assert!(set_mode_from_str("accept-edits"));
        assert!(set_mode_from_str("plan"));
        assert!(set_mode_from_str("auto"));
        assert!(set_mode_from_str("dont-ask"));
        assert!(set_mode_from_str("bypass-permissions"));
        assert!(set_mode_from_str("Default"));
        assert!(set_mode_from_str("BYPASS-PERMISSIONS"));
        // Invalid strings are rejected
        assert!(!set_mode_from_str(""));
        assert!(!set_mode_from_str("nonsense"));
        assert!(!set_mode_from_str("accept_edits"));
    }

    #[test]
    fn consume_allow_once_rejects_invalid_code() {
        // An empty or garbage code should not be consumable
        assert!(!consume_allow_once(""));
        assert!(!consume_allow_once("zzzzzz"));
    }

    #[test]
    fn accept_edits_mode_allows_write_shaped_tools() {
        // In AcceptEdits mode, write-shaped tools (todowrite, memory) allow
        assert_eq!(
            classify_with_mode("todowrite", Mode::AcceptEdits),
            BridgeDecision::Allow,
            "todowrite must allow in AcceptEdits"
        );
        assert_eq!(
            classify_with_mode("memory", Mode::AcceptEdits),
            BridgeDecision::Allow,
            "memory must allow in AcceptEdits"
        );
    }

    #[test]
    fn deny_carries_reason() {
        match classify_with_mode("todowrite", Mode::Plan) {
            BridgeDecision::Deny { reason, .. } => {
                assert!(!reason.is_empty(), "Deny must carry a reason");
            }
            other => panic!("expected Deny, got {:?}", other),
        }
    }

    #[test]
    fn prompt_carries_reason() {
        match classify_with_mode("bash", Mode::Default) {
            BridgeDecision::Prompt { reason, .. } => {
                assert!(!reason.is_empty(), "Prompt must carry a reason");
            }
            other => panic!("expected Prompt, got {:?}", other),
        }
    }

    #[test]
    fn is_valid_allow_once_code_rejects_bad_shape() {
        // Too short
        assert!(!is_valid_allow_once_code("ab"));
        // Too long
        assert!(!is_valid_allow_once_code("abcdef0"));
        // Not hex
        assert!(!is_valid_allow_once_code("abcxyz"));
        // Empty
        assert!(!is_valid_allow_once_code(""));
        // Valid
        assert!(is_valid_allow_once_code("a1b2c3"));
        assert!(is_valid_allow_once_code("000000"));
        assert!(is_valid_allow_once_code("ffff00"));
    }

    #[test]
    fn session_approve_action_allows_subsequent_calls() {
        let sid = "session-approve-test";
        // Start fresh with no pre-existing allow-list state for this sid
        clear_session_mode(sid);

        // Initially, classify_for_session should prompt (in Default mode for
        // a non-legacy action) — use classify_with_mode with an explicit mode
        // so concurrent test mutation of GLOBAL_MODE doesn't flake.
        let result_before = classify_with_mode("make_cappuccino", Mode::Default);
        assert!(
            matches!(&result_before, BridgeDecision::Prompt { .. }),
            "unknown action should prompt before approval in Default: {result_before:?}"
        );

        // Approve the action for this session
        approve_session_action(sid, "make_cappuccino");
        let result_after = classify_for_session("make_cappuccino", sid);
        assert_eq!(
            result_after,
            BridgeDecision::Allow,
            "approved action should allow for the session"
        );

        // A different action on the same session should still prompt.
        // Use classify_with_mode with explicit Default mode because
        // classify_for_session reads GLOBAL_MODE which a concurrent
        // test may have changed.
        let result_other = classify_with_mode("make_espresso", Mode::Default);
        assert!(
            matches!(&result_other, BridgeDecision::Prompt { .. }),
            "different action should still prompt: {result_other:?}"
        );

        // Clean up the session state (also clears SESSION_ALLOWED_ACTIONS)
        clear_session_mode(sid);
    }

    #[test]
    fn session_approve_all_wildcard_allows_everything() {
        let sid = "session-approve-all-test";
        let original_mode = current_mode();
        set_mode(Mode::Default);

        // Approve every tool for this session
        approve_session_all(sid);
        assert_eq!(
            classify_for_session("anything_123", sid),
            BridgeDecision::Allow,
            "wildcard should allow everything"
        );
        assert_eq!(
            classify_for_session("bash", sid),
            BridgeDecision::Allow,
            "wildcard should even allow bash"
        );

        // Restore
        set_mode(original_mode);
        clear_session_mode(sid);
    }

    #[test]
    fn consume_allow_once_validates_shape_before_hash() {
        // The function should reject non-6-hex strings without touching
        // the session (no panic, no hang).
        assert!(
            !consume_allow_once("nothex!"),
            "non-hex must be rejected before hash"
        );
        assert!(!consume_allow_once(""), "empty must be rejected");
        assert!(
            !consume_allow_once(&"a".repeat(100_000)),
            "long string must be rejected before hash"
        );
    }
}

/// Evaluate a command string against the execution policy engine, then
/// fall through to dcg-core's mode evaluation if no policy rule matched.
///
/// This is the **Stage 2** permission gate, called from inside tool
/// execution (e.g. `BashTool::execute()`) where the real command string
/// is available. It complements the Stage 1 tool-name gate in
/// `validate_tool_allowed()`.
///
/// # Precedence
///
/// 1. **Session allow-list** — if the tool was approved for the session
///    (via TUI "y" key), returns `Allow` immediately, bypassing all
///    further checks. This means session-level approval always beats
///    command-level rules. For command-level enforcement, use
///    allow-once codes instead of session approval.
/// 2. **Custom policy rules** — TOML-configured allow/deny/prompt patterns.
/// 3. **dcg-core fallthrough** — passes `ToolCall::Bash { cmd }` to
///    dcg-core's built-in safe whitelist and dangerous patterns.
/// 4. **Mode fallback** — returns `Allow`/`Deny`/`Prompt` based on the
///    current permission mode.
///
/// # Flow
///
/// 1. Session allow-list check (short-circuit)
/// 2. ExecutionPolicyEngine (custom TOML rules)
/// 3. dcg-core with real command string (safe whitelist + dangerous patterns)
/// 4. Mode-based fallthrough (Allow / Deny / Prompt)
#[must_use]
pub fn classify_command(tool_name: &str, command: &str, session_id: &str) -> BridgeDecision {
    // Short-circuit: session allow-list already approved this tool
    if session_allows_action(session_id, tool_name) {
        return BridgeDecision::Allow;
    }

    // Stage 2a: Check execution policy engine (custom rules)
    if let Some(decision) = crate::execution_policy::evaluate_command(tool_name, command) {
        return match decision {
            crate::execution_policy::PolicyDecision::Allow { reason } => BridgeDecision::Allow,
            crate::execution_policy::PolicyDecision::Deny {
                reason,
                alternatives,
                ..
            } => BridgeDecision::Deny {
                reason,
                alternatives,
            },
            crate::execution_policy::PolicyDecision::Prompt {
                reason,
                allow_once_code,
                alternatives,
            } => BridgeDecision::Prompt {
                reason,
                allow_once_code,
                alternatives,
            },
        };
    }

    // Stage 2b: Fall through to dcg-core with the real command string.
    // This enables the built-in SafeCommandWhitelist and
    // DangerousPatternRegistry to evaluate the actual command.
    let mode = session_mode(session_id).unwrap_or_else(current_mode);

    // Legacy auto-allow gating (same as classify_with_mode for Default/Auto)
    if matches!(mode, Mode::Default | Mode::Auto) {
        if is_legacy_auto_allowed(tool_name) {
            return BridgeDecision::Allow;
        }
        if mode == Mode::Auto {
            // YOLO classifier handles Mode::Auto
            let classifier = crate::yolo_classifier::YoloClassifier::get_or_init();
            let (_, effects) = action_to_tool_call(tool_name);
            let effect_strings: Vec<String> = effects.iter().map(|e| format!("{:?}", e)).collect();
            return classifier.evaluate(tool_name, tool_name, &effect_strings);
        }
        // Mode::Default non-legacy: prompt
        return BridgeDecision::Prompt {
            reason: format!(
                "Tool '{}' requires permission in current mode {:?}",
                tool_name,
                current_mode()
            ),
            allow_once_code: String::new(),
            alternatives: vec![],
        };
    }

    // Build ToolCall with the REAL command string instead of placeholder.
    // This is the key change: dcg-core's SafeCommandWhitelist and
    // DangerousPatternRegistry will now see the actual command.
    let (base_tool, effects) = action_to_tool_call(tool_name);
    let tool = match base_tool {
        ToolCall::Bash { .. } => ToolCall::bash(command),
        _ => base_tool,
    };

    let decision = match SESSION.lock() {
        Ok(mut session) => ENGINE.evaluate(&mut session, &tool, mode, &effects),
        Err(_) => {
            return BridgeDecision::Prompt {
                reason: "Session poisoned".into(),
                allow_once_code: String::new(),
                alternatives: vec![],
            };
        }
    };

    match decision {
        Decision::Allow => BridgeDecision::Allow,
        Decision::Prompt {
            reason,
            allow_once_code,
            alternatives,
        } => BridgeDecision::Prompt {
            reason,
            allow_once_code,
            alternatives,
        },
        Decision::Deny {
            reason,
            alternatives,
        } => BridgeDecision::Deny {
            reason,
            alternatives,
        },
    }
}

/// Initialize the session allow-list from the config's `always_allow_tools` list.
/// Called at session start so tools the user previously marked as always-allow
/// are pre-approved without re-prompting.
pub fn init_session_allow_list(session_id: &str) {
    let config = crate::config::config();
    for tool in &config.always_allow_tools {
        approve_session_action(session_id, tool);
    }
}
