mod agentgrep;
pub mod ambient;
mod apply_patch;
mod bash;
mod batch;
mod beads;
mod best_of_n;
mod bg;
mod browser;
mod codesearch;
mod communicate;
#[cfg(target_os = "macos")]
mod computer;
mod conversation_search;
#[cfg(feature = "dcp")]
mod dcp_compress;
mod debug_socket;
mod edit;
mod ffs_engine_tools;
mod ffs_glob;
mod ffs_grep;
mod ffs_multi_grep;
mod ffs_outline;
mod ffs_support;
mod ffs_symbol;
mod gmail;

mod goal;
mod hashline_edit;
mod invalid;
mod ls;
mod lsp;
pub mod mcp;
mod memory;
mod multiedit;
mod notepad;
mod open;
mod patch;
mod propose_edit;
mod propose_hashline_edit;
mod propose_write;
mod read;
pub mod selfdev;
pub(crate) mod serde_coerce;
mod session_search;
mod side_panel;
mod skill;
mod task;
mod team;
mod todo;
mod webfetch;
mod websearch;
mod write;

use crate::compaction::CompactionManager;
use crate::provider::Provider;
use crate::skill::SkillRegistry;
use anyhow::Result;
use jcode_hooks::{
    DispatchConfig, HookContext, HookEvent, HookInputBuilder, HookRegistry,
    legacy_v1_to_v2_handlers,
};
use jcode_message_types::ToolDefinition;
use jcode_plugin_core::ToolTier;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
#[cfg(feature = "dcp")]
use std::sync::Mutex;

use std::sync::{LazyLock, RwLock as StdRwLock};
use tokio::sync::RwLock;

pub(crate) use jcode_tool_core::intent_schema_property;
pub use jcode_tool_core::{StdinInputRequest, Tool, ToolContext, ToolExecutionMode};
pub use jcode_tool_types::{ToolImage, ToolOutput};
pub(crate) use session_search::spawn_recent_index_warmup;

#[derive(Clone, Debug, Default)]
struct SessionToolPolicy {
    allowed_tools: Option<HashSet<String>>,
    disabled_tools: HashSet<String>,
}

static SESSION_TOOL_POLICIES: LazyLock<StdRwLock<HashMap<String, SessionToolPolicy>>> =
    LazyLock::new(|| StdRwLock::new(HashMap::new()));

pub(crate) fn set_session_tool_policy(
    session_id: &str,
    allowed_tools: Option<HashSet<String>>,
    disabled_tools: HashSet<String>,
) {
    let mut policies = SESSION_TOOL_POLICIES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    policies.insert(
        session_id.to_string(),
        SessionToolPolicy {
            allowed_tools,
            disabled_tools,
        },
    );
}

pub(crate) fn clear_session_tool_policy(session_id: &str) {
    let mut policies = SESSION_TOOL_POLICIES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    policies.remove(session_id);
}

fn session_tool_policy(session_id: &str) -> Option<SessionToolPolicy> {
    SESSION_TOOL_POLICIES
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(session_id)
        .cloned()
}

/// Global handle to the active best-of-N orchestrator's
/// `BestOfNOrchestratorHandle`, registered by `Agent::new_with_session`
/// while a best-of-N run is in flight.
///
/// `propose_edit` / `propose_write` are registered as stateless base tools,
/// so they have no constructor-time access to the Registry. They look up
/// the store through this static instead. The handle is `None` outside of
/// best-of-N runs and the propose tools must refuse to execute in that case.
///
/// Uses `StdRwLock` (not `OnceLock`) so the handle is updated on each run
/// rather than being permanently stuck on the first run's values.
static BEST_OF_N_HANDLE: StdRwLock<Option<BestOfNOrchestratorHandle>> = StdRwLock::new(None);

/// Install the global best-of-N handle. Called by the agent when a
/// best-of-N run starts. Updates on every call so subsequent runs
/// overwrite the stale handle.
pub fn set_best_of_n_handle(handle: BestOfNOrchestratorHandle) {
    if let Ok(mut guard) = BEST_OF_N_HANDLE.write() {
        *guard = Some(handle);
    }
}

/// Borrow the global best-of-N handle, if one has been installed.
pub fn get_best_of_n_handle() -> Option<BestOfNOrchestratorHandle> {
    BEST_OF_N_HANDLE.read().ok()?.clone()
}

/// Clear the global best-of-N handle. Called after a run completes
/// (both auto-apply and show-mode paths) so stale state does not leak
/// across unrelated agent turns.
pub fn clear_best_of_n_handle() {
    if let Ok(mut guard) = BEST_OF_N_HANDLE.write() {
        *guard = None;
    }
}

// ---------------------------------------------------------------------------
// ApprovalGate dispatcher bridge
// ---------------------------------------------------------------------------

/// Global handle to the plugin system's [`RcuDispatcher`] so the tool execution
/// path can consult the ApprovalGate before running a tool.
///
/// Set once during plugin system initialisation. The handle is
/// `Option<Arc<...>>` so that tools still work when the plugin system is
/// disabled or not yet initialised — the gate check simply passes through.
static GATE_DISPATCHER: StdRwLock<Option<std::sync::Arc<jcode_plugin_runtime::RcuDispatcher>>> =
    StdRwLock::new(None);

/// Install the global gate dispatcher. Called once during plugin system
/// initialisation. Replacing it at runtime is safe (the old handle is dropped
/// once the write lock is released).
pub fn set_gate_dispatcher(dispatcher: std::sync::Arc<jcode_plugin_runtime::RcuDispatcher>) {
    if let Ok(mut guard) = GATE_DISPATCHER.write() {
        *guard = Some(dispatcher);
    }
}

/// Map a tool name to its [`ToolTier`] for the approval gate.
///
/// This is a built-in classification based on the tool's known behaviour.
/// Plugin-registered tools default to `Exec` (the most restrictive tier).
fn tool_name_to_tier(name: &str) -> ToolTier {
    // Read-only introspection tools.
    if matches!(
        name,
        "read"
            | "ls"
            | "grep"
            | "ffs_glob"
            | "ffs_grep"
            | "ffs_outline"
            | "ffs_symbol"
            | "ffs_multi_grep"
            | "ffs_find"
            | "ffs_dispatch"
            | "ffs_callers"
            | "ffs_callees"
            | "ffs_refs"
            | "ffs_flow"
            | "agentgrep"
            | "websearch"
            | "webfetch"
            | "codesearch"
            | "session_search"
            | "notepad_read_priority"
            | "notepad_read_working"
            | "notepad_read_manual"
            | "notepad_stats"
            | "beads_list"
            | "memory"
            | "lsp"
            | "conversation_search"
            | "skill_manage"
            | "team_status"
            | "team_task_list"
    ) {
        return ToolTier::Read;
    }
    // Write tools that mutate workspace or session state.
    if matches!(
        name,
        "write"
            | "edit"
            | "multiedit"
            | "patch"
            | "apply_patch"
            | "hashline_edit"
            | "ffs_hashline_edit"
            | "propose_write"
            | "propose_edit"
            | "ffs_propose_hashline"
            | "notepad_write_priority"
            | "notepad_write_working"
            | "notepad_write_manual"
            | "beads_create"
            | "beads_ready"
            | "beads_claim"
            | "beads_close"
            | "beads_dep"
            | "batch"
            | "bg"
            | "todo"
            | "mcp"
            | "team_create"
            | "team_delete"
            | "team_send_message"
            | "team_task_create"
            | "team_task_claim"
            | "team_shutdown"
    ) {
        return ToolTier::Write;
    }
    // Everything else is Exec (most restrictive).
    ToolTier::Exec
}

/// Run the plugin-system approval gate check before executing a tool.
///
/// Returns `Ok(())` if the gate allows or no gate is installed.
/// Returns `Err` with a descriptive message if the gate denies the call.
pub(crate) fn check_approval_gate(tool_name: &str, input: &Value) -> Result<()> {
    let Ok(guard) = GATE_DISPATCHER.read() else {
        return Ok(());
    };
    let Some(dispatcher) = guard.as_ref() else {
        return Ok(());
    };
    let tier = tool_name_to_tier(tool_name);
    match dispatcher.check_tool(tool_name, tier, input) {
        None | Some(jcode_plugin_runtime::gate::GateDecision::Allow) => Ok(()),
        Some(jcode_plugin_runtime::gate::GateDecision::Deny { reason, layer }) => {
            Err(anyhow::anyhow!(
                "Tool '{}' blocked by approval gate: {} ({})",
                tool_name,
                reason,
                layer
            ))
        }
        Some(jcode_plugin_runtime::gate::GateDecision::NeedsApproval { prompt }) => Err(
            anyhow::anyhow!("Tool '{}' requires approval: {}", tool_name, prompt.reason),
        ),
    }
}

/// Registry of available tools (Arc-wrapped for sharing)
///
/// Clone creates a fresh CompactionManager so each subagent gets independent
/// message history tracking. Tools and skills are shared via Arc.
pub struct Registry {
    tools: Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>,
    skills: Arc<RwLock<SkillRegistry>>,
    compaction: Arc<RwLock<CompactionManager>>,
    /// Hook system for lifecycle events (PreToolUse, PostToolUse, etc.)
    hook_registry: Arc<RwLock<HookRegistry>>,
    /// Dispatch configuration for hooks
    dispatch_config: DispatchConfig,
    /// Best-of-N orchestrator handle, set during Agent::new_with_session.
    pub best_of_n: Arc<RwLock<Option<BestOfNOrchestratorHandle>>>,
    #[cfg(feature = "dcp")]
    dcp: Option<Arc<Mutex<crate::dcp_plugin::DcpPlugin>>>,
}

/// Handle to the best-of-N orchestrator, stored on the Registry
/// for access by propose_edit/propose_write tools.
#[derive(Clone)]
pub struct BestOfNOrchestratorHandle {
    /// Run ID for the current best-of-N cycle.
    pub run_id: String,
    /// Candidate ID for the current subagent.
    pub candidate_id: String,
    /// Best-of-N config (mode, count, temperatures).
    pub config: jcode_best_of_n::BestOfNConfig,
    /// Shared proposed content store.
    pub store: std::sync::Arc<jcode_best_of_n::ProposedContentStore>,
}

impl Clone for Registry {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
            skills: self.skills.clone(),
            // Each clone gets a fresh CompactionManager to prevent parallel
            // subagents from corrupting each other's message history
            compaction: Arc::new(RwLock::new(CompactionManager::new())),
            hook_registry: self.hook_registry.clone(),
            dispatch_config: self.dispatch_config.clone(),
            best_of_n: self.best_of_n.clone(),
            #[cfg(feature = "dcp")]
            dcp: self.dcp.clone(),
        }
    }
}

impl Registry {
    /// Access the hook registry for dispatching lifecycle hooks.
    pub fn hook_registry(&self) -> &Arc<RwLock<HookRegistry>> {
        &self.hook_registry
    }

    /// Access the dispatch configuration for hooks.
    pub fn dispatch_config(&self) -> &DispatchConfig {
        &self.dispatch_config
    }

    /// Install the DCP plugin (dynamic context pruning) so DCP-aware tools can
    /// access it. Only available when the crate is built with the `dcp` feature.
    #[cfg(feature = "dcp")]
    pub fn set_dcp(&mut self, plugin: crate::dcp_plugin::DcpPlugin) {
        self.dcp = Some(Arc::new(Mutex::new(plugin)));
    }

    /// Access the DCP plugin if one has been installed.
    #[cfg(feature = "dcp")]
    pub fn dcp(&self) -> Option<&Arc<Mutex<crate::dcp_plugin::DcpPlugin>>> {
        self.dcp.as_ref()
    }

    fn shared_skills_registry() -> Arc<RwLock<SkillRegistry>> {
        SkillRegistry::shared_registry()
    }

    fn insert_tool<T>(tools: &mut HashMap<String, Arc<dyn Tool>>, name: &str, tool: T)
    where
        T: Tool + 'static,
    {
        tools.insert(name.into(), Arc::new(tool) as Arc<dyn Tool>);
    }

    fn insert_tool_timed<T>(
        tools: &mut HashMap<String, Arc<dyn Tool>>,
        timings: &mut Vec<(String, u128)>,
        name: &str,
        make_tool: impl FnOnce() -> T,
    ) where
        T: Tool + 'static,
    {
        let start = std::time::Instant::now();
        Self::insert_tool(tools, name, make_tool());
        timings.push((name.to_string(), start.elapsed().as_millis()));
    }

    /// Create a lightweight empty registry (no tools, no skill loading).
    /// Used by remote-mode clients that don't execute tools locally.
    pub fn empty() -> Self {
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
            skills: Arc::new(RwLock::new(SkillRegistry::default())),
            compaction: Arc::new(RwLock::new(CompactionManager::new())),
            hook_registry: Arc::new(RwLock::new(HookRegistry::default())),
            dispatch_config: DispatchConfig::default(),
            best_of_n: Arc::new(RwLock::new(None)),
            #[cfg(feature = "dcp")]
            dcp: None,
        }
    }

    /// Base tools that are stateless and can be shared across sessions.
    /// Created once and cached in a OnceLock, then cloned (cheap Arc bumps) per session.
    fn base_tools(skills: &Arc<RwLock<SkillRegistry>>) -> HashMap<String, Arc<dyn Tool>> {
        use std::sync::OnceLock;
        static BASE: OnceLock<HashMap<String, Arc<dyn Tool>>> = OnceLock::new();
        let base = BASE.get_or_init(|| {
            let init_start = std::time::Instant::now();
            let mut timings = Vec::new();
            let mut m = HashMap::new();
            Self::insert_tool_timed(&mut m, &mut timings, "read", read::ReadTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "write", write::WriteTool::new);
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "propose_write",
                propose_write::ProposeWriteTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "agentgrep",
                agentgrep::AgentGrepTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "side_panel",
                side_panel::SidePanelTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "edit", edit::EditTool::new);
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "propose_edit",
                propose_edit::ProposeEditTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_propose_hashline",
                propose_hashline_edit::ProposeHashlineEditTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "multiedit",
                multiedit::MultiEditTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "patch", patch::PatchTool::new);
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "apply_patch",
                apply_patch::ApplyPatchTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "ffs_glob", ffs_glob::FfsGlobTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "ffs_grep", ffs_grep::FfsGrepTool::new);
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_outline",
                ffs_outline::FfsOutlineTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_symbol",
                ffs_symbol::FfsSymbolTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_multi_grep",
                ffs_multi_grep::FfsMultiGrepTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_find",
                ffs_engine_tools::FfsFindTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_dispatch",
                ffs_engine_tools::FfsDispatchTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_callers",
                ffs_engine_tools::FfsCallersTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_callees",
                ffs_engine_tools::FfsCalleesTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_refs",
                ffs_engine_tools::FfsRefsTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_flow",
                ffs_engine_tools::FfsFlowTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "ls", ls::LsTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "bash", bash::BashTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "browser", browser::BrowserTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "open", open::OpenTool::new);
            #[cfg(target_os = "macos")]
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "macos_computer_use",
                computer::ComputerTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "webfetch",
                webfetch::WebFetchTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "websearch",
                websearch::WebSearchTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "codesearch",
                codesearch::CodeSearchTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "invalid", invalid::InvalidTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "lsp", lsp::LspTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "todo", todo::TodoTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "bg", bg::BgTool::new);
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "swarm",
                communicate::CommunicateTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "session_search",
                session_search::SessionSearchTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "initiative",
                goal::InitiativeTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "beads_list",
                beads::BeadsListTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "beads_create",
                beads::BeadsCreateTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "beads_ready",
                beads::BeadsReadyTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "beads_claim",
                beads::BeadsClaimTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "beads_close",
                beads::BeadsCloseTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "beads_dep", beads::BeadsDepTool::new);
            Self::insert_tool_timed(&mut m, &mut timings, "gmail", gmail::GmailTool::new);
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "ffs_hashline_edit",
                hashline_edit::HashlineEditTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "schedule", ambient::ScheduleTool::new);
            #[cfg(feature = "dcp")]
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "dcp_compress",
                dcp_compress::DcpCompressTool::new,
            );
            Self::insert_tool_timed(&mut m, &mut timings, "selfdev", selfdev::SelfDevTool::new);
            // Notepad tools (compaction-resistant notes)
            // Names are namespaced (`notepad_*`) to avoid collision
            // with future built-in or MCP tools.
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_read_priority",
                notepad::NotepadTool::read_priority,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_write_priority",
                notepad::NotepadTool::write_priority,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_read_working",
                notepad::NotepadTool::read_working,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_write_working",
                notepad::NotepadTool::write_working,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_read_manual",
                notepad::NotepadTool::read_manual,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_write_manual",
                notepad::NotepadTool::write_manual,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_prune",
                notepad::NotepadPruneTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "notepad_stats",
                notepad::NotepadStatsTool::new,
            );
            // Team tools (multi-agent orchestration)
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_create",
                team::TeamCreateTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_delete",
                team::TeamDeleteTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_status",
                team::TeamStatusTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_send_message",
                team::TeamSendMessageTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_task_create",
                team::TeamTaskCreateTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_task_claim",
                team::TeamTaskClaimTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_task_list",
                team::TeamTaskListTool::new,
            );
            Self::insert_tool_timed(
                &mut m,
                &mut timings,
                "team_shutdown",
                team::TeamShutdownTool::new,
            );
            let nonzero: Vec<String> = timings
                .iter()
                .filter(|(_, ms)| *ms > 0)
                .map(|(name, ms)| format!("{name}={ms}ms"))
                .collect();
            crate::logging::info(&format!(
                "[TIMING] registry_base_tools_init: total={}ms, nonzero=[{}]",
                init_start.elapsed().as_millis(),
                nonzero.join(", ")
            ));
            m
        });
        // Clone the Arc entries (cheap refcount bumps, not deep copies)
        let mut tools = base.clone();
        // SkillTool needs the skills registry reference (shared across sessions)
        Self::insert_tool(
            &mut tools,
            "skill_manage",
            skill::SkillTool::new(skills.clone()),
        );
        tools
    }

    pub async fn new(provider: Arc<dyn Provider>) -> Self {
        let start = std::time::Instant::now();
        let skills_start = std::time::Instant::now();
        let skills = Self::shared_skills_registry();
        let skills_ms = skills_start.elapsed().as_millis();
        let compaction_start = std::time::Instant::now();
        let compaction = Arc::new(RwLock::new(CompactionManager::new()));
        let compaction_ms = compaction_start.elapsed().as_millis();
        let registry_struct_start = std::time::Instant::now();
        // Load v2 hooks config (.jcode/hooks.toml, $JCODE_HOOKS_CONFIG, etc.)
        let mut hook_config = jcode_hooks::load_hooks_config();
        // Merge v1 legacy config.toml [hooks] entries into the v2 config so
        // existing single-line hooks (`pre_tool = "check.sh"`) continue working
        // alongside the richer v2 TOML format without dual-firing.
        let v1_hooks = &crate::config::config().hooks;
        let v2_entries = legacy_v1_to_v2_handlers(
            v1_hooks.turn_end.clone(),
            v1_hooks.session_start.clone(),
            v1_hooks.session_end.clone(),
            v1_hooks.pre_tool.clone(),
            Some(v1_hooks.pre_tool_timeout_ms),
            v1_hooks.post_tool.clone(),
        );
        for (event_name, handlers) in v2_entries {
            hook_config
                .events
                .entry(event_name)
                .or_default()
                .extend(handlers);
        }
        let hook_registry = Arc::new(RwLock::new(HookRegistry::from_config(hook_config.clone())));
        let dispatch_config = DispatchConfig::from_settings(&hook_config.settings);
        let registry = Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
            skills: skills.clone(),
            compaction: compaction.clone(),
            hook_registry,
            dispatch_config,
            best_of_n: Arc::new(RwLock::new(None)),
            #[cfg(feature = "dcp")]
            dcp: None,
        };
        let registry_struct_ms = registry_struct_start.elapsed().as_millis();

        let base_start = std::time::Instant::now();
        let mut tools_map = Self::base_tools(&skills);
        let base_ms = base_start.elapsed().as_millis();

        // Per-session tools that need provider/registry references
        let session_tools_start = std::time::Instant::now();
        Self::insert_tool(
            &mut tools_map,
            "subagent",
            task::SubagentTool::new(provider.clone(), registry.clone()),
        );
        Self::insert_tool(
            &mut tools_map,
            "best_of_n",
            best_of_n::BestOfNTool::new(provider, registry.clone()),
        );
        Self::insert_tool(
            &mut tools_map,
            "batch",
            batch::BatchTool::new(registry.clone()),
        );
        Self::insert_tool(
            &mut tools_map,
            "conversation_search",
            conversation_search::ConversationSearchTool::new(compaction),
        );
        // Memory tool — uses MemoryProvider abstraction.
        // Default: legacy MemoryManager. When mempalace-backend feature is
        // enabled, tries to create a MempalaceAdapter first, falling back
        // to the legacy manager if the adapter fails to open.
        {
            let memory_provider: Arc<dyn jcode_memory_types::MemoryProvider> =
                Self::create_memory_provider().await;
            Self::insert_tool(
                &mut tools_map,
                "memory",
                memory::MemoryTool::new(memory_provider),
            );
        }
        let session_tools_ms = session_tools_start.elapsed().as_millis();

        let write_start = std::time::Instant::now();
        *registry.tools.write().await = tools_map;
        let write_ms = write_start.elapsed().as_millis();
        crate::logging::info(&format!(
            "[TIMING] registry_new: skills={}ms, compaction={}ms, registry_struct={}ms, base_tools={}ms, session_tools={}ms, write={}ms, total={}ms",
            skills_ms,
            compaction_ms,
            registry_struct_ms,
            base_ms,
            session_tools_ms,
            write_ms,
            start.elapsed().as_millis()
        ));
        registry
    }

    /// Get all tool definitions for the API
    pub async fn definitions(
        &self,
        allowed_tools: Option<&HashSet<String>>,
    ) -> Vec<ToolDefinition> {
        let tools = self.tools.read().await;
        let mut defs: Vec<ToolDefinition> = tools
            .iter()
            .filter(|(name, _)| allowed_tools.map(|set| set.contains(*name)).unwrap_or(true))
            .map(|(name, tool)| {
                let mut def = tool.to_definition();
                // Use registry key as the tool name (important for MCP tools where
                // the registry key is "mcp__server__tool" but Tool::name() returns
                // just the raw tool name)
                if def.name != *name {
                    def.name = name.clone();
                }
                def
            })
            .collect();

        // Sort by name for deterministic ordering - critical for prompt cache hits
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }

    pub async fn tool_names(&self) -> Vec<String> {
        let tools = self.tools.read().await;
        tools.keys().cloned().collect()
    }

    /// Enable test mode for memory tools (isolated storage)
    /// Called when session is marked as debug
    pub async fn enable_memory_test_mode(&self) {
        let mut tools = self.tools.write().await;

        // Replace memory tool with test version
        tools.insert(
            "memory".to_string(),
            Arc::new(memory::MemoryTool::new_test()) as Arc<dyn Tool>,
        );

        crate::logging::info("Memory test mode enabled - using isolated storage");
    }

    /// Create a memory provider, trying MempalaceAdapter first when feature
    /// `mempalace-backend` is enabled, falling back to legacy MemoryManager.
    async fn create_memory_provider() -> Arc<dyn jcode_memory_types::MemoryProvider> {
        #[cfg(feature = "mempalace-backend")]
        {
            match Self::open_mempalace().await {
                Ok(provider) => {
                    crate::logging::info("Using mempalace memory backend");
                    return provider;
                }
                Err(e) => {
                    crate::logging::warn(&format!("Failed to open mempalace, using legacy: {e}"));
                }
            }
        }
        Arc::new(crate::memory::MemoryManager::new())
    }

    /// Try to open a MempalaceAdapter at the default palace path.
    #[cfg(feature = "mempalace-backend")]
    async fn open_mempalace() -> anyhow::Result<Arc<dyn jcode_memory_types::MemoryProvider>> {
        use jcode_mempalace_adapter::MempalaceAdapter;

        let jcode_home = jcode_storage::jcode_dir()?;
        let palace_path = jcode_home.join("mempalace");

        let adapter = MempalaceAdapter::open(&palace_path).await?;
        Ok(Arc::new(adapter) as Arc<dyn jcode_memory_types::MemoryProvider>)
    }

    /// Resolve tool name aliases.
    ///
    /// When using OAuth, the API presents tools with Claude Code names
    /// (e.g. `file_grep`, `shell_exec`). The model uses those names in
    /// sub-tool calls (e.g. inside `batch`), but our registry uses internal
    /// names (`grep`, `bash`). This mapping ensures both forms resolve
    /// correctly.
    ///
    /// The canonical mapping lives in `jcode-tool-types::resolve_tool_name` so
    /// lower-level crates (e.g. config) can normalize tool names without
    /// depending on the tool subsystem; this method delegates to it.
    pub(crate) fn resolve_tool_name(name: &str) -> &str {
        jcode_tool_types::resolve_tool_name(name)
    }

    /// Suggest up to 3 available tool names that look similar to `name`.
    /// Uses cheap, dependency-free heuristics: case-insensitive equality,
    /// prefix/substring containment, then bounded edit distance. Helps the
    /// model recover from hallucinated tool names (#104).
    fn closest_tool_names(name: &str, available: &[&str]) -> Vec<String> {
        let needle = name.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(usize, &str)> = available
            .iter()
            .filter_map(|candidate| {
                let hay = candidate.to_ascii_lowercase();
                let score = if hay == needle {
                    0
                } else if hay.starts_with(&needle) || needle.starts_with(&hay) {
                    1
                } else if hay.contains(&needle) || needle.contains(&hay) {
                    2
                } else {
                    let dist = levenshtein(&needle, &hay);
                    // Only suggest near-misses, scaled to the longer name.
                    let threshold = (hay.len().max(needle.len()) / 3).max(2);
                    if dist <= threshold {
                        3 + dist
                    } else {
                        return None;
                    }
                };
                Some((score, *candidate))
            })
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        scored
            .into_iter()
            .take(3)
            .map(|(_, name)| name.to_string())
            .collect()
    }

    /// Estimate token count for a string (chars / 4, matching compaction heuristic)
    fn estimate_tokens(s: &str) -> usize {
        crate::util::estimate_tokens(s)
    }

    fn tool_lifecycle_fields(
        phase: &str,
        requested_name: &str,
        resolved_name: &str,
        input: &Value,
        ctx: &ToolContext,
    ) -> Vec<(String, String)> {
        let cwd = ctx
            .working_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string());
        let input_json = serde_json::to_string(input).unwrap_or_default();
        let mut fields = vec![
            ("phase".to_string(), phase.to_string()),
            ("tool_name".to_string(), requested_name.to_string()),
            ("resolved_tool_name".to_string(), resolved_name.to_string()),
            ("session_id".to_string(), ctx.session_id.clone()),
            ("message_id".to_string(), ctx.message_id.clone()),
            ("tool_call_id".to_string(), ctx.tool_call_id.clone()),
            (
                "execution_mode".to_string(),
                format!("{:?}", ctx.execution_mode),
            ),
            ("cwd".to_string(), cwd),
            ("input_json_bytes".to_string(), input_json.len().to_string()),
        ];

        if let Some(object) = input.as_object() {
            let mut keys = object.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            fields.push(("input_keys".to_string(), keys.join(",")));

            let path_fields = [
                "file_path",
                "path",
                "target",
                "target_path",
                "old_path",
                "new_path",
            ];
            let mut touched_paths = Vec::new();
            for key in path_fields {
                if let Some(path) = object.get(key).and_then(Value::as_str) {
                    touched_paths.push(format!(
                        "{key}:{}",
                        ctx.resolve_path(std::path::Path::new(path)).display()
                    ));
                }
            }
            if let Some(paths) = object.get("paths").and_then(Value::as_array) {
                for path in paths.iter().filter_map(Value::as_str).take(8) {
                    touched_paths.push(format!(
                        "paths:{}",
                        ctx.resolve_path(std::path::Path::new(path)).display()
                    ));
                }
            }
            if !touched_paths.is_empty() {
                fields.push(("touched_paths".to_string(), touched_paths.join(",")));
                fields.push((
                    "touched_path_count".to_string(),
                    touched_paths.len().to_string(),
                ));
            }

            for text_key in ["command", "prompt", "task", "query", "content"] {
                if let Some(text) = object.get(text_key).and_then(Value::as_str) {
                    fields.push((format!("{text_key}_bytes"), text.len().to_string()));
                    fields.push((
                        format!("{text_key}_chars"),
                        text.chars().count().to_string(),
                    ));
                }
            }
        }

        fields
    }

    /// Maximum fraction of context budget a single tool output may consume.
    /// Outputs that would push total context beyond this are truncated.
    const CONTEXT_GUARD_THRESHOLD: f32 = 0.90;

    /// Maximum fraction of context budget a single tool output may occupy.
    /// Even if we have room, a single output shouldn't dominate the context.
    const SINGLE_OUTPUT_MAX_FRACTION: f32 = 0.30;

    /// Execute a tool by name
    pub async fn execute(&self, name: &str, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let tools = self.tools.read().await;
        let resolved_name = Self::resolve_tool_name(name);
        if let Some(policy) = session_tool_policy(&ctx.session_id) {
            if let Some(allowed) = policy.allowed_tools.as_ref()
                && !allowed.contains(resolved_name)
            {
                return Err(anyhow::anyhow!("Tool '{}' is not allowed", resolved_name));
            }
            if policy.disabled_tools.contains(resolved_name) {
                return Err(anyhow::anyhow!("Tool '{}' is disabled", resolved_name));
            }
        }
        let tool = match tools.get(resolved_name) {
            Some(tool) => tool.clone(),
            None => {
                // List available tools so the model can recover instead of
                // spiraling through hallucinated names like "ToolSearch" (#104).
                let mut available: Vec<&str> = tools.keys().map(|k| k.as_str()).collect();
                available.sort_unstable();
                let suggestions = Self::closest_tool_names(name, &available);
                let mut msg = format!("Unknown tool: {name}.");
                if !suggestions.is_empty() {
                    msg.push_str(&format!(" Did you mean: {}?", suggestions.join(", ")));
                }
                msg.push_str(&format!(" Available tools: {}.", available.join(", ")));
                return Err(anyhow::anyhow!(msg));
            }
        };

        // Drop the lock before executing
        drop(tools);

        crate::logging::event_info(
            "TOOL_LIFECYCLE",
            Self::tool_lifecycle_fields("start", name, resolved_name, &input, &ctx),
        );

        // --- PreToolUse hook ---
        let cwd = ctx
            .working_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let hook_ctx = HookContext::for_tool(
            resolved_name.to_string(),
            ctx.session_id.clone(),
            cwd.clone(),
        );
        {
            let hook_registry = self.hook_registry.read().await;
            let handlers = hook_registry.get_matching(&HookEvent::PreToolUse, &hook_ctx);
            if !handlers.is_empty() {
                let hook_input = HookInputBuilder::new()
                    .session(&ctx.session_id, &cwd)
                    .event("PreToolUse")
                    .tool(resolved_name, input.clone(), &ctx.tool_call_id)
                    .build();
                let stats = jcode_hooks::dispatch_hooks(
                    &HookEvent::PreToolUse,
                    &hook_input,
                    &handlers,
                    &self.dispatch_config,
                )
                .await;
                if stats.any_denied() {
                    let deny_reason = stats
                        .results
                        .iter()
                        .find(|r| matches!(r.outcome, jcode_hooks::ClassifiedOutcome::Deny { .. }))
                        .map(|r| match &r.outcome {
                            jcode_hooks::ClassifiedOutcome::Deny { reason } => reason.clone(),
                            _ => String::new(),
                        })
                        .unwrap_or_else(|| "blocked by hook".to_string());
                    return Err(anyhow::anyhow!(
                        "Tool '{}' blocked by hook: {}",
                        resolved_name,
                        deny_reason
                    ));
                }
                if stats.any_asked() {
                    let ask_reasons: Vec<&str> = stats
                        .results
                        .iter()
                        .filter_map(|r| match &r.outcome {
                            jcode_hooks::ClassifiedOutcome::Ask { reason }
                                if !reason.is_empty() =>
                            {
                                Some(reason.as_str())
                            }
                            _ => None,
                        })
                        .collect();
                    let reason = if ask_reasons.is_empty() {
                        "user approval required by hook".to_string()
                    } else {
                        format!("user approval required by hook: {}", ask_reasons.join("; "))
                    };
                    return Err(anyhow::anyhow!("Tool '{}': {}", resolved_name, reason));
                }
            }
        }

        // --- Approval gate check (plugin-system gate, not DCG) ---
        if let Err(e) = check_approval_gate(resolved_name, &input) {
            let error_msg = format!("{}", e);
            crate::logging::warn(&error_msg);
            let mut fields =
                Self::tool_lifecycle_fields("denied", name, resolved_name, &input, &ctx);
            fields.push(("error".to_string(), error_msg.clone()));
            crate::logging::event_warn("TOOL_LIFECYCLE", fields);
            return Err(anyhow::anyhow!(error_msg));
        }

        let started_at = std::time::Instant::now();
        let result = tool.execute(input.clone(), ctx.clone()).await;
        let latency_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

        crate::telemetry::record_tool_execution(resolved_name, &input, result.is_ok(), latency_ms);

        let mut output = match result {
            Ok(output) => {
                // --- PostToolUse hook (fire-and-forget) ---
                let hook_registry = self.hook_registry.clone();
                let dispatch_config = self.dispatch_config.clone();
                let hook_input = HookInputBuilder::new()
                    .session(&ctx.session_id, &cwd)
                    .event("PostToolUse")
                    .tool(resolved_name, input.clone(), &ctx.tool_call_id)
                    .tool_output(serde_json::json!({ "output": &output.output }))
                    .duration(latency_ms)
                    .build();
                let handlers: Vec<_> = {
                    let reg = hook_registry.read().await;
                    reg.get_matching(&HookEvent::PostToolUse, &hook_ctx)
                        .into_iter()
                        .cloned()
                        .collect()
                };
                if !handlers.is_empty() {
                    let event = HookEvent::PostToolUse;
                    tokio::spawn(async move {
                        let refs: Vec<_> = handlers.iter().collect();
                        jcode_hooks::dispatch_hooks(&event, &hook_input, &refs, &dispatch_config)
                            .await;
                    });
                }
                output
            }
            Err(error) => {
                // --- PostToolUseFailure hook (fire-and-forget) ---
                let hook_registry = self.hook_registry.clone();
                let dispatch_config = self.dispatch_config.clone();
                let event = HookEvent::PostToolUseFailure;
                let hook_input = HookInputBuilder::new()
                    .session(&ctx.session_id, &cwd)
                    .event("PostToolUseFailure")
                    .tool(resolved_name, input.clone(), &ctx.tool_call_id)
                    .error(&crate::util::format_error_chain(&error), -1)
                    .duration(latency_ms)
                    .build();
                let handlers: Vec<_> = {
                    let reg = hook_registry.read().await;
                    reg.get_matching(&HookEvent::PostToolUseFailure, &hook_ctx)
                        .into_iter()
                        .cloned()
                        .collect()
                };
                if !handlers.is_empty() {
                    tokio::spawn(async move {
                        let refs: Vec<_> = handlers.iter().collect();
                        jcode_hooks::dispatch_hooks(&event, &hook_input, &refs, &dispatch_config)
                            .await;
                    });
                }

                // --- ToolError hook (fire-and-forget, diagnostic) ---
                {
                    let hook_registry = self.hook_registry.clone();
                    let dispatch_config = self.dispatch_config.clone();
                    let event = HookEvent::ToolError;
                    let hook_input = HookInputBuilder::new()
                        .session(&ctx.session_id, &cwd)
                        .event("ToolError")
                        .tool(resolved_name, input.clone(), &ctx.tool_call_id)
                        .error(&crate::util::format_error_chain(&error), -1)
                        .duration(latency_ms)
                        .build();
                    let handlers: Vec<_> = {
                        let reg = hook_registry.read().await;
                        reg.get_matching(&HookEvent::ToolError, &hook_ctx)
                            .into_iter()
                            .cloned()
                            .collect()
                    };
                    if !handlers.is_empty() {
                        tokio::spawn(async move {
                            let refs: Vec<_> = handlers.iter().collect();
                            jcode_hooks::dispatch_hooks(
                                &event,
                                &hook_input,
                                &refs,
                                &dispatch_config,
                            )
                            .await;
                        });
                    }
                }
                let mut fields =
                    Self::tool_lifecycle_fields("error", name, resolved_name, &input, &ctx);
                fields.push(("elapsed_ms".to_string(), latency_ms.to_string()));
                fields.push(("error".to_string(), crate::util::format_error_chain(&error)));
                crate::logging::event_warn("TOOL_LIFECYCLE", fields);
                return Err(error);
            }
        };

        // Context overflow guard: check if this output would push us over the limit
        output = self.guard_context_overflow(name, output).await;

        let mut fields = Self::tool_lifecycle_fields("done", name, resolved_name, &input, &ctx);
        fields.push(("elapsed_ms".to_string(), latency_ms.to_string()));
        fields.push(("output_bytes".to_string(), output.output.len().to_string()));
        fields.push((
            "output_chars".to_string(),
            output.output.chars().count().to_string(),
        ));
        fields.push(("image_count".to_string(), output.images.len().to_string()));
        crate::logging::event_info("TOOL_LIFECYCLE", fields);

        Ok(output)
    }

    /// Check if a tool output would overflow the context window and truncate if needed.
    /// Returns the (possibly truncated) output.
    async fn guard_context_overflow(&self, tool_name: &str, output: ToolOutput) -> ToolOutput {
        let compaction = self.compaction.read().await;
        let budget = compaction.token_budget();
        if budget == 0 {
            return output;
        }

        let current_tokens = compaction.effective_token_count();
        let output_tokens = Self::estimate_tokens(&output.output);

        // Check 1: Would adding this output push us over the safety threshold?
        let projected = current_tokens + output_tokens;
        let threshold_tokens = (budget as f32 * Self::CONTEXT_GUARD_THRESHOLD) as usize;

        // Check 2: Is this single output unreasonably large relative to budget?
        let single_max_tokens = (budget as f32 * Self::SINGLE_OUTPUT_MAX_FRACTION) as usize;

        let needs_truncation = projected > threshold_tokens || output_tokens > single_max_tokens;

        if !needs_truncation {
            return output;
        }

        // Calculate how many tokens we can afford for this output
        let remaining = if current_tokens < threshold_tokens {
            threshold_tokens - current_tokens
        } else {
            // Already over threshold — allow a small amount for the error message
            budget / 50 // ~2% of budget for the truncation notice
        };
        let max_tokens = remaining.min(single_max_tokens);

        // Convert token limit back to approximate character limit
        let max_chars = max_tokens * 4;

        if output.output.len() <= max_chars {
            return output;
        }

        crate::logging::info(&format!(
            "Context guard: truncating {} output from ~{}k to ~{}k tokens \
             (context: {}k/{}k, {:.0}% used)",
            tool_name,
            output_tokens / 1000,
            max_tokens / 1000,
            current_tokens / 1000,
            budget / 1000,
            (current_tokens as f32 / budget as f32) * 100.0,
        ));

        // Truncate the output, keeping the beginning (usually most relevant)
        let truncated = if max_chars > 200 {
            // Keep beginning of output + truncation notice
            let kept = &output.output[..output.output.floor_char_boundary(max_chars - 150)];
            format!(
                "{}\n\n⚠️ OUTPUT TRUNCATED: This tool output was {:.0}k tokens which would \
                 exceed the context window ({:.0}k/{}k tokens used, {}k budget). \
                 Only the first ~{:.0}k tokens are shown. Use more targeted queries \
                 (e.g., smaller line ranges, specific grep patterns) to get the content \
                 you need without exceeding context limits.",
                kept,
                output_tokens as f32 / 1000.0,
                current_tokens as f32 / 1000.0,
                budget / 1000,
                budget / 1000,
                max_tokens as f32 / 1000.0,
            )
        } else {
            // Context is almost completely full — just return error
            format!(
                "⚠️ CONTEXT LIMIT REACHED: Cannot return this tool output (~{:.0}k tokens) \
                 because the context window is nearly full ({:.0}k/{}k tokens). \
                 Consider using /compact to free up space, or use more targeted queries.",
                output_tokens as f32 / 1000.0,
                current_tokens as f32 / 1000.0,
                budget / 1000,
            )
        };

        ToolOutput {
            output: truncated,
            title: output.title,
            metadata: output.metadata,
            images: output.images,
        }
    }

    /// Register a tool dynamically (for MCP tools, etc.)
    pub async fn register(&self, name: String, tool: Arc<dyn Tool>) {
        let mut tools = self.tools.write().await;
        tools.insert(name, tool);
    }

    /// Register MCP tools (MCP management and server tools)
    /// Connections happen in background to avoid blocking startup.
    /// If `event_tx` is provided, sends an McpStatus event when connections complete.
    /// If `shared_pool` is provided, shared servers reuse processes from the pool.
    pub async fn register_mcp_tools(
        &self,
        event_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::protocol::ServerEvent>>,
        shared_pool: Option<std::sync::Arc<crate::mcp::SharedMcpPool>>,
        session_id: Option<String>,
    ) {
        use crate::mcp::McpManager;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        let mcp_manager = if let Some(pool) = shared_pool {
            let sid = session_id.unwrap_or_else(|| "unknown".to_string());
            Arc::new(RwLock::new(McpManager::with_shared_pool(pool, sid)))
        } else {
            Arc::new(RwLock::new(McpManager::new()))
        };

        // Register MCP management tool immediately (with registry for dynamic tool registration)
        let mcp_tool =
            mcp::McpManagementTool::new(Arc::clone(&mcp_manager)).with_registry(self.clone());
        self.register("mcp".to_string(), Arc::new(mcp_tool) as Arc<dyn Tool>)
            .await;

        // Check if we have servers to connect to
        let server_count = {
            let manager = mcp_manager.read().await;
            manager.config().servers.len()
        };

        if server_count > 0 {
            crate::logging::info(&format!("MCP: Found {} server(s) in config", server_count));

            // Send immediate "connecting" status so the TUI shows loading state
            // Server names with count 0 means "connecting..."
            if let Some(ref tx) = event_tx {
                let server_names: Vec<String> = {
                    let manager = mcp_manager.read().await;
                    manager
                        .config()
                        .servers
                        .keys()
                        .map(|name| format!("{}:0", name))
                        .collect()
                };
                let _ = tx.send(crate::protocol::ServerEvent::McpStatus {
                    servers: server_names,
                });
            }

            // Advertise-early: register proxy tools for each configured server
            // from the on-disk schema cache *before* connections settle, so the
            // first locked tool snapshot already contains MCP tools and we avoid
            // the intentional prompt-cache miss entirely (#206 Phase 2). The
            // proxies connect-on-first-call. Servers with no cached schemas yet
            // (cold start, or reconfigured) fall back to the post-connect
            // registration + one-shot late-register rebuild below.
            let schema_cache = crate::mcp::McpSchemaCache::load();
            let mut advertised_servers: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            {
                let config_servers: Vec<(String, crate::mcp::McpServerConfig)> = {
                    let manager = mcp_manager.read().await;
                    manager
                        .config()
                        .servers
                        .iter()
                        .map(|(name, cfg)| (name.clone(), cfg.clone()))
                        .collect()
                };
                let mut advertised_tool_count = 0usize;
                for (server, cfg) in &config_servers {
                    if let Some(cached) = schema_cache.tools_for(server, cfg) {
                        let tools = crate::mcp::create_mcp_tools_from_cached(
                            server,
                            cached,
                            Arc::clone(&mcp_manager),
                        );
                        advertised_tool_count += tools.len();
                        for (name, tool) in tools {
                            self.register(name, tool).await;
                        }
                        advertised_servers.insert(server.clone());
                    }
                }
                if advertised_tool_count > 0 {
                    crate::logging::info(&format!(
                        "MCP: advertised {} cached tool(s) from {} server(s) at spawn \
                         (connect-on-first-call); zero prompt-cache miss expected (#206)",
                        advertised_tool_count,
                        advertised_servers.len()
                    ));
                    // Reflect the advertised tools in the status indicator
                    // immediately so the UI shows them before connections settle.
                    if let Some(ref tx) = event_tx {
                        let mut counts: std::collections::BTreeMap<String, usize> =
                            std::collections::BTreeMap::new();
                        for (server, cfg) in &config_servers {
                            if let Some(cached) = schema_cache.tools_for(server, cfg) {
                                counts.insert(server.clone(), cached.len());
                            }
                        }
                        let servers: Vec<String> = counts
                            .into_iter()
                            .map(|(name, count)| format!("{}:{}", name, count))
                            .collect();
                        let _ = tx.send(crate::protocol::ServerEvent::McpStatus { servers });
                    }
                }
            }

            // Spawn connection and tool registration in background
            let registry = self.clone();
            tokio::spawn(async move {
                let (successes, failures) = {
                    let manager = mcp_manager.write().await;
                    manager.connect_all().await.unwrap_or((0, Vec::new()))
                };

                if successes > 0 {
                    crate::logging::info(&format!("MCP: Connected to {} server(s)", successes));
                }
                if !failures.is_empty() {
                    for (name, error) in &failures {
                        crate::logging::event_rate_limited(
                            crate::logging::LogLevel::Error,
                            &format!("mcp_register_failed:{name}"),
                            std::time::Duration::from_secs(60),
                            "MCP_REGISTER_FAILED",
                            vec![("server", name.to_string()), ("error", error.to_string())],
                        );
                    }
                }

                // Register MCP server tools and collect server info
                let tools = crate::mcp::create_mcp_tools(Arc::clone(&mcp_manager)).await;
                let mut server_counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for (name, tool) in &tools {
                    if let Some(rest) = name.strip_prefix("mcp__")
                        && let Some((server, _)) = rest.split_once("__")
                    {
                        *server_counts.entry(server.to_string()).or_default() += 1;
                    }
                    // Idempotent: advertise-early may have already registered an
                    // identical proxy. Re-registering refreshes it with the live
                    // schema, which is correct (handles schema drift).
                    registry.register(name.clone(), tool.clone()).await;
                }

                // Reconcile the on-disk schema cache with the live schemas so the
                // next spawn can advertise the up-to-date tools with zero cache
                // miss. Group live tool defs by server and update each entry
                // under the current config fingerprint; prune servers that are
                // no longer configured. (#206 Phase 2)
                #[allow(clippy::type_complexity)]
                {
                    // Live tool defs grouped by server, plus a snapshot of the
                    // configured servers, captured under one read lock.
                    type LiveToolsByServer =
                        std::collections::BTreeMap<String, Vec<crate::mcp::McpToolDef>>;
                    type ConfigSnapshot = Vec<(String, crate::mcp::McpServerConfig)>;
                    let (live_by_server, config_snapshot): (LiveToolsByServer, ConfigSnapshot) = {
                        let manager = mcp_manager.read().await;
                        let mut grouped: std::collections::BTreeMap<
                            String,
                            Vec<crate::mcp::McpToolDef>,
                        > = std::collections::BTreeMap::new();
                        for (server, def) in manager.all_tools().await {
                            grouped.entry(server).or_default().push(def);
                        }
                        let configs = manager
                            .config()
                            .servers
                            .iter()
                            .map(|(name, cfg)| (name.clone(), cfg.clone()))
                            .collect();
                        (grouped, configs)
                    };

                    let mut cache = crate::mcp::McpSchemaCache::load();
                    let mut dirty = false;
                    for (server, cfg) in &config_snapshot {
                        if let Some(defs) = live_by_server.get(server) {
                            // Only cache servers that actually exposed tools.
                            if cache.update(server, cfg, defs.clone()) {
                                dirty = true;
                            }
                        }
                    }
                    let configured_names: Vec<String> =
                        config_snapshot.iter().map(|(n, _)| n.clone()).collect();
                    if cache.retain_servers(&configured_names) {
                        dirty = true;
                    }
                    if dirty {
                        cache.save();
                        crate::logging::info(
                            "MCP: updated on-disk tool-schema cache from live connection (#206)",
                        );
                    }
                }

                // Notify client of MCP status
                if let Some(tx) = event_tx {
                    let servers: Vec<String> = server_counts
                        .into_iter()
                        .map(|(name, count)| format!("{}:{}", name, count))
                        .collect();
                    let _ = tx.send(crate::protocol::ServerEvent::McpStatus { servers });
                }
            });
        }
    }

    /// Register self-dev tools (only for canary/self-dev sessions)
    pub async fn register_selfdev_tools(&self) {
        // Self-dev management tool
        let selfdev_tool = selfdev::SelfDevTool::new();
        self.register(
            "selfdev".to_string(),
            Arc::new(selfdev_tool) as Arc<dyn Tool>,
        )
        .await;

        // Debug socket tool for direct debug socket access
        let debug_socket_tool = debug_socket::DebugSocketTool::new();
        self.register(
            "debug_socket".to_string(),
            Arc::new(debug_socket_tool) as Arc<dyn Tool>,
        )
        .await;
    }

    /// Register ambient-mode tools (only for ambient sessions)
    pub async fn register_ambient_tools(&self) {
        self.register(
            "end_ambient_cycle".to_string(),
            Arc::new(ambient::EndAmbientCycleTool::new()) as Arc<dyn Tool>,
        )
        .await;

        self.register(
            "schedule_ambient".to_string(),
            Arc::new(ambient::ScheduleAmbientTool::new()) as Arc<dyn Tool>,
        )
        .await;

        self.register(
            "request_permission".to_string(),
            Arc::new(ambient::RequestPermissionTool::new()) as Arc<dyn Tool>,
        )
        .await;

        self.register(
            "send_message".to_string(),
            Arc::new(ambient::SendChannelMessageTool::new()) as Arc<dyn Tool>,
        )
        .await;
    }

    /// Unregister a tool
    pub async fn unregister(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let mut tools = self.tools.write().await;
        tools.remove(name)
    }

    /// Unregister all tools matching a prefix
    pub async fn unregister_prefix(&self, prefix: &str) -> Vec<String> {
        let mut tools = self.tools.write().await;
        let to_remove: Vec<String> = tools
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        for name in &to_remove {
            tools.remove(name);
        }
        to_remove
    }

    /// Get shared access to the skill registry
    pub fn skills(&self) -> Arc<RwLock<SkillRegistry>> {
        self.skills.clone()
    }

    /// Get shared access to the compaction manager
    pub fn compaction(&self) -> Arc<RwLock<CompactionManager>> {
        self.compaction.clone()
    }
}

/// Classic Levenshtein edit distance over Unicode scalar values.
/// Used only for tool-name "did you mean" suggestions, so the simple
/// O(n*m) two-row implementation is more than sufficient.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod best_of_n_tests;
