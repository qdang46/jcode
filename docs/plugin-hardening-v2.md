# Implementation Plan: Plugin System v2 (Harden + Custom Plugin Authoring, Rust-first)
> Generated from research across 3 repos (opencode, oh-my-pi, pi-agent-rust) + user interview
> **Goal:** Extend jcode's existing `jcode-plugin-core` + `jcode-plugin-runtime` + `jcode-hooks` into a full custom-plugin authoring platform — users can create their own plugins and connect them into jcode — using **oh-my-pi as the primary inspiration**, with security hardening from **pi-agent-rust**, TUI richness from **opencode**, and the existing QuickJS + SWC + RCU + preflight + audit infrastructure kept in place.
>
> **Distribution policy (user-confirmed, 2026-06-18):**
> 1. **NO** `npm` distribution. **NO** `npm` registry. **NO** publishing anything to npm.
> 2. **NO** marketplace. **NO** plugin registry. All plugins are local or git-cloned.
> 3. Plugin runtime priority: **Rust workspace crate** (compile-in) > **JS/TS via QuickJS+SWC** (load-from-file at runtime).
> 4. Three distribution paths only: **local path**, **git clone**, **workspace crate** (`cargo path = true`).
>
> **Stage:** dev — backward compatibility is a soft preference, not a hard constraint. The plan is allowed to refactor or replace existing structures where the new model is clearly better.

---

## 1. Executive Summary

jcode already has a sophisticated plugin runtime (QuickJS via `rquickjs` + SWC TypeScript transpiler + RCU dispatcher + `CapabilityChain` + `AuditTrail` + TUI slot system + kill switches). What's missing — and what the 3 reference repos teach us to add — is **(a) a per-tool `ToolTier` (read/write/exec) as the load-bearing approval primitive**, **(b) a manifest schema that lets plugin authors declare tier, capabilities, settings, and engines compat in one place**, **(c) a `PluginManager` that handles load/unload/rollback for local path + git clone (no npm, no marketplace)**, **(d) a Rust workspace-crate path so plugin authors can write Rust and register via `inventory::submit!`** — the preferred path for Rust developers, **(e) hot-reload for fast iteration**, **(f) a STRIDE threat model that turns "is this plugin safe?" into a checkable list of properties**, and **(g) real author documentation with two working example plugins (one TS, one Rust)**. The single architectural choice that ties everything together is the **single chokepoint pattern**: every tool call — built-in, extension-registered, workspace-crate, MCP-bridged, or hot-reloaded — must route through `RcuDispatcher::dispatch` so approval, capability check, audit, and event emission happen in one place. We will add 10 new files (incl. 1 example workspace crate), modify 9 existing files, ship two example plugins (TS + Rust), and write one threat model. Expected outcome: a Rust developer can add a new workspace crate `crates/jcode-ext-foo/`, write `registerTool` + `on("before_tool_call")` calls, run `cargo build`, and have their tool appear in the next jcode session — with the right approval prompt, the right capability check, and a clear audit trail. A TS developer can `jcode plugin load ./my-plugin` and have a TS plugin work the same way. **No npm, no marketplace, no registry, no publish step ever.**

---

## 2. Architecture Decision

### Chosen Approach

**Adopt oh-my-pi's three-layer model** (capability-driven discovery + unified `Extension` surface + single chokepoint `ExtensionToolWrapper`) **on top of jcode's existing QuickJS+SWC+RCU+preflight+audit infrastructure**, **and borrow pi-agent-rust's 5-layer capability precedence + STRIDE threat model** for the security half.

The existing `jcode-plugin-core::CapabilityChain` already has 4 of the 5 layers from pi-agent-rust. We add the 5th (mode fallback) and re-name for clarity. The existing `jcode-plugin-runtime::RcuDispatcher` is exactly the single chokepoint that oh-my-pi implements with `ExtensionToolWrapper`. We keep the dispatcher and add a thin `ApprovalGate` wrapper that consults tier + capability + mode before each call.

The existing `jcode-plugin-core::PluginManifest` is already similar to oh-my-pi's. We extend it (add `tier`, `approval`, structured `capabilities`, `engines.jcode`) and bump the schema to `jcode-plugin.v2`. The existing `jcode-plugin-runtime::PluginLoader` handles file/JS/TS loading; we add npm install + git clone + project override + rollback on top.

The new pieces (none invented from scratch — all adapted from the 3 repos):
- **`ToolTier`** in `jcode-tool-types` (from omp `ToolTier = "read" | "write" | "exec"`)
- **`PluginManager`** in `jcode-plugin-core::manager` (from omp `PluginManager` install/uninstall/list/link with backup/rollback)
- **`ApprovalGate`** in `jcode-plugin-runtime::gate` (from omp `ExtensionToolWrapper.execute()` + pia `WasmExtensionToolWrapper` with timeout)
- **`HotReload`** in `jcode-plugin-runtime::loader` (new — neither omp nor pia have it, but opencode's `PluginMeta.fingerprint` is the seed for "did the file change")
- **`PluginThreatModel`** doc (from pia `docs/extension-runtime-threat-model.md`)

### Alternatives Considered

| Approach | Source Repo | Pros | Cons | Decision |
|----------|-------------|------|------|----------|
| **Adopt omp's 3-layer model + harden with pia's security, Rust-first distribution** | omp + pia | Builds on jcode's existing system. omp's authoring surface is closest to jcode's QuickJS+JS. pia gives us STRIDE + 5-layer precedence. Adds Rust workspace-crate path so plugin authors can write Rust without ever touching npm. Risk: small, mostly additive. | Two inspirations means we have to reconcile omp's `tier: read\|write\|exec` with pia's 5-layer `ExtensionPolicy`. | **CHOSEN** |
| Adopt pia's full WASM runtime as the only path | pia | Cleanest ABI, true sandbox, formal threat model. | Requires `wasmtime` ~80MB+ dependency, breaks QuickJS path, huge refactor, not the user's stage. | REJECTED — too costly for dev stage. Stretch goal (v3): add WASM as a parallel runtime behind a feature flag. |
| Adopt opencode's V1+V2 split (V1 in-process, V2 Effect-based) | opencode | Two-layer server+TUI is clean. | Effect-based V2 is TypeScript-specific (Effect library). Rust has no equivalent. Would require designing a new "Effect for Rust" runtime. | REJECTED — wrong language. Borrow opencode's two-layer (server + TUI) split but not the Effect pattern. |
| Rewrite from scratch on a clean foundation | none | Cleanest design. | Throws away QuickJS+SWC+RCU+preflight+audit investment. High risk, no incremental value. | REJECTED — user already has the foundation. |
| **Reject all npm/marketplace distribution** | (user constraint) | No registry dependency, no publish step, no version coupling, simpler mental model. Plugin author keeps full control of their code. | Smaller plugin ecosystem (no `npm install jcode-plugin-foo`). | **CHOSEN** — user explicitly opted out. Three distribution paths only: local path, git clone, Rust workspace crate. |
| Drop QuickJS entirely, go Rust-only | (hypothetical) | One language, no SWC transpiler dep, type-safe plugin code. | Loses the existing QuickJS+SWC+audit+preflight investment. Plugin author must compile before testing. | REJECTED — keep QuickJS for JS/TS plugins; add Rust workspace-crate path as the preferred option. |

---

## 3. Data Structures & Types

All new types go in `jcode-plugin-core`. The plan modifies `manifest.rs` and `security.rs`; the `manager` module is new.

```rust
// crates/jcode-plugin-core/src/manifest.rs — additions

/// Tier of risk/privilege a tool carries. Adapted from omp's ToolTier
/// (https://github.com/can1357/oh-my-pi/blob/main/packages/agent/src/types.ts#L477-L489).
/// Used by ApprovalGate to decide which prompts to show in which permission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolTier {
    /// Pure read of already-loaded data, no I/O, no mutation. (e.g. read_file, grep, ls)
    Read,
    /// Mutates workspace state or session state but doesn't spawn processes. (e.g. write_file, edit, todo)
    Write,
    /// Spawns subprocesses, makes network calls, or executes user code. (e.g. bash, fetch, browser)
    Exec,
}

impl Default for ToolTier {
    /// Fail-closed: unknown tools default to Exec (most privileged).
    /// Same default as omp's omitted-tier fallback.
    fn default() -> Self { Self::Exec }
}

/// v2 manifest. Bumps schema to "jcode-plugin.v2". v1 manifests
/// (`jcode-plugin.v1` or `pi/omp/opencode` field) are auto-upgraded
/// via `PluginManifest::migrate_v1_to_v2` at load time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifestV2 {
    pub schema: PluginSchemaVersion,                // = "jcode-plugin.v2"
    pub name: String,
    pub package_name: String,                       // npm-style "scope/name"
    pub version: String,                            // semver
    pub kind: PluginKind,                           // Server | Tui | Both
    pub entry: PluginEntry,                         // { server, tui, both } relative paths
    pub tier: ToolTier,                             // default tier if a tool doesn't override
    pub capabilities: PluginCapabilities,           // see below
    pub approval: PluginApprovalPolicy,             // default approval policy
    pub features: HashMap<String, PluginFeature>,
    pub settings: HashMap<String, SettingSchema>,
    pub engines: PluginEngines,                     // { jcode: ">=0.29" }
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginSchemaVersion { #[serde(rename = "jcode-plugin.v2")] V2 }

/// Per-tool tier override and approval declaration.
/// Borrowed from omp's ToolApproval (function form): either a static tier
/// or a function from args → decision. In Rust we use a discriminated
/// enum since we don't have first-class function values in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginApprovalPolicy {
    /// All tools use the manifest's default tier; user policy decides.
    Default,
    /// Specific tools get specific tiers; everything else uses default.
    PerTool { overrides: HashMap<String, ToolTier> },
    /// A tool requires user approval every time, regardless of mode.
    AlwaysPrompt { tools: Vec<String> },
    /// A tool is never allowed, even in BypassPermissions mode.
    Deny { tools: Vec<String> },
}

impl Default for PluginApprovalPolicy {
    fn default() -> Self { Self::Default }
}

/// Capabilities with explicit types per omp + pia.
/// Replaces jcode's current free-form `PluginCapabilities { fs_read, fs_write, ... }`
/// with a structured form that the preflight static analyzer can verify.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    /// Filesystem read paths (glob patterns, relative to plugin root).
    #[serde(default)] pub fs_read: Vec<String>,
    /// Filesystem write paths (glob patterns, relative to plugin root).
    #[serde(default)] pub fs_write: Vec<String>,
    /// HTTP hosts the plugin may call (exact, `*.suffix`, or `*`).
    #[serde(default)] pub http_hosts: Vec<String>,
    /// Environment variables the plugin may read (exact names).
    #[serde(default)] pub env_read: Vec<String>,
    /// Shell commands the plugin may execute (glob patterns, e.g. `git *`).
    #[serde(default)] pub shell_commands: Vec<String>,
    /// Tool names this plugin requires to be present (declare deps).
    #[serde(default)] pub requires_tools: Vec<String>,
    /// Tools this plugin provides (filled in by preflight, not author-declared).
    #[serde(default)] pub provides_tools: Vec<String>,
    /// Maximum number of host calls per second (pi-agent-rust quota pattern).
    #[serde(default)] pub max_hostcalls_per_sec: Option<u32>,
    /// Maximum wall-clock seconds per tool invocation (pi-agent-rust timeout pattern).
    #[serde(default)] pub max_tool_duration_secs: Option<u32>,
    /// Maximum bytes the plugin may write to disk cumulatively.
    #[serde(default)] pub max_bytes_written: Option<u64>,
}
```

```rust
// crates/jcode-plugin-core/src/security.rs — additions

/// 5-layer capability chain. Adapted from pi-agent-rust's ExtensionPolicy
/// (https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/src/extensions.rs#L2146)
/// and omp's per-extension + global policy merge. The current jcode
/// CapabilityChain has 4 layers; we add a "mode fallback" layer that
/// kicks in when no allow/deny list matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityChainV2 {
    /// Layer 1: per-extension deny list (highest priority, fails first).
    pub plugin_deny: CapabilitySet,
    /// Layer 2: global deny list (applies to all extensions).
    pub global_deny: CapabilitySet,
    /// Layer 3: per-extension allow list.
    pub plugin_allow: CapabilitySet,
    /// Layer 4: global allow list.
    pub global_allow: CapabilitySet,
    /// Layer 5: mode-based fallback (Strict | Prompt | Permissive | Disabled).
    /// Borrowed from pi-agent-rust's ExtensionPolicyMode.
    pub mode: PolicyMode,
    /// Optional global default override for unknown resources.
    /// If None, derive from mode: Strict → Deny, Permissive → Allow, Prompt → NeedsApproval.
    #[serde(default)] pub global_default: Option<AccessDefault>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyMode {
    /// Deny by default; explicit allow required. (omp: "always-ask")
    Strict,
    /// Allow by default; audit everything. (omp: "yolo", pia: "Permissive")
    Permissive,
    /// Deny unknown; prompt for ambiguous. (omp: "write", pia: "Prompt")
    Prompt,
    /// All extension calls disabled (kill switch).
    Disabled,
}

impl Default for PolicyMode { fn default() -> Self { Self::Prompt } }

/// Decision returned by CapabilityChainV2::check. Same as current
/// AccessDecision but renamed for clarity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecisionV2 {
    Allow { reason: String, layer: u8 },
    Deny { reason: String, layer: u8 },
    NeedsApproval { reason: String, layer: u8 },
}

impl CapabilityChainV2 {
    /// Check if a resource access is allowed. Returns the first matching layer.
    /// Evaluation order is strict: layer 1 → 2 → 3 → 4 → 5.
    /// Same shape as the current CapabilityChain::check, just with the
    /// 5th layer added and structured return.
    pub fn check(&self, resource: &str, action: &CapabilityAction) -> AccessDecisionV2 {
        // Layer 1: per-plugin deny
        if self.plugin_deny.matches(resource, action) {
            return AccessDecisionV2::Deny {
                reason: "Denied by plugin deny list".into(),
                layer: 1,
            };
        }
        // Layer 2: global deny
        if self.global_deny.matches(resource, action) {
            return AccessDecisionV2::Deny {
                reason: "Denied by global policy".into(),
                layer: 2,
            };
        }
        // Layer 3: per-plugin allow
        if self.plugin_allow.matches(resource, action) {
            return AccessDecisionV2::Allow {
                reason: "Allowed by plugin allow list".into(),
                layer: 3,
            };
        }
        // Layer 4: global allow
        if self.global_allow.matches(resource, action) {
            return AccessDecisionV2::Allow {
                reason: "Allowed by global allow list".into(),
                layer: 4,
            };
        }
        // Layer 5: mode-based fallback
        match (self.mode, self.global_default) {
            (PolicyMode::Disabled, _) => AccessDecisionV2::Deny {
                reason: "Plugin mode is 'disabled' (kill switch)".into(),
                layer: 5,
            },
            (_, Some(AccessDefault::Deny)) => AccessDecisionV2::Deny {
                reason: "Denied by global default".into(),
                layer: 5,
            },
            (_, Some(AccessDefault::Allow)) => AccessDecisionV2::Allow {
                reason: "Allowed by global default".into(),
                layer: 5,
            },
            (_, Some(AccessDefault::Ask)) => AccessDecisionV2::NeedsApproval {
                reason: "Requires user approval (global default)".into(),
                layer: 5,
            },
            (PolicyMode::Strict, None) => AccessDecisionV2::Deny {
                reason: "Strict mode: no explicit allow".into(),
                layer: 5,
            },
            (PolicyMode::Permissive, None) => AccessDecisionV2::Allow {
                reason: "Permissive mode: allow by default".into(),
                layer: 5,
            },
            (PolicyMode::Prompt, None) => AccessDecisionV2::NeedsApproval {
                reason: "Prompt mode: ask user".into(),
                layer: 5,
            },
        }
    }
}
```

```rust
// crates/jcode-plugin-core/src/manager.rs — new file (excerpt)

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// PluginManager handles install/uninstall/rollback/enable/disable for
/// user-authored plugins. Adapted from omp's PluginManager
/// (https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/plugins/manager.ts).
pub struct PluginManager {
    state: Arc<RwLock<PluginState>>,
    loader: Arc<PluginLoader>,
    install_root: PathBuf,         // ~/.jcode/plugins/
    lock_path: PathBuf,            // ~/.jcode/plugins/installed.json
    backup_dir: PathBuf,           // ~/.jcode/plugins/.backups/
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginState {
    /// Keyed by package_name.
    pub installed: HashMap<String, InstalledPlugin>,
    /// Last-known version after every mutation (for rollback).
    pub last_known_good: HashMap<String, InstalledPlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub manifest: PluginManifestV2,
    pub source: PluginSource,              // Npm { version } | Git { url, rev } | Local { path }
    pub install_path: PathBuf,
    pub installed_at: chrono::DateTime<chrono::Utc>,
    pub enabled: bool,
    pub settings: HashMap<String, serde_json::Value>,
}

impl PluginManager {
    /// Install a plugin from any source. Returns the InstalledPlugin record.
    /// If install fails, restores the previous state (atomic rollback).
    pub async fn install(&self, source: PluginSource) -> Result<InstalledPlugin, PluginError> {
        // Save last_known_good BEFORE any mutation
        let backup = self.state.read().await.last_known_good.clone();
        match self.install_inner(&source).await {
            Ok(installed) => {
                self.state.write().await.installed
                    .insert(installed.manifest.package_name.clone(), installed.clone());
                self.persist_state().await?;
                Ok(installed)
            }
            Err(e) => {
                // Rollback to backup
                self.state.write().await.last_known_good = backup;
                self.persist_state().await?;
                Err(e)
            }
        }
    }

    /// Uninstall a plugin by package_name. Idempotent.
    pub async fn uninstall(&self, package_name: &str) -> Result<(), PluginError> { ... }

    /// List installed plugins. Filters out hidden ones unless show_all is true.
    pub async fn list(&self, show_all: bool) -> Vec<InstalledPlugin> { ... }

    /// Enable or disable a plugin without uninstalling.
    /// Disabled plugins stay on disk; loader skips them at startup.
    pub async fn set_enabled(&self, package_name: &str, enabled: bool) -> Result<(), PluginError> { ... }

    /// Link a local directory as a plugin (symlink under install_root/<name>).
    /// Useful for plugin authors iterating locally.
    pub async fn link(&self, path: &PathBuf) -> Result<InstalledPlugin, PluginError> { ... }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginSource {
    /// Load a plugin from a local path (file or directory). The directory
    /// must contain a `package.json` with a `jcode-plugin.v2` field, or
    /// (for TS plugins) an `index.ts`/`index.js` entry. Plugin source is
    /// NOT copied to `install_root` — the loader resolves the path at
    /// every startup. This is the simplest distribution and ideal for
    /// plugin authors iterating locally.
    Local { path: PathBuf },

    /// Clone a git repository into `install_root/<name>/`, then load as
    /// if it were a local path. Supports `https://...`, `git@...`, and
    /// `git://...`. Optional `rev` pins to a commit SHA, branch, or tag.
    /// The clone is a regular git clone (no submodule init by default);
    /// plugin author can document build steps in their README.
    Git { url: String, rev: Option<String> },

    /// Reference a Rust crate that is already a member of the jcode
    /// workspace. The crate's `lib.rs` uses `inventory::submit!` to
    /// register itself at link time. This is the **preferred** path for
    /// plugin authors who want full Rust type safety, no JS/TS layer,
    /// and zero runtime cost. Enabled/disabled via `[plugins.workspace]`
    /// in config.toml; no install step required.
    WorkspaceCrate { crate_name: String },
}

// NOTE: Npm { ... } and Registry { ... } variants were considered and
// rejected. See Section 2 (Alternatives Considered) for rationale.
// The user's distribution policy is: NO npm, NO marketplace, NO registry.
```

```rust
// crates/jcode-plugin-runtime/src/gate.rs — new file (excerpt)

use jcode_plugin_core::{AccessDecisionV2, CapabilityAction, CapabilityChainV2, ToolTier};
use jcode_tui_permissions::PermissionMode;

/// ApprovalGate is the single chokepoint that wraps every tool invocation.
/// It consults: (1) per-tool tier, (2) capability chain, (3) current
/// permission mode, (4) per-tool user override, (5) tool's own declaration.
/// Adapted from omp's ExtensionToolWrapper.execute()
/// (https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L113-L179).
pub struct ApprovalGate {
    chain: CapabilityChainV2,
    mode: PermissionMode,
    user_overrides: HashMap<String, ApprovalOverride>,  // tools.approval.<tool>: allow|deny|prompt
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOverride { Allow, Deny, Prompt }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    Deny { reason: String, layer: &'static str },
    NeedsApproval { prompt: ApprovalPrompt },
}

#[derive(Debug, Clone)]
pub struct ApprovalPrompt {
    pub tool_name: String,
    pub tier: ToolTier,
    pub reason: String,
    pub mode: PermissionMode,
    /// Optional human-readable details to show the user (command preview, etc.)
    pub details: Option<String>,
}

impl ApprovalGate {
    /// Check whether a tool call is allowed. Called by RcuDispatcher::dispatch
    /// for every tool invocation, before the tool's execute() is called.
    /// Returns Allow (proceed), Deny (return error to LLM), or NeedsApproval
    /// (show prompt to user, then re-call with the user's decision).
    pub fn check(
        &self,
        tool_name: &str,
        tier: ToolTier,
        args: &serde_json::Value,
    ) -> GateDecision {
        // 1. User override always wins.
        if let Some(ov) = self.user_overrides.get(tool_name) {
            return match ov {
                ApprovalOverride::Allow => GateDecision::Allow,
                ApprovalOverride::Deny => GateDecision::Deny {
                    reason: format!("User policy denies '{}'", tool_name),
                    layer: "user_override",
                },
                ApprovalOverride::Prompt => GateDecision::NeedsApproval(self.prompt_for(tool_name, tier, args)),
            };
        }

        // 2. Map tier to resource class and check capability chain.
        let resource = format!("tool:{}", tool_name);
        let action = match tier {
            ToolTier::Read => CapabilityAction::Read,
            ToolTier::Write => CapabilityAction::Write,
            ToolTier::Exec => CapabilityAction::Execute,
        };
        match self.chain.check(&resource, &action) {
            AccessDecisionV2::Allow { .. } => GateDecision::Allow,
            AccessDecisionV2::Deny { reason, layer } => GateDecision::Deny { reason, layer: layer_name(layer) },
            AccessDecisionV2::NeedsApproval { reason, layer } => {
                // Check mode: does this tier need a prompt in the current mode?
                let needs_prompt = match (self.mode, tier) {
                    (PermissionMode::Plan, _) => true,                     // Plan mode: prompt for everything
                    (PermissionMode::AcceptEdits, ToolTier::Exec) => true, // AcceptEdits: prompt for Exec only
                    (PermissionMode::AcceptEdits, _) => false,            // Read + Write auto-approve
                    (PermissionMode::BypassPermissions, _) => false,       // Bypass: never prompt
                    (PermissionMode::DontAsk, ToolTier::Read) => false,   // DontAsk: Read auto-approves
                    (PermissionMode::DontAsk, _) => true,                 // Write + Exec prompt
                };
                if needs_prompt {
                    GateDecision::NeedsApproval(self.prompt_for(tool_name, tier, args))
                } else {
                    GateDecision::Allow
                }
            }
        }
    }

    fn prompt_for(&self, tool_name: &str, tier: ToolTier, args: &serde_json::Value) -> ApprovalPrompt {
        ApprovalPrompt {
            tool_name: tool_name.into(),
            tier,
            reason: format!("{} tier tool", tier_name(tier)),
            mode: self.mode,
            details: approval_details(tool_name, args),  // e.g. truncate command for bash
        }
    }
}

fn tier_name(t: ToolTier) -> &'static str {
    match t { ToolTier::Read => "read", ToolTier::Write => "write", ToolTier::Exec => "exec" }
}
fn layer_name(l: u8) -> &'static str {
    match l {
        1 => "plugin_deny", 2 => "global_deny", 3 => "plugin_allow",
        4 => "global_allow", 5 => "mode_fallback", _ => "unknown",
    }
}
```

---

## 4. Pseudocode — Core Algorithm

The single algorithm that ties everything together is the **tool dispatch pipeline**. It is the same shape as omp's `ExtensionToolWrapper.execute()` and pia's `WasmExtensionToolWrapper::execute`, but with our 5-layer gate wired in.

```
FUNCTION dispatch_tool_call(tool_name, args, call_id, ctx) -> Result<ToolOutput>:
    # === Stage 0: pre-tool hooks ===
    # Fire "before_tool_call" event. First handler returning {block: true, reason}
    # short-circuits. (omp pattern, packages/coding-agent/src/extensibility/extensions/runner.ts:597-695)
    block_reason = emit_event("before_tool_call", { tool_name, args, ctx }, cancellable=true)
    IF block_reason IS NOT NULL:
        audit_log("tool_blocked", { tool_name, reason: block_reason })
        RETURN Error("Tool blocked: " + block_reason)

    # === Stage 1: look up the tool ===
    snapshot = rcudispatcher.read_snapshot()             # Lock-free read
    tool = snapshot.tools.get(tool_name)
    IF tool IS NULL:
        RETURN Error("Unknown tool: " + tool_name)

    # === Stage 2: derive tier ===
    # Tool's own declaration wins; else fall back to manifest's default tier;
    # else fail-closed to Exec.
    tier = tool.declared_tier()
        OR registry.get_manifest(tool.plugin_id).tier
        OR ToolTier::Exec

    # === Stage 3: approval gate (the single chokepoint) ===
    decision = approval_gate.check(tool_name, tier, args)
    SWITCH decision:
        CASE Deny:
            audit_log("tool_denied", { tool_name, tier, reason, layer })
            RETURN Error("Denied: " + reason)
        CASE NeedsApproval(prompt):
            user_choice = await ui.show_approval_prompt(prompt)  # 30s timeout
            IF user_choice == "deny":
                audit_log("tool_denied_by_user", { tool_name, tier, prompt })
                RETURN Error("Denied by user")
            audit_log("tool_approved_by_user", { tool_name, tier, prompt })
        CASE Allow:
            PASS

    # === Stage 4: acquire concurrency slot ===
    # tool.effects() returns "read" | "write" | "exec"; read tools can run
    # in parallel, write/exec are serialized per-plugin.
    # (pi-agent-rust ToolEffects pattern,
    #  https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/src/tools.rs#L38-L155)
    slot = concurrency_limiter.acquire(tool.effects(), ctx.plugin_id)
    DEFER slot.release()

    # === Stage 5: execute with timeout ===
    # Default 60s for most tools; 5min for explicitly-declared long-running tools.
    timeout_secs = tool.max_duration_secs() OR 60
    output = WITH_TIMEOUT(timeout_secs, tool.execute(call_id, args, on_update, ctx))
    IF timeout:
        audit_log("tool_timeout", { tool_name, tier, timeout_secs })
        RETURN Error("Tool timed out after " + timeout_secs + "s")

    # === Stage 6: post-tool hooks ===
    # Fire "after_tool_call" event. Each handler can mutate output.
    # Last handler's overrides win. (omp pattern)
    emit_event("after_tool_call", { tool_name, args, output, ctx }, cancellable=false)

    # === Stage 7: audit log ===
    audit_log("tool_executed", {
        tool_name, tier, duration_ms, output_blocks: output.content.len(),
        is_error: output.is_error,
    })

    RETURN output
```

The `emit_event` function is the typed event bus. It dispatches to all
registered handlers in registration order, with per-handler timeouts
(30s default, 2s for `session_shutdown`).

```
FUNCTION emit_event(event_name, payload, cancellable) -> Option<block_reason>:
    handlers = event_registry.get_handlers(event_name).clone()  # snapshot for safety
    FOR handler IN handlers:
        timeout = if event_name == "session_shutdown" { 2 } else { 30 }
        TRY:
            result = WITH_TIMEOUT(timeout, handler.call(payload.clone()))
            IF result.blocks AND cancellable:
                RETURN result.reason       # short-circuit
            IF NOT cancellable:
                payload = result.modified_payload  # chain mutations
        CATCH TimeoutError:
            audit_log("handler_timeout", { event: event_name, handler: handler.id, timeout })
            IF NOT cancellable: CONTINUE
            RETURN "Handler timeout"
        CATCH e:
            audit_log("handler_error", { event: event_name, handler: handler.id, error: e })
            IF NOT cancellable: CONTINUE
            RETURN "Handler error: " + e.message
    RETURN None
```

The `PluginManager.install` algorithm is the install/rollback pipeline:

```
FUNCTION plugin_manager_install(source) -> Result<InstalledPlugin>:
    # 1. Save state snapshot for rollback
    backup = state.clone()

    # 2. Resolve source to a path
    match source:
        Npm { spec }:
            resolved = npm_install_to_tmp(spec)
            IF error: ROLLBACK
        Git { url, rev }:
            resolved = git_clone_to_tmp(url, rev)
            IF error: ROLLBACK
        Local { path }:
            resolved = path  # no copy
        Registry { ... }: UNIMPLEMENTED (stretch)

    # 3. Read + parse manifest (with v1→v2 migration)
    manifest = read_manifest(resolved)
    IF error OR schema != "jcode-plugin.v2":
        IF can_migrate: manifest = migrate_v1_to_v2(manifest)
        ELSE: ROLLBACK with "unsupported schema"

    # 4. Run preflight static analysis
    preflight_result = preflight.analyze(resolved, manifest)
    IF preflight_result.critical_issues:
        ROLLBACK with "preflight failed: " + issues

    # 5. Check engines compatibility
    IF manifest.engines.jcode AND NOT semver_compatible(manifest.engines.jcode, JCODE_VERSION):
        ROLLBACK with "engines.jcode mismatch"

    # 6. Check per-extension kill switch
    IF env_var("JCODE_PLUGIN_KILL_" + manifest.package_name.uppercase()) == "1":
        ROLLBACK with "per-extension kill switch set"

    # 7. Move resolved to install_root/<package_name>/
    install_path = install_root / manifest.package_name
    IF install_path.exists():
        backup_existing(install_path, backup_dir)
    move(resolved, install_path)

    # 8. Update state
    installed = InstalledPlugin { manifest, source, install_path, installed_at: now, enabled: true, settings: {} }
    state.installed.insert(manifest.package_name, installed)
    persist_state()
    RETURN installed
```

---

## 5. Implementation Code

### File: `crates/jcode-plugin-core/src/manifest.rs` (modify)

Add `ToolTier`, `PluginSchemaVersion`, `PluginApprovalPolicy`, `PluginCapabilities` (replace the existing free-form version with the structured one). Bump schema to v2. Add `migrate_v1_to_v2` function.

```rust
// At the top, keep existing imports. Add the new types from Section 3.

impl PluginManifestV2 {
    /// Parse from package.json value, with v1→v2 migration.
    /// v1 manifests have the same fields but as `jcode` or `pi` (legacy) keys
    /// and use the old free-form PluginCapabilities. We detect by checking
    /// for `schema: "jcode-plugin.v2"`.
    pub fn from_package_json(value: &serde_json::Value) -> Result<Self, PluginError> {
        // First try v2 explicit schema
        if let Some(section) = value.get("jcode-plugin").and_then(|v| v.get("v2")) {
            return serde_json::from_value(section.clone())
                .map_err(|e| PluginError::InvalidManifest(e.to_string()));
        }
        // Then try v1 keys: "jcode", "pi", "omp", "opencode" (any of these means it's v1)
        for key in &["jcode", "pi", "omp", "opencode"] {
            if let Some(section) = value.get(*key) {
                let v1: PluginManifestV1 = serde_json::from_value(section.clone())
                    .map_err(|e| PluginError::InvalidManifest(format!("v1 parse: {}", e)))?;
                return Ok(Self::migrate_v1_to_v2(v1));
            }
        }
        Err(PluginError::InvalidManifest("no plugin manifest found".into()))
    }

    /// Migrate a v1 manifest to v2. Tier defaults to Exec (fail-closed).
    /// Capabilities are wrapped into the structured form.
    fn migrate_v1_to_v2(v1: PluginManifestV1) -> Self {
        Self {
            schema: PluginSchemaVersion::V2,
            name: v1.name,
            package_name: v1.package_name,
            version: v1.version,
            kind: v1.kind,
            entry: v1.entry,
            tier: ToolTier::Exec, // fail-closed for v1 plugins we don't know
            capabilities: PluginCapabilities {
                fs_read: v1.capabilities.fs_read,
                fs_write: v1.capabilities.fs_write,
                http_hosts: v1.capabilities.hosts.unwrap_or_default(),
                env_read: v1.capabilities.env_vars.unwrap_or_default(),
                shell_commands: v1.capabilities.shell_commands.unwrap_or_default(),
                requires_tools: v1.capabilities.tools.unwrap_or_default(),
                provides_tools: vec![],
                max_hostcalls_per_sec: None,
                max_tool_duration_secs: None,
                max_bytes_written: None,
            },
            approval: PluginApprovalPolicy::Default,
            features: v1.features,
            settings: v1.settings,
            engines: v1.engines,
            description: v1.description,
            author: v1.author,
            license: v1.license,
            homepage: v1.homepage,
            repository: v1.repository,
            tags: v1.tags,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifestV1 {
    pub name: String,
    pub package_name: String,
    pub version: String,
    #[serde(default)] pub kind: PluginKind,
    #[serde(default)] pub entry: PluginEntry,
    #[serde(default)] pub capabilities: PluginCapabilitiesV1,
    #[serde(default)] pub features: HashMap<String, PluginFeature>,
    #[serde(default)] pub settings: HashMap<String, SettingSchema>,
    #[serde(default)] pub engines: PluginEngines,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub author: Option<String>,
    #[serde(default)] pub license: Option<String>,
    #[serde(default)] pub homepage: Option<String>,
    #[serde(default)] pub repository: Option<String>,
    #[serde(default)] pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilitiesV1 {
    #[serde(default)] pub fs_read: Vec<String>,
    #[serde(default)] pub fs_write: Vec<String>,
    #[serde(default)] pub hosts: Option<Vec<String>>,
    #[serde(default)] pub env_vars: Option<Vec<String>>,
    #[serde(default)] pub shell_commands: Option<Vec<String>>,
    #[serde(default)] pub tools: Option<Vec<String>>,
}
```

### File: `crates/jcode-plugin-core/src/security.rs` (modify)

Replace the existing `CapabilityChain` with `CapabilityChainV2` (5 layers, structured return). Keep `CapabilityChain` as a deprecated type alias for one release, then remove.

### File: `crates/jcode-plugin-core/src/manager.rs` (new)

Full file content from Section 3 excerpt above, plus:
- `install_inner` (the actual install steps)
- `uninstall_inner`
- `link` (symlink under install_root)
- `persist_state` (write installed.json with atomic write + tempfile)
- `load_state` (read installed.json, missing file → empty state)
- `get_enabled` (read state, return only `enabled: true` plugins for the loader)

### File: `crates/jcode-plugin-core/src/lib.rs` (modify)

Add `pub mod manager;` and re-export the new types.

### File: `crates/jcode-plugin-runtime/src/gate.rs` (new)

Full file content from Section 3 excerpt above, plus:
- `with_user_override(user_overrides: HashMap<String, ApprovalOverride>) -> Self` builder
- `set_mode(mode: PermissionMode)` for live mode changes
- `format_prompt(prompt: &ApprovalPrompt) -> String` for the UI

### File: `crates/jcode-plugin-runtime/src/dispatcher.rs` (modify)

The current `RcuDispatcher::dispatch` (which is the single chokepoint) needs to be wired to call `ApprovalGate::check` before `tool.execute()`. The change is small:

```rust
impl RcuDispatcher {
    pub async fn dispatch(
        &self,
        tool_name: &str,
        args: serde_json::Value,
        call_id: &str,
        ctx: ToolContext,
    ) -> Result<ToolOutput, DispatchError> {
        // Read snapshot (lock-free)
        let snap = self.snapshot.read().await.clone();
        let tool = snap.tools.get(tool_name)
            .ok_or_else(|| DispatchError::UnknownTool(tool_name.into()))?
            .clone();

        // === NEW: approval gate (the single chokepoint) ===
        let tier = tool.declared_tier()
            .or_else(|| snap.manifest_for(&tool.plugin_id).map(|m| m.tier))
            .unwrap_or(ToolTier::Exec);
        match self.gate.check(tool_name, tier, &args).await {
            GateDecision::Allow => {} // proceed
            GateDecision::Deny { reason, layer } => {
                self.audit.log_deny(tool_name, tier, &reason, layer).await;
                return Err(DispatchError::Denied(reason));
            }
            GateDecision::NeedsApproval(prompt) => {
                let choice = self.ui.show_approval(prompt).await?;
                match choice {
                    ApprovalChoice::Deny => {
                        self.audit.log_deny_by_user(tool_name, tier).await;
                        return Err(DispatchError::DeniedByUser(tool_name.into()));
                    }
                    ApprovalChoice::Allow => {
                        self.audit.log_approved_by_user(tool_name, tier).await;
                    }
                }
            }
        }

        // Emit before_tool_call (cancellable)
        if let Some(reason) = self.event_bus.emit("before_tool_call", &ctx, true).await {
            self.audit.log_blocked(tool_name, &reason).await;
            return Err(DispatchError::Blocked(reason));
        }

        // Acquire concurrency slot
        let _slot = self.concurrency.acquire(tool.effects(), &tool.plugin_id).await;

        // Execute with timeout
        let timeout = tool.max_duration_secs().unwrap_or(60);
        let output = tokio::time::timeout(
            Duration::from_secs(timeout),
            tool.execute(call_id, args.clone(), ctx.on_update.clone(), ctx.clone()),
        )
        .await
        .map_err(|_| DispatchError::Timeout(tool_name.into(), timeout))?
        .map_err(DispatchError::Tool)?;

        // Emit after_tool_call (chained mutation)
        self.event_bus.emit("after_tool_call", &(tool_name, &output, &ctx), false).await;

        // Audit
        self.audit.log_executed(tool_name, tier, &output, &ctx).await;
        Ok(output)
    }
}
```

### File: `crates/jcode-plugin-runtime/src/loader.rs` (modify)

Add `reload(plugin_id: &PluginId) -> Result<(), LoaderError>` for hot-reload. The implementation:

1. Read the file mtime and hash (SHA-256) into a `PluginFingerprint` struct.
2. If the fingerprint matches the cached one, return `Ok(())` (no-op).
3. Otherwise:
   - Call `transpiler.transpile(&path)` to get fresh JS.
   - Create a new `rquickjs::Context` with the same sandbox + timeout config.
   - Evaluate the new JS, capture any tool registrations.
   - Build a new `PluginInstance` (without touching the old one yet).
   - Run preflight static analysis on the new code.
   - Atomically swap: `RcuDispatcher::replace_plugin(old_id, new_instance)`. The old instance is dropped AFTER the new one is in place; QuickJS contexts are cheap to drop.
   - Update the fingerprint cache.

```rust
impl PluginLoader {
    /// Hot-reload a single plugin by id. Compares SHA-256 of source;
    /// if unchanged, no-op. Otherwise re-transpiles, re-instantiates,
    /// and atomically swaps into RcuDispatcher.
    /// Adapted from opencode's PluginMeta.fingerprint pattern
    /// (https://github.com/anomalyco/opencode/blob/main/packages/opencode/src/plugin/meta.ts).
    pub async fn reload(&self, plugin_id: &PluginId) -> Result<(), LoaderError> {
        let path = self.path_for(plugin_id).await?;
        let new_fp = self.fingerprint(&path).await?;
        {
            let cache = self.fingerprints.read().await;
            if cache.get(plugin_id) == Some(&new_fp) {
                return Ok(()); // no-op
            }
        }

        // Transpile + preflight
        let js = self.transpiler.transpile(&path).await?;
        self.preflight.check(&js, &path)?;

        // Build new instance
        let new_instance = self.instantiate(plugin_id.clone(), &js, &path).await?;

        // Atomic swap via RCU
        self.dispatcher.replace_plugin(plugin_id, new_instance).await?;

        // Update fingerprint cache
        self.fingerprints.write().await.insert(plugin_id.clone(), new_fp);
        Ok(())
    }

    async fn fingerprint(&self, path: &Path) -> Result<PluginFingerprint, LoaderError> {
        let bytes = tokio::fs::read(path).await?;
        let mtime = tokio::fs::metadata(path).await?.modified()?;
        let hash = seahash::hash(&bytes);
        Ok(PluginFingerprint { sha256: hash, mtime, size: bytes.len() as u64 })
    }
}
```

### File: `crates/jcode-plugin-runtime/src/server.rs` (modify)

Add per-extension kill switch check in `PluginSystem::load`:

```rust
fn is_killed(plugin_name: &str) -> bool {
    if std::env::var("JCODE_PLUGIN_KILL_ALL").is_ok() { return true; }
    let key = format!("JCODE_PLUGIN_KILL_{}", plugin_name.to_uppercase().replace('-', "_").replace('/', "_"));
    std::env::var(&key).map(|v| v == "1").unwrap_or(false)
}
```

### File: `crates/jcode-tui-permissions/src/lib.rs` (modify)

Add `PermissionMode` mapping to `ToolTier` so the existing modes work with the new tier system:

```rust
use jcode_plugin_core::ToolTier;

impl PermissionMode {
    /// Map a tier + this mode to whether the tier is auto-approved.
    /// Encodes the table from the gate pseudocode in Section 4.
    pub fn auto_approves(&self, tier: ToolTier) -> bool {
        match (self, tier) {
            (PermissionMode::Plan, _) => false,                    // always prompt in Plan
            (PermissionMode::AcceptEdits, ToolTier::Read) => true,
            (PermissionMode::AcceptEdits, ToolTier::Write) => true,
            (PermissionMode::AcceptEdits, ToolTier::Exec) => false,
            (PermissionMode::BypassPermissions, _) => true,        // never prompt
            (PermissionMode::DontAsk, ToolTier::Read) => true,
            (PermissionMode::DontAsk, ToolTier::Write) => false,
            (PermissionMode::DontAsk, ToolTier::Exec) => false,
        }
    }
}
```

### File: `crates/jcode-tool-types/src/lib.rs` (modify)

Add `ToolTier` re-export and a `Tool::declared_tier()` method (default returns `None`, meaning "use manifest's default tier"):

```rust
pub use jcode_plugin_core::ToolTier;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn declared_tier(&self) -> Option<ToolTier> { None } // default: use manifest tier
    fn max_duration_secs(&self) -> Option<u32> { None }  // default: 60s
    fn effects(&self) -> ToolEffects { ToolEffects::Write }  // fail-closed
    async fn execute(&self, call_id: &str, input: serde_json::Value, on_update: Option<...>, ctx: ToolContext) -> Result<ToolOutput, ToolError>;
}
```

### File: `examples/plugins/hello-plugin/package.json` (new)

```json
{
  "name": "hello-plugin",
  "version": "0.1.0",
  "description": "A demo plugin that registers a tool and a hook",
  "jcode-plugin": {
    "v2": {
      "schema": "jcode-plugin.v2",
      "name": "Hello Plugin",
      "package_name": "jcode-hello-plugin",
      "version": "0.1.0",
      "kind": "server",
      "entry": { "server": "index.ts" },
      "tier": "write",
      "capabilities": {
        "fs_write": ["output.txt"]
      },
      "approval": { "kind": "default" },
      "engines": { "jcode": ">=0.29" }
    }
  }
}
```

### File: `examples/plugins/hello-plugin/index.ts` (new)

```typescript
// Authored as TypeScript, transpiled by jcode-plugin-runtime::Transpiler (SWC).
// Same authoring surface as omp's `examples/extensions/hello.ts`.

import type { ExtensionAPI } from "@jcode/plugin-api";

export default function (pi: ExtensionAPI): void {
  // Register a tool. Tier declared as "write" (the plugin's default).
  pi.registerTool({
    name: "hello",
    description: "Say hello to a name and write it to output.txt",
    parameters: { type: "object", properties: { name: { type: "string" } }, required: ["name"] },
    async execute(toolCallId, args) {
      const greeting = `Hello, ${args.name}!`;
      // Capability: fs_write on output.txt is declared in manifest, gate allows.
      await pi.host.fs.writeFile("output.txt", greeting);
      return { content: [{ type: "text", text: greeting }] };
    },
  });

  // Subscribe to a lifecycle event. Cancels bash calls containing "rm -rf".
  pi.on("before_tool_call", (event) => {
    if (event.toolName === "bash" && /rm\s+-rf/.test(event.args.command)) {
      return { block: true, reason: "rm -rf is not allowed by hello-plugin" };
    }
  });
}
```

### File: `crates/jcode-ext-hello/Cargo.toml` (new — Rust workspace-crate example)

```toml
[package]
name = "jcode-ext-hello"
version = "0.1.0"
edition = "2024"
description = "Hello-world example plugin written in Rust (workspace crate, compiled into the jcode binary)"

[dependencies]
jcode-plugin-core = { path = "../jcode-plugin-core" }
inventory = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
async-trait = "0.1"
tokio = { version = "1", features = ["fs", "macros"] }
```

### File: `crates/jcode-ext-hello/src/lib.rs` (new — Rust example)

```rust
//! Hello-world plugin authored in Rust, compiled into the jcode binary.
//! Registers itself via the `inventory` crate at link time; the host scans
//! `inventory::iter::<PluginDescriptor>()` at startup and instantiates each
//! submitted plugin. No `jcode plugin install` step needed — just `cargo build`.

use std::sync::Arc;
use jcode_plugin_core::prelude::*;  // {PluginDescriptor, register, Tier, ...}

pub struct HelloPlugin;

#[async_trait]
impl ExtensionHandler for HelloPlugin {
    fn manifest(&self) -> PluginManifestV2 {
        PluginManifestV2 {
            schema: PluginSchemaVersion::V2,
            name: "Hello Plugin (Rust)".into(),
            package_name: "jcode-ext-hello".into(),
            version: "0.1.0".into(),
            kind: PluginKind::Server,
            entry: PluginEntry::default(), // not used for workspace crates
            tier: ToolTier::Write,
            capabilities: PluginCapabilities {
                fs_write: vec!["output.txt".into()],
                ..Default::default()
            },
            approval: PluginApprovalPolicy::Default,
            features: Default::default(),
            settings: Default::default(),
            engines: PluginEngines { jcode: Some(">=0.29".into()), ..Default::default() },
            description: Some("Rust example plugin".into()),
            author: None, license: None, homepage: None, repository: None, tags: vec![],
        }
    }

    async fn register(&self, api: &mut ExtensionApi<'_>) -> Result<(), PluginError> {
        // Register a tool — same `registerTool` semantics as the TS version.
        let tool = ToolDef::builder("hello", "Say hello and write to output.txt")
            .arg("name", ArgType::String, "Name to greet")
            .tier(ToolTier::Write)
            .execute(|_call_id, args, _ctx| async move {
                let name = args.get_string("name")?;
                let greeting = format!("Hello, {name}!");
                // Capability: fs_write on output.txt is declared, gate allows.
                tokio::fs::write("output.txt", &greeting).await
                    .map_err(PluginError::from)?;
                Ok(ToolOutput::text(greeting))
            })
            .build();

        api.register_tool(tool);

        // Subscribe to a lifecycle event — block bash commands with rm -rf.
        api.on("before_tool_call", |event| async move {
            if event.tool_name == "bash"
                && event.args.get_string("command")
                       .map(|c| c.contains("rm -rf"))
                       .unwrap_or(false)
            {
                return Ok(EventResponse::Block { reason: "rm -rf is not allowed by hello-plugin".into() });
            }
            Ok(EventResponse::Pass)
        });

        Ok(())
    }
}

// Self-register at link time. `inventory::submit!` makes this descriptor
// discoverable by `inventory::iter::<PluginDescriptor>()` from anywhere in
// the binary. Same pattern as `axum`, `metrics`, `tower`.
inventory::submit! {
    PluginDescriptor::new(
        "jcode-ext-hello",
        HelloPlugin::register_manifest_and_handler,
    )
}
```

To enable/disable this plugin in the host, the user edits `~/.jcode/config.toml`:

```toml
[plugins.workspace]
"jcode-ext-hello" = { enabled = true }
# "jcode-ext-other" = { enabled = false }
```

No `jcode plugin install` step. The crate is a workspace member, so `cargo build` links it in. The host enables/disables at startup by checking the config flag. This is the **fastest iteration loop** for Rust plugin authors: edit `.rs` → `cargo build` → restart jcode → test.

### File: `docs/plugins.md` (new)

Modeled on omp's `docs/extensions.md` (417 lines). Covers:
1. Quick start (copy hello-plugin, install, run)
2. Manifest schema reference
3. `ExtensionAPI` reference (registerTool, registerCommand, on, sendMessage)
4. Lifecycle events reference (before_tool_call, after_tool_call, session_start, ...)
5. Capability model (fs_read, fs_write, http_hosts, env_read, shell_commands, requires_tools)
6. Approval model (ToolTier, ApprovalOverride, mode interaction)
7. Examples (one tool, one hook, one command)
8. Testing (jcode-plugin-runtime's `integration_tests.rs` pattern)
9. Distribution (npm, git, local path, link)
10. Migration from v1
11. Security checklist (STRIDE threat model)
12. Troubleshooting (common errors, kill switches, debug logs)

### File: `docs/plugin-threat-model.md` (new)

STRIDE threat model, modeled on pia's `docs/extension-runtime-threat-model.md` (146 lines). Categories:
- **S**poofing: plugin claims to be a different package → mitigated by package_name uniqueness check, sha256 fingerprint at install.
- **T**ampering: plugin mutates files outside declared scope → mitigated by FsConnector scope check, audit trail.
- **R**epudiation: plugin denies it called a tool → mitigated by AuditTrail, every tool call logged with plugin_id + tier + args hash.
- **I**nformation disclosure: plugin reads env vars outside declared list → mitigated by env_read capability check, SecretBroker-style redaction in logs.
- **D**enial of service: plugin floods with host calls → mitigated by max_hostcalls_per_sec quota, max_tool_duration_secs timeout.
- **E**levation of privilege: plugin registers tools with elevated tier → mitigated by declared_tier() being immutable after registration, preflight static analysis detecting suspicious patterns.

Each threat has: description, attack scenario, mitigation, test reference (pointing at the integration test that verifies the mitigation).

---

## 6. Configuration & Wiring

### `~/.jcode/config.toml` additions

```toml
[plugins]
# Mode for the capability chain. Strict | Permissive | Prompt (default) | Disabled.
# Strict = deny by default. Permissive = allow by default. Prompt = ask for ambiguous.
# Disabled = all plugin calls denied (global kill switch).
mode = "prompt"

# Per-tool user override. Key is the tool name; value is allow|deny|prompt.
# This is loaded into ApprovalGate.user_overrides.
[plugins.approval]
read = "allow"       # always allow read tools
edit = "allow"       # always allow edit
bash = "prompt"      # always prompt for bash
"my-plugin:hello" = "allow"  # allow this specific plugin tool

# Per-plugin deny list (highest priority in the capability chain).
# Plugins listed here cannot register tools that match these patterns.
[plugins.deny]
tools = ["dangerous-tool", "*-destructive"]
shell_commands = ["rm -rf /*", "dd if=/dev/zero"]
env_read = ["AWS_SECRET_ACCESS_KEY"]
http_hosts = ["169.254.169.254"]  # AWS metadata service

# Per-plugin allow list. Tool names here are auto-approved for the named plugin.
[plugins.allow]
"jcode-hello-plugin" = ["hello"]
"jcode-grep-plugin" = ["grep"]

# Per-extension kill switch. Setting this disables the plugin at load time.
# Mirrors JCODE_PLUGIN_KILL_<name>=1 env var.
[plugins.kill]
"some-plugin" = true

# Workspace-crate plugins (compiled into the jcode binary, discovered via
# the `inventory` crate). Keyed by crate name. Enabled by default; toggle
# off here to exclude from the registry without removing the crate.
[plugins.workspace]
"jcode-ext-hello"   = { enabled = true }
"jcode-ext-grep"    = { enabled = true }
"jcode-ext-noisy"   = { enabled = false }  # excluded without recompile

# Per-plugin settings (override defaults declared in the plugin's manifest).
# Type-checked against the manifest's `settings` schema if present.
[plugins.settings."jcode-ext-grep"]
case_insensitive = true
max_results = 200
```

### `~/.jcode/plugins/installed.json` schema

The on-disk state file for `PluginManager`:

```json
{
  "schema": "jcode-plugin-state.v1",
  "installed": {
    "jcode-hello-plugin": {
      "manifest": { "schema": "jcode-plugin.v2", "name": "Hello Plugin", "package_name": "jcode-hello-plugin", "version": "0.1.0", ... },
      "source": { "kind": "local", "path": "/Users/foo/.jcode/plugins/jcode-hello-plugin" },
      "install_path": "/Users/foo/.jcode/plugins/jcode-hello-plugin",
      "installed_at": "2026-06-18T10:30:00Z",
      "enabled": true,
      "settings": { "greeting_prefix": "Howdy" }
    }
  }
}
```

### Environment variables

| Var | Effect |
|-----|--------|
| `JCODE_PLUGIN_KILL_ALL=1` | Disable all plugins (existing) |
| `JCODE_PLUGIN_FORCE_DENY=1` | Force-deny all plugin tool calls (existing) |
| `JCODE_PLUGIN_SKIP_HOOKS=1` | Skip plugin event handlers (existing) |
| `JCODE_PLUGIN_KILL_<UPPERCASE_NAME>=1` | Kill switch for a specific plugin (new) |
| `JCODE_PLUGIN_LOG=trace` | Trace-level logging for plugin subsystem (new) |
| `JCODE_PLUGIN_AUDIT_PATH=/path/audit.log` | Override audit log path (new) |

### CLI additions (`jcode plugin` subcommand)

**Three subcommands, one per distribution path. No `npm`, no `npx`, no registry lookup anywhere.**

```
# Local path: load a plugin from a directory or .ts/.js file
jcode plugin load ./plugins/my-plugin           # load a directory (with package.json + index.ts)
jcode plugin load ./plugins/my-plugin/index.ts  # load a single file
jcode plugin load ~/code/my-plugin

# Git: clone to ~/.jcode/plugins/<name>/ then load as local
jcode plugin clone https://github.com/foo/bar.git
jcode plugin clone https://github.com/foo/bar.git --rev v1.2.3
jcode plugin clone git@github.com:foo/bar.git

# Workspace crate: nothing to do — already compiled in. Just toggle on/off.
# (The host scans inventory::iter::<PluginDescriptor>() at startup.)
jcode plugin list --kind workspace             # show all linked-in workspace crates
jcode plugin info jcode-ext-hello              # show manifest + capabilities

# Common operations on all installed plugins
jcode plugin list                              # show all (local + git-cloned + workspace)
jcode plugin reload <name>                     # hot-reload a single plugin
jcode plugin unload <name>                     # remove a previously-loaded plugin
jcode plugin enable <name>                     # re-enable a disabled plugin
jcode plugin disable <name>                    # disable without unloading
```

The `load` command copies (or symlinks with `--symlink`) the source into `~/.jcode/plugins/<name>/` so subsequent runs are stable. The `clone` command does the same with `git clone`. The `workspace` crates are discovered via `inventory` at link time and gated by `[plugins.workspace]` in config.toml.

**No `jcode plugin install <npm-spec>`. No `jcode plugin marketplace`. The plugin source always originates from local files or a git clone the user explicitly invokes.**

### Integration with `jcode-app-core`

`jcode-app-core` already wires `RcuDispatcher` into the agent loop. The new wiring:
1. On startup, `PluginManager::load_state` reads `~/.jcode/plugins/installed.json`.
2. The host scans `inventory::iter::<PluginDescriptor>()` to find all workspace-crate plugins compiled into the binary. Enabled ones (per `[plugins.workspace]`) are registered.
3. For each enabled local/git-cloned plugin, `PluginLoader::load` instantiates the QuickJS context, runs preflight, calls the plugin's default export, captures `registerTool`/`on` calls, and inserts into the registry.
4. `ApprovalGate` is constructed from the merged `[plugins.approval]` + `[plugins.deny]` + `[plugins.allow]` + `mode` from config.toml.
5. `RcuDispatcher` holds a reference to `ApprovalGate`; `dispatch` calls `gate.check` first.
6. The agent loop's tool-call handler uses `RcuDispatcher::dispatch` for every tool (built-in, workspace-crate, or local/git-cloned plugin). The audit of every call goes to `~/.jcode/logs/jcode-YYYY-MM-DD.log` (existing) and optionally to `JCODE_PLUGIN_AUDIT_PATH` if set.

---

## 7. Repo References

| Feature Aspect | Repo | File | Link |
|----------------|------|------|------|
| `ToolTier` enum | oh-my-pi | `packages/agent/src/types.ts:477` | https://github.com/can1357/oh-my-pi/blob/main/packages/agent/src/types.ts#L477-L489 |
| `ExtensionToolWrapper` chokepoint | oh-my-pi | `packages/coding-agent/src/extensibility/extensions/wrapper.ts:113` | https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L113-L179 |
| `ExtensionAPI` author surface | oh-my-pi | `packages/coding-agent/src/extensibility/extensions/types.ts:941` | https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/extensions/types.ts#L941-L1171 |
| Lifecycle event ordering (tool_call blockable, tool_result mutable) | oh-my-pi | `packages/coding-agent/src/extensibility/extensions/runner.ts:597` | https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/extensions/runner.ts#L597-L695 |
| Per-handler timeout (30s/2s) | oh-my-pi | `packages/coding-agent/src/extensibility/extensions/runner.ts:202` | https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/extensions/runner.ts#L202-L983 |
| `PluginManager` install/uninstall/list/link | oh-my-pi | `packages/coding-agent/src/extensibility/plugins/manager.ts:113` | https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/extensibility/plugins/manager.ts#L113-L700 |
| Plugin lockfile format | oh-my-pi | docs `plugin-manager-installer-plumbing.md` | https://github.com/can1357/oh-my-pi/blob/main/docs/plugin-manager-installer-plumbing.md |
| Plugin author guide | oh-my-pi | `docs/extensions.md:1` | https://github.com/can1357/oh-my-pi/blob/main/docs/extensions.md |
| 5-layer capability precedence | pi-agent-rust | `src/extensions.rs:2146` | https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/src/extensions.rs#L2146-L2162 |
| `ExtensionPolicyMode` (Strict/Prompt/Permissive) | pi-agent-rust | `src/extensions.rs:1834` | https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/src/extensions.rs#L1834-L1840 |
| `ToolEffects` parallel-safety declaration | pi-agent-rust | `src/tools.rs:38` | https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/src/tools.rs#L38-L155 |
| `WasmExtensionToolWrapper` timeout pattern | pi-agent-rust | `src/extension_tools.rs:206` | https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/src/extension_tools.rs#L206-L263 |
| STRIDE threat model template | pi-agent-rust | `docs/extension-runtime-threat-model.md` | https://github.com/Dicklesworthstone/pi_agent_rust/blob/main/docs/extension-runtime-threat-model.md |
| `engines.<runtime>` semver compat | opencode | `packages/opencode/src/plugin/shared.ts:36` (checkPluginCompatibility) | https://github.com/anomalyco/opencode/blob/main/packages/opencode/src/plugin/shared.ts#L36 |
| Fingerprint-based hot-reload seed | opencode | `packages/opencode/src/plugin/meta.ts` | https://github.com/anomalyco/opencode/blob/main/packages/opencode/src/plugin/meta.ts |
| immer `Draft` for safe hook output mutation | opencode | `packages/core/src/plugin.ts:136-167` | https://github.com/anomalyco/opencode/blob/main/packages/core/src/plugin.ts#L136-L167 |
| Two-layer server + TUI split | opencode | `packages/opencode/specs/tui-plugins.md` | https://github.com/anomalyco/opencode/blob/main/packages/opencode/specs/tui-plugins.md |
| TUI plugin API (`keymap`, `slots`, `route`) | opencode | `packages/plugin/src/tui.ts:1` | https://github.com/anomalyco/opencode/blob/main/packages/plugin/src/tui.ts |
| Existing `CapabilityChain` (4 layers) | jcode | `crates/jcode-plugin-core/src/security.rs` | (current file in this repo) |
| Existing `RcuDispatcher` | jcode | `crates/jcode-plugin-runtime/src/dispatcher.rs` | (current file in this repo) |
| Existing `AuditTrail` | jcode | `crates/jcode-plugin-runtime/src/audit.rs` | (current file in this repo) |
| Existing `TuiPluginApi` + `SlotRegistry` | jcode | `crates/jcode-plugin-runtime/src/tui_api.rs`, `tui_system.rs` | (current files in this repo) |
| Existing permission modes | jcode | `crates/jcode-tui-permissions/src/lib.rs` | (current file in this repo) |

---

## 8. Test Cases

Tests go in the existing test modules: `crates/jcode-plugin-core/src/tests.rs`, `crates/jcode-plugin-runtime/src/integration_tests.rs`. Use the existing fixture patterns.

### Happy Path

```rust
// crates/jcode-plugin-runtime/src/integration_tests.rs

#[tokio::test]
async fn dispatch_allows_read_tier_in_accept_edits_mode() {
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::AcceptEdits, HashMap::new());
    let args = json!({ "path": "/tmp/foo.txt" });
    let decision = gate.check("read", ToolTier::Read, &args);
    assert_eq!(decision, GateDecision::Allow, "read should auto-approve in AcceptEdits");
}

#[tokio::test]
async fn dispatch_prompts_exec_tier_in_accept_edits_mode() {
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::AcceptEdits, HashMap::new());
    let args = json!({ "command": "ls -la" });
    let decision = gate.check("bash", ToolTier::Exec, &args);
    assert!(matches!(decision, GateDecision::NeedsApproval(_)),
        "exec should prompt in AcceptEdits, got {:?}", decision);
}

#[tokio::test]
async fn dispatch_auto_approves_all_in_bypass_mode() {
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::BypassPermissions, HashMap::new());
    for tier in [ToolTier::Read, ToolTier::Write, ToolTier::Exec] {
        let decision = gate.check("any_tool", tier, &json!({}));
        assert_eq!(decision, GateDecision::Allow, "BypassPermissions should allow {:?}", tier);
    }
}

#[tokio::test]
async fn dispatch_prompts_all_in_plan_mode() {
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::Plan, HashMap::new());
    for tier in [ToolTier::Read, ToolTier::Write, ToolTier::Exec] {
        let decision = gate.check("any_tool", tier, &json!({}));
        assert!(matches!(decision, GateDecision::NeedsApproval(_)),
            "Plan should prompt for {:?}", tier);
    }
}

#[tokio::test]
async fn user_override_allow_wins_over_mode() {
    let mut overrides = HashMap::new();
    overrides.insert("bash".into(), ApprovalOverride::Allow);
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::Plan, overrides);
    let decision = gate.check("bash", ToolTier::Exec, &json!({}));
    assert_eq!(decision, GateDecision::Allow);
}

#[tokio::test]
async fn user_override_deny_wins_over_mode() {
    let mut overrides = HashMap::new();
    overrides.insert("bash".into(), ApprovalOverride::Deny);
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::BypassPermissions, overrides);
    let decision = gate.check("bash", ToolTier::Exec, &json!({}));
    assert!(matches!(decision, GateDecision::Deny { .. }));
}
```

### Capability Chain 5-Layer Tests

```rust
// crates/jcode-plugin-core/src/tests.rs

#[test]
fn chain_layer1_plugin_deny_wins() {
    let mut plugin_deny = CapabilitySet::default();
    plugin_deny.tools.push("bash".into());
    let chain = CapabilityChainV2 {
        plugin_deny, plugin_allow: CapabilitySet::default(),
        global_deny: CapabilitySet::default(), global_allow: CapabilitySet::default(),
        mode: PolicyMode::Permissive, // would allow
        global_default: Some(AccessDefault::Allow), // would allow
    };
    let decision = chain.check("tool:bash", &CapabilityAction::Execute);
    assert!(matches!(decision, AccessDecisionV2::Deny { layer: 1, .. }));
}

#[test]
fn chain_layer2_global_deny_wins() {
    let mut global_deny = CapabilitySet::default();
    global_deny.tools.push("bash".into());
    let chain = CapabilityChainV2 {
        plugin_deny: CapabilitySet::default(),
        global_deny,
        plugin_allow: CapabilitySet::default(), global_allow: CapabilitySet::default(),
        mode: PolicyMode::Permissive, global_default: Some(AccessDefault::Allow),
    };
    let decision = chain.check("tool:bash", &CapabilityAction::Execute);
    assert!(matches!(decision, AccessDecisionV2::Deny { layer: 2, .. }));
}

#[test]
fn chain_layer3_plugin_allow_wins_over_mode() {
    let mut plugin_allow = CapabilitySet::default();
    plugin_allow.tools.push("my-tool".into());
    let chain = CapabilityChainV2 {
        plugin_deny: CapabilitySet::default(),
        global_deny: CapabilitySet::default(),
        plugin_allow,
        global_allow: CapabilitySet::default(),
        mode: PolicyMode::Strict, // would deny
        global_default: None,
    };
    let decision = chain.check("tool:my-tool", &CapabilityAction::Read);
    assert!(matches!(decision, AccessDecisionV2::Allow { layer: 3, .. }));
}

#[test]
fn chain_layer4_global_allow_wins_over_strict_mode() {
    let mut global_allow = CapabilitySet::default();
    global_allow.tools.push("read".into());
    let chain = CapabilityChainV2 {
        plugin_deny: CapabilitySet::default(),
        global_deny: CapabilitySet::default(),
        plugin_allow: CapabilitySet::default(),
        global_allow,
        mode: PolicyMode::Strict,
        global_default: None,
    };
    let decision = chain.check("tool:read", &CapabilityAction::Read);
    assert!(matches!(decision, AccessDecisionV2::Allow { layer: 4, .. }));
}

#[test]
fn chain_layer5_strict_mode_denies_unknown() {
    let chain = CapabilityChainV2::default(); // mode = Prompt, but let's override
    let chain = CapabilityChainV2 { mode: PolicyMode::Strict, ..chain };
    let decision = chain.check("tool:unknown", &CapabilityAction::Read);
    assert!(matches!(decision, AccessDecisionV2::Deny { layer: 5, .. }));
}

#[test]
fn chain_layer5_permissive_mode_allows_unknown() {
    let chain = CapabilityChainV2 { mode: PolicyMode::Permissive, ..CapabilityChainV2::default() };
    let decision = chain.check("tool:unknown", &CapabilityAction::Read);
    assert!(matches!(decision, AccessDecisionV2::Allow { layer: 5, .. }));
}

#[test]
fn chain_disabled_mode_denies_everything() {
    let chain = CapabilityChainV2 { mode: PolicyMode::Disabled, ..CapabilityChainV2::default() };
    let decision = chain.check("tool:read", &CapabilityAction::Read);
    assert!(matches!(decision, AccessDecisionV2::Deny { layer: 5, .. }));
}
```

### Plugin Manager Tests

```rust
// crates/jcode-plugin-core/src/manager.rs tests

#[tokio::test]
async fn install_from_local_path() {
    let tmp = tempdir().unwrap();
    let plugin_dir = tmp.path().join("my-plugin");
    std::fs::create_dir(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("package.json"), r#"{
        "jcode-plugin": { "v2": {
            "schema": "jcode-plugin.v2", "name": "My", "package_name": "my-plugin",
            "version": "0.1.0", "kind": "server", "entry": { "server": "index.js" },
            "tier": "read", "capabilities": {}, "approval": { "kind": "default" },
            "engines": { "jcode": ">=0.29" }
        }}
    }"#).unwrap();
    std::fs::write(plugin_dir.join("index.js"), "module.exports = {};").unwrap();

    let mgr = PluginManager::new(tmp.path().join("install_root")).await.unwrap();
    let installed = mgr.install(PluginSource::Local { path: plugin_dir.clone() }).await.unwrap();
    assert_eq!(installed.manifest.package_name, "my-plugin");
    assert!(installed.install_path.exists());
    assert!(mgr.list(true).await.iter().any(|p| p.manifest.package_name == "my-plugin"));
}

#[tokio::test]
async fn install_rolls_back_on_preflight_failure() {
    let tmp = tempdir().unwrap();
    let plugin_dir = tmp.path().join("bad-plugin");
    std::fs::create_dir(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("package.json"), r#"{ bad json"#).unwrap(); // invalid manifest

    let mgr = PluginManager::new(tmp.path().join("install_root")).await.unwrap();
    let result = mgr.install(PluginSource::Local { path: plugin_dir }).await;
    assert!(result.is_err());
    assert!(mgr.list(true).await.is_empty(), "rollback should leave no installed plugin");
}

#[tokio::test]
async fn uninstall_is_idempotent() {
    let mgr = PluginManager::new(tempdir().unwrap().path().join("ir")).await.unwrap();
    // Never installed
    mgr.uninstall("nonexistent").await.unwrap();
    // Install then uninstall
    mgr.install(PluginSource::Local { path: make_minimal_plugin("foo") }).await.unwrap();
    mgr.uninstall("foo").await.unwrap();
    mgr.uninstall("foo").await.unwrap(); // idempotent
    assert!(!mgr.list(true).await.iter().any(|p| p.manifest.package_name == "foo"));
}

#[tokio::test]
async fn link_creates_symlink() {
    let src = make_minimal_plugin("linked");
    let mgr = PluginManager::new(tempdir().unwrap().path().join("ir")).await.unwrap();
    let installed = mgr.link(&src).await.unwrap();
    assert!(installed.install_path.is_symlink(), "link should be a symlink");
}

#[tokio::test]
async fn engines_compat_rejects_mismatch() {
    let mut manifest = minimal_manifest("incompat");
    manifest.engines.jcode = Some(">=99.0.0".into()); // we are 0.29
    let plugin_dir = make_plugin_with_manifest(manifest);
    let mgr = PluginManager::new(tempdir().unwrap().path().join("ir")).await.unwrap();
    let result = mgr.install(PluginSource::Local { path: plugin_dir }).await;
    assert!(matches!(result, Err(PluginError::EnginesMismatch { .. })));
}
```

### Hot Reload Tests

```rust
// crates/jcode-plugin-runtime/src/integration_tests.rs

#[tokio::test]
async fn reload_no_op_when_unchanged() {
    let (loader, plugin_id, plugin_path) = setup_minimal_plugin().await;
    loader.reload(&plugin_id).await.unwrap();
    // Second reload with same file should be a no-op (no error, no rebuild).
    let initial_count = loader.instantiation_count(&plugin_id).await;
    loader.reload(&plugin_id).await.unwrap();
    assert_eq!(initial_count, loader.instantiation_count(&plugin_id).await);
}

#[tokio::test]
async fn reload_picks_up_file_changes() {
    let (loader, plugin_id, plugin_path) = setup_minimal_plugin().await;
    let initial_tool_count = loader.tools_for(&plugin_id).await.len();
    // Add a new tool to the plugin source
    std::fs::write(&plugin_path, r#"
        module.exports = {
            register(pi) {
                pi.registerTool({ name: "tool1", execute: async () => ({ content: [] }) });
                pi.registerTool({ name: "tool2", execute: async () => ({ content: [] }) });
            }
        };
    "#).unwrap();
    loader.reload(&plugin_id).await.unwrap();
    assert_eq!(loader.tools_for(&plugin_id).await.len(), initial_tool_count + 1);
}

#[tokio::test]
async fn reload_rolls_back_on_transpile_failure() {
    let (loader, plugin_id, plugin_path) = setup_minimal_plugin().await;
    let old_count = loader.tools_for(&plugin_id).await.len();
    // Write invalid TypeScript that won't transpile
    std::fs::write(&plugin_path, "this is not valid typescript !!!").unwrap();
    let result = loader.reload(&plugin_id).await;
    assert!(result.is_err());
    // Old tools still present
    assert_eq!(loader.tools_for(&plugin_id).await.len(), old_count);
}
```

### Edge Cases

```rust
#[tokio::test]
async fn gate_deny_returns_actionable_error() {
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::BypassPermissions, HashMap::new());
    // Set up a deny at layer 1
    // ... (omitted: chain setup)
    let decision = gate.check("bash", ToolTier::Exec, &json!({"command": "rm -rf /"}));
    if let GateDecision::Deny { reason, layer } = decision {
        assert!(reason.contains("Denied by"));
        assert_eq!(layer, "plugin_deny");
    } else {
        panic!("expected deny");
    }
}

#[tokio::test]
async fn empty_args_does_not_panic() {
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::Plan, HashMap::new());
    let _ = gate.check("any", ToolTier::Read, &json!({}));
}

#[tokio::test]
async fn concurrent_dispatch_serializes_writes() {
    // Two parallel calls to a Write-tier tool should be serialized per-plugin.
    let dispatcher = setup_test_dispatcher().await;
    let handles: Vec<_> = (0..10)
        .map(|i| dispatcher.dispatch("write_tool", json!({"i": i}), &format!("c{}", i), test_ctx()))
        .collect();
    let results: Vec<_> = futures::future::join_all(handles).await;
    for r in results { assert!(r.is_ok()); }
}

#[tokio::test]
async fn large_args_does_not_block() {
    let gate = ApprovalGate::new(CapabilityChainV2::default(), PermissionMode::BypassPermissions, HashMap::new());
    let big = json!({ "data": "x".repeat(1_000_000) }); // 1MB
    let start = std::time::Instant::now();
    let _ = gate.check("bash", ToolTier::Exec, &big);
    assert!(start.elapsed() < Duration::from_millis(10));
}

#[tokio::test]
async fn timeout_kills_long_running_tool() {
    let dispatcher = setup_test_dispatcher_with_short_timeout(2).await;
    let result = dispatcher.dispatch("slow_tool", json!({}), "c1", test_ctx()).await;
    assert!(matches!(result, Err(DispatchError::Timeout(_, 2))));
}

#[tokio::test]
async fn per_extension_kill_switch_blocks_load() {
    std::env::set_var("JCODE_PLUGIN_KILL_MY_PLUGIN", "1");
    let loader = setup_test_loader().await;
    let result = loader.load(PluginSource::Local { path: make_minimal_plugin("my-plugin") }).await;
    assert!(matches!(result, Err(LoaderError::Killed("my-plugin"))));
    std::env::remove_var("JCODE_PLUGIN_KILL_MY_PLUGIN");
}
```

### Integration Tests

```rust
#[tokio::test]
async fn end_to_end_install_then_dispatch() {
    // 1. Install hello-plugin from examples/plugins/hello-plugin/
    let mgr = PluginManager::new(tempdir().unwrap().path().join("ir")).await.unwrap();
    let installed = mgr.install(PluginSource::Local {
        path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins/hello-plugin"),
    }).await.unwrap();

    // 2. Wire it into the dispatcher
    let loader = PluginLoader::new(...);
    loader.load_from(installed).await.unwrap();
    let dispatcher = RcuDispatcher::new(loader.registry(), gate);

    // 3. Dispatch a call to the plugin-registered tool
    let result = dispatcher.dispatch("hello", json!({"name": "world"}), "c1", test_ctx()).await.unwrap();
    let content = &result.content[0];
    if let ContentBlock::Text { text } = content {
        assert!(text.contains("Hello, world"));
    } else {
        panic!("expected text content");
    }

    // 4. Verify audit log has the call
    let audit = mgr.audit_trail();
    let entries = audit.entries_for("hello", last_5_minutes()).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tier, ToolTier::Write);
    assert!(!entries[0].is_error);
}
```

---

## 9. Benchmarks

### What to Measure

| Metric | Baseline (today) | Target | How to Measure |
|--------|------------------|--------|----------------|
| Tool dispatch overhead (no gate) | ~50µs (existing RcuDispatcher) | ≤55µs (added gate) | `benchmarks/dispatch.rs` — 10K calls, p50/p99 |
| Tool dispatch with gate (allow path) | N/A | ≤70µs | same bench with gate enabled |
| Tool dispatch with gate (deny path) | N/A | ≤70µs | same bench with deny configured |
| Tool dispatch with gate (prompt path) | N/A | ≤80µs (no UI call) | same bench, but no UI is called in bench |
| CapabilityChainV2::check (5 layers) | N/A | ≤500ns | `benchmarks/capability.rs` — 1M calls |
| Plugin load time (QuickJS init + transpile + execute) | ~200ms (existing) | ≤250ms (added preflight) | `benchmarks/loader.rs` — 100 loads, p50/p99 |
| Hot reload (file changed) | N/A | ≤100ms | `benchmarks/loader.rs` — 100 reloads |
| Hot reload (file unchanged) | N/A | ≤1ms (fingerprint check only) | same bench with same file |
| Memory per loaded plugin | ~2MB (existing) | ≤3MB (added manifest + audit) | `dhat` heap profiling on 10 plugins |
| SWC transpile time (1KB TS) | ~5ms (existing) | ≤5ms (no change) | `benchmarks/transpiler.rs` |
| SWC transpile time (10KB TS) | ~30ms (existing) | ≤30ms (no change) | same bench |
| Audit log write time | N/A | ≤50µs per entry | `benchmarks/audit.rs` — 10K writes |
| Approval UI prompt time (mocked) | N/A | ≤20ms (mocked) | mock UI that auto-deny after 20ms |

### Benchmark Code

```rust
// crates/jcode-plugin-runtime/benches/dispatch.rs

use criterion::{criterion_group, criterion_main, Criterion};
use jcode_plugin_core::*;
use jcode_plugin_runtime::*;

fn bench_dispatch_no_gate(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dispatcher = rt.block_on(setup_test_dispatcher_no_gate());
    c.bench_function("dispatch_no_gate", |b| {
        b.to_async(&rt).iter(|| async {
            dispatcher.dispatch("read", serde_json::json!({"path": "/tmp/x"}), "c1", test_ctx()).await.unwrap();
        });
    });
}

fn bench_dispatch_with_gate_allow(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dispatcher = rt.block_on(setup_test_dispatcher_with_gate(PermissionMode::BypassPermissions));
    c.bench_function("dispatch_with_gate_allow", |b| {
        b.to_async(&rt).iter(|| async {
            dispatcher.dispatch("read", serde_json::json!({"path": "/tmp/x"}), "c1", test_ctx()).await.unwrap();
        });
    });
}

fn bench_capability_chain_5_layers(c: &mut Criterion) {
    use jcode_plugin_core::CapabilityChainV2;
    let chain = CapabilityChainV2 {
        plugin_deny: CapabilitySet::default(),
        global_deny: CapabilitySet::default(),
        plugin_allow: CapabilitySet::default(),
        global_allow: CapabilitySet::default(),
        mode: PolicyMode::Prompt,
        global_default: None,
    };
    c.bench_function("capability_chain_5_layers", |b| {
        b.iter(|| chain.check("tool:read", &CapabilityAction::Read));
    });
}

fn bench_plugin_load(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let loader = rt.block_on(setup_test_loader());
    c.bench_function("plugin_load", |b| {
        b.to_async(&rt).iter(|| async {
            loader.load(PluginSource::Local { path: make_minimal_plugin("bench") }).await.unwrap();
        });
    });
}

criterion_group!(benches, bench_dispatch_no_gate, bench_dispatch_with_gate_allow, bench_capability_chain_5_layers, bench_plugin_load);
criterion_main!(benches);
```

### Measurement Method

- **Latency:** criterion's `iter_batched` with 100 samples for warmup, 1000 samples for measurement. Report p50, p99, p99.9.
- **Memory:** `dhat` heap profiler on 10 plugins loaded, report peak heap + allocations.
- **Throughput:** `tokio::time::Instant::now()` around N concurrent `dispatch` calls, divide.
- **Compile time:** `cargo clean && cargo build --release 2>&1 | grep "Finished"`; baseline before change vs after.
- **CI integration:** Add `cargo bench --bench dispatch --bench capability --bench loader` to a new `bench.yml` workflow; fail if p99 latency regresses >10% vs the stored baseline.

---

## 10. Migration / Rollout

### Strategy

The plan is **additive + opt-in** for v1 plugins, **breaking** for v2 manifest schema. The dev stage allows the breaking change.

### Step-by-step

1. **Phase 0 (1 day):** Add `ToolTier` enum to `jcode-tool-types`. Add `declared_tier()` default method to `Tool` trait. No existing tools set it; they all get `None` → fall back to manifest tier → fall back to `Exec` (fail-closed).

2. **Phase 1 (2 days):** Add `CapabilityChainV2` alongside existing `CapabilityChain` (deprecate the old one but keep it compiling). All existing tests pass unchanged.

3. **Phase 2 (3 days):** Add `ApprovalGate` + `PluginManager`. Wire into `RcuDispatcher::dispatch`. Existing tools are still in the registry but go through the gate. The default gate config (`mode: Prompt`, no overrides) means: Plan mode prompts for everything, AcceptEdits prompts for Exec only, BypassPermissions allows everything — same as today's behavior. Audit log gets a new structured format.

4. **Phase 3 (2 days):** Bump manifest schema to `jcode-plugin.v2`. `PluginManifest::from_package_json` auto-migrates v1 → v2 with `tier: Exec` (fail-closed for v1). Add `migrate_v1_to_v2` unit test.

5. **Phase 4 (2 days):** Add `PluginLoader::reload` for hot-reload. Wire the `JCODE_PLUGIN_LOG=trace` env var. Add per-extension kill switch.

6. **Phase 5 (2 days):** Build the example plugin (`examples/plugins/hello-plugin/`). Write `docs/plugins.md` (≥200 lines). Write `docs/plugin-threat-model.md` (STRIDE, ~150 lines).

7. **Phase 6 (1 day):** Add CLI subcommand `jcode plugin install/uninstall/list/enable/disable/link/reload/info`. Wire into existing `cli` module.

8. **Phase 7 (1 day):** Integration testing — install hello-plugin from a real `~/.jcode/plugins/` path, run jcode, verify the tool appears in the next session.

9. **Phase 8 (1 day):** Update `AGENTS.md` to mention the new plugin system, new CLI subcommand, new env vars, new docs files.

Total: ~14 dev days for v2.0.0. Each phase ships as a separate PR.

### Deprecation Path

- `CapabilityChain` (4-layer) → keep working for 1 minor version, log deprecation warning, replace internals with call to `CapabilityChainV2`. Remove in v3.0.
- v1 manifest schema → auto-migrated to v2, no log warning needed (silent upgrade).
- `ToolEffects` (existing free-form enum) → map to `ToolTier` for the gate, keep `ToolEffects` for the concurrency limiter.

### Backward Compat for Plugin Authors

- A v1 plugin keeps working, with `tier: Exec` (fail-closed). Plugin authors can opt into a less-restrictive tier by adding `jcode-plugin.v2.tier: read|write|exec` to their manifest.
- A plugin that used to work in `BypassPermissions` mode will now go through the gate. If the gate denies, the plugin stops working. This is the desired behavior — surfaces misconfigured plugins.

---

## 11. Known Limitations & Future Work

### v2 (this plan)

- ✅ Custom plugin authoring with `ExtensionAPI`-style surface (both TS via QuickJS and Rust via workspace-crate + `inventory`)
- ✅ `ToolTier` + `ApprovalGate` + `CapabilityChainV2` (5 layers)
- ✅ `PluginManager` with load/clone/unload/rollback for local path + git clone
- ✅ Rust workspace-crate path (compile in, register via `inventory`, toggle via `[plugins.workspace]`)
- ✅ Hot reload for local-path plugins
- ✅ Per-extension kill switch (`JCODE_PLUGIN_KILL_<NAME>=1` or `[plugins.kill]`)
- ✅ STRIDE threat model + author docs + example plugin (TS + Rust)

### v2.1 (stretch)

- [ ] `cdylib` plugin path via `libloading` — for trusted external Rust plugins that want to be shipped as a `.so`/`.dylib`/`.dll` without being a workspace member. No registry; the user points `jcode plugin load ./my-plugin.so` at a file. Same `ExtensionAPI`, same `ToolTier`, same `CapabilityChainV2`.
- [ ] Per-tool `formatApprovalDetails` UI polish (show truncated command for bash, diff preview for edit)
- [ ] Plugin signing (sha256 of manifest + signature field; load refuses unsigned unless `--allow-unsigned`)
- [ ] Pluggable `host.call`-style JSON RPC for plugin→host communication (pia pattern), so plugins can be migrated to WASM later without API change

### v3 (future)

- [ ] WASM runtime as a parallel path (behind `wasm-runtime` feature flag), using `wasmtime` + the same WIT-style ABI pia uses. Plugin author writes Rust → `cargo build --target wasm32-wasip2` → load `.wasm` file. No registry, no npm.
- [ ] Per-call streaming for long-running tools (pia's `streaming-hostcalls.md` pattern)
- [ ] Reactive state primitive (RxJS-style observable store shared between plugins)
- [ ] Plugin dependency resolution (`requires_tools: ["grep"]` → load-time check; if a workspace crate provides it, use it; if a local path provides it, load it; if neither, fail with actionable error)
- [ ] Versioned API contract (`api_version: "2.0"` field, host refuses to load mismatched plugins)

### Not Covered

- **npm distribution** — explicitly rejected per user policy. No `npm install jcode-plugin-foo`, no `npx`, no `package.json` registry lookups, no `npm publish` flow. The plugin source always originates from local files or a git clone the user explicitly invokes.
- **Marketplace / plugin registry** — explicitly rejected. The user keeps full control of which plugins exist in their environment. There is no central catalog, no curated list, no install count.
- Plugin→plugin direct calls (plugins can only interact via the event bus)
- Sandboxed subprocess execution (QuickJS has no subprocess; bash tool goes through `dcg-core` for safety)
- Plugin-distributed themes (jcode's TUI has its own theming; not a plugin concern)
- Plugin-distributed MCP servers (MCP servers are config-only in jcode today; could be a v2.1 stretch)

---

## 12. Success Criteria Checklist

- [ ] `ToolTier` enum exists in `jcode-plugin-core` and is re-exported from `jcode-tool-types`
- [ ] `CapabilityChainV2` has 5 layers and the new `AccessDecisionV2` return type
- [ ] All 5-layer chain unit tests pass
- [ ] `ApprovalGate::check` is called for every tool call (audit by grep: no `tool.execute(` call sites outside `RcuDispatcher::dispatch`)
- [ ] `PluginManager` has `load_local/clone_git/list_unload/enable/disable/persist_state/load_state` (no `install_npm`, no `marketplace_*`)
- [ ] All `PluginManager` tests pass (local-path happy path, git-clone happy path, rollback on failure, idempotent unload, engines compat)
- [ ] `PluginLoader::reload` works for unchanged files (no-op) and changed files (re-transpile + swap)
- [ ] `JCODE_PLUGIN_KILL_<NAME>=1` blocks plugin load (test exists and passes)
- [ ] Example TS plugin (`examples/plugins/hello-plugin/`) loads via `jcode plugin load ./examples/plugins/hello-plugin` and registers a working tool
- [ ] Example Rust workspace-crate plugin (`crates/jcode-ext-hello/`) compiles into the jcode binary, registers via `inventory::submit!`, and its tool is invocable after `cargo build && jcode`
- [ ] `[plugins.workspace]` config section enables/disables workspace-crate plugins without recompile
- [ ] `docs/plugins.md` exists, is ≥200 lines, modeled on omp's `docs/extensions.md`
- [ ] `docs/plugin-threat-model.md` exists, covers all 6 STRIDE categories, points at test references
- [ ] `jcode plugin load/clone/list/unload/enable/disable/reload/info` CLI subcommand works
- [ ] **No npm, no marketplace, no registry** in the entire plugin subsystem (verified by grep: no `npm`, no `registry.npmjs`, no `marketplace` strings in any plugin code or config)
- [ ] p99 tool dispatch overhead (with gate) ≤ 70µs (vs ≤55µs baseline)
- [ ] p99 capability check overhead ≤ 500ns
- [ ] No regression in existing test suite (`cargo test --workspace`)
- [ ] No regression in cargo build time for the workspace
- [ ] Audit log records every tool call with: tool_name, tier, decision, layer, duration_ms, plugin_id
- [ ] Per-tool user override in `[plugins.approval]` works (test: bash = "deny" blocks bash even in BypassPermissions mode)
- [ ] Existing v1 manifests auto-migrate to v2 silently (test: load a v1 manifest, assert no error)
- [ ] `AGENTS.md` updated to mention new plugin system, new CLI subcommand, new env vars, new docs, and the Rust-first / no-npm distribution policy

---

## Appendix A: File Inventory

### New files (10)

| File | LOC est | Purpose |
|------|---------|---------|
| `crates/jcode-plugin-core/src/manager.rs` | ~600 | `PluginManager` load/clone/unload/rollback for local path + git |
| `crates/jcode-plugin-runtime/src/gate.rs` | ~250 | `ApprovalGate` (single chokepoint) |
| `crates/jcode-plugin-core/src/inventory.rs` | ~150 | `PluginDescriptor` + `inventory::submit!` helpers for workspace-crate registration |
| `docs/plugins.md` | ~400 | Plugin author guide (modeled on omp) |
| `docs/plugin-threat-model.md` | ~200 | STRIDE threat model (modeled on pia) |
| `examples/plugins/hello-plugin/package.json` | ~20 | Example TS plugin manifest |
| `examples/plugins/hello-plugin/index.ts` | ~40 | Example TS plugin source |
| `crates/jcode-ext-hello/Cargo.toml` | ~15 | Example Rust workspace-crate manifest |
| `crates/jcode-ext-hello/src/lib.rs` | ~120 | Example Rust workspace-crate plugin source |
| `crates/jcode-plugin-runtime/benches/dispatch.rs` | ~100 | Criterion bench for dispatch overhead |

### Modified files (9)

| File | Changes |
|------|---------|
| `Cargo.toml` (workspace root) | Add `crates/jcode-ext-hello` as workspace member; add `inventory = "0.3"` to `[workspace.dependencies]` |
| `crates/jcode-plugin-core/src/manifest.rs` | Add `ToolTier`, `PluginSchemaVersion`, `PluginApprovalPolicy`, v2 manifest, `migrate_v1_to_v2` |
| `crates/jcode-plugin-core/src/security.rs` | Add `CapabilityChainV2` (5 layers), deprecate `CapabilityChain` |
| `crates/jcode-plugin-core/src/lib.rs` | Re-export new types, add `pub mod manager`, `pub mod inventory` |
| `crates/jcode-plugin-runtime/src/dispatcher.rs` | Wire `ApprovalGate` into `dispatch` |
| `crates/jcode-plugin-runtime/src/loader.rs` | Add `reload`, `fingerprint` |
| `crates/jcode-plugin-runtime/src/server.rs` | Add per-extension kill switch; scan `inventory::iter::<PluginDescriptor>()` at startup |
| `crates/jcode-tool-types/src/lib.rs` | Re-export `ToolTier`, add `declared_tier`/`max_duration_secs` to `Tool` trait |
| `crates/jcode-tui-permissions/src/lib.rs` | Add `auto_approves(tier)` method to `PermissionMode` |

### Unchanged files (reference only)

- `crates/jcode-plugin-core/src/events.rs` — existing event types, used as-is
- `crates/jcode-plugin-core/src/preflight.rs` — existing static analysis, used as-is
- `crates/jcode-plugin-core/src/config.rs` — existing config types, used as-is
- `crates/jcode-plugin-runtime/src/audit.rs` — existing `AuditTrail`, used as-is
- `crates/jcode-plugin-runtime/src/transpiler.rs` — existing SWC transpiler, used as-is
- `crates/jcode-plugin-runtime/src/sandbox.rs` — existing QuickJS sandbox, used as-is
- `crates/jcode-plugin-runtime/src/tui_*.rs` — existing TUI plugin system, used as-is
- `crates/jcode-hooks/src/*` — existing legacy hook system, kept for v1 plugins, will be deprecated in v3

---

## Appendix B: Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Approval gate adds too much overhead | Low | High | Bench before/after; gate is 5 HashMap lookups in worst case |
| v1 manifest migration loses data | Medium | Medium | Round-trip test in `migrate_v1_to_v2`; keep v1 struct around |
| Hot reload race with in-flight tool calls | Medium | High | Atomic swap via RCU snapshot; old instance held until new is in place |
| QuickJS memory leak from reload | Low | Medium | `dhat` profile every release; `seahash`-based fingerprint forces reload on every change |
| Plugin authors forget to declare `tier` | High | Low | Default to `Exec` (fail-closed), log warning on plugin load |
| `engines.jcode` semver too strict | Medium | Low | Use `^0.29` style for jcode (caret means "compatible within 0.x"); document in `docs/plugins.md` |
| **`inventory`-based registration causes link-time conflicts** (two plugins claim the same name) | Medium | Medium | `inventory` returns iter in unspecified order; add a name-uniqueness check at host startup, fail-fast on conflict |
| **Git-cloned plugin has build step the host doesn't run** (e.g. needs `npm install` or `cargo build`) | High | Medium | Document the convention in `docs/plugins.md`: plugins must ship a pre-built `index.js` for JS, or a Rust source tree the workspace can `cargo build` if it's a separate Cargo project. The host's `jcode plugin clone` runs `npm install` (or `cargo build`) if a `package.json`/`Cargo.toml` is present. |
| **cdylib stretch goal causes scope creep** | High | High | Marked as v2.1, not v2. Don't ship without explicit user ask. |
| **WASM runtime stretch goal causes scope creep** | High | High | Marked as v3, not v2. Don't ship without 6-week budget. |

---

## Appendix C: Open Questions for the User

1. Should `jcode plugin clone` support submodules? Default: yes, with `--recursive` flag. Submodules are common in monorepo-style plugins.
2. Should plugins be allowed to declare `provides_tools: ["my-tool"]` (claim a tool name in their manifest), or only at runtime via `registerTool`? Default: runtime only, ignore `provides_tools` field. The preflight analyzer warns if a plugin's runtime-registered tools don't match the manifest's `provides_tools`.
3. Should `ToolTier` be exposed in the LLM's system prompt? If yes, the LLM can choose not to call a "high-tier" tool. If no, the LLM only sees the tool list. Default: not exposed; the gate is enforced regardless of what the LLM sees.
4. Should per-tool user override be per-session or persistent? Default: persistent in `config.toml`, but a `/tools-approval` slash command in the TUI could set it per-session.
5. Should hot-reload be allowed in production mode, or only in dev? Default: allowed, but the gate's audit log flags it as a hot-reload event so the user can see it happened.
6. For workspace-crate plugins, should we require a `[[bin]]` or `[[example]]` entry that runs the plugin's smoke tests, or just rely on the workspace's test suite? Default: rely on workspace tests; document the convention in `docs/plugins.md`.
7. For git-cloned plugins, should we pin to a specific commit SHA by default (security) or track a branch by default (convenience)? Default: pin to the resolved SHA at clone time and write it to `installed.json`; provide `--track <branch>` for users who want auto-update.
8. For the `inventory`-based registration, should we support a "lazy" registration mode where the plugin code is only linked if its feature flag is enabled? Default: no, all workspace crates are always linked; the runtime gate (via `[plugins.workspace]`) is the only toggle. This keeps the Rust build graph simple.
