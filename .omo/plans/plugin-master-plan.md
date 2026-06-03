# jcode Plugin System — Master Implementation Plan

> Generated from research across 9 reference repos + jcode codebase deep exploration + user interview
> Goal: Design and implement a first-class plugin system for jcode combining the best patterns from opencode (dual-architecture + npm distribution), oh-my-pi (3-tier extensibility + npm install + typed settings), and pi-agent-rust (QuickJS embedding + RCU dispatch + 5-layer capability security)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Reference Architecture Analysis](#2-reference-architecture-analysis)
3. [Cross-Repo Pattern Synthesis — "Best of All Worlds"](#3-cross-repo-pattern-synthesis)
4. [Architecture Decisions](#4-architecture-decisions)
5. [Cargo Workspace Structure](#5-cargo-workspace-structure)
6. [Core Data Structures & Types](#6-core-data-structures--types)
7. [Plugin Manifest Convention](#7-plugin-manifest-convention)
8. [Plugin Discovery & Loading](#8-plugin-discovery--loading)
9. [QuickJS Runtime Integration](#9-quickjs-runtime-integration)
10. [Plugin API Surface](#10-plugin-api-surface)
11. [Capability Security Model](#11-capability-security-model)
12. [Event/Hook Integration](#12-eventhook-integration)
13. [Tool Registration System](#13-tool-registration-system)
14. [Dual-Process Architecture (Server vs TUI)](#14-dual-process-architecture)
15. [Configuration](#15-configuration)
16. [CLI Commands](#16-cli-commands)
17. [Integration Points in Existing Code](#17-integration-points)
18. [Test Plan](#18-test-plan)
19. [Migration Strategy](#19-migration-strategy)
20. [Cross-Repo Reference Matrix](#20-cross-repo-reference-matrix)
21. [Success Criteria](#21-success-criteria)
22. [Known Limitations & Future Work](#22-known-limitations--future-work)

---

## 1. Executive Summary

We are building a **first-class TypeScript/JavaScript plugin system** for jcode, embedding QuickJS via `rquickjs` to run user-authored scripts with process isolation, capability-based security, and full integration into jcode's existing server-client architecture.

The system draws from three proven approaches:
- **opencode**'s dual-plugin architecture (server + TUI plugins sharing the same npm package, auto-discovery from `.opencode/plugin/*.ts`, Hooks input/output mutation pattern)
- **oh-my-pi**'s 3-tier progression (legacy hooks → typed extensions with 30s timeout → npm-distributable plugins with feature toggles and typed settings schemas), CLI-first plugin management (`omp plugin install`)
- **pi-agent-rust**'s embedded QuickJS engine with SWC transpilation, Promise bridge for Rust↔JS async, RCU snapshot event dispatch with O(1) hook bitmap, 5-layer capability security chain, and dual timeout (500ms info / 5000ms actionable)

jcode is uniquely positioned because it **already has the server-client split** (`jcode serve` daemon + `jcode connect` client) that opencode's architecture requires. The plugin system naturally follows this: **server plugins** run in the daemon process to hook into agent lifecycle, tool execution, and session management; **TUI plugins** run in the client process to extend the terminal UI with custom views, keybindings, and side panels.

**Key design principles:**
1. **Safety first** — plugins run in QuickJS sandbox with capability-based permissions, not in-process with full Node.js access
2. **Progressive complexity** — simple config-triggered hooks for beginners, full TypeScript plugins for advanced users
3. **No vendor lock-in** — npm as distribution channel, standard TypeScript/JS authoring
4. **Best of all worlds** — opencode's DX (auto-discover, npm install, simple API) + oh-my-pi's maturity (typed settings, feature toggles, CLI) + pi-agent-rust's safety (QuickJS, capability chain, preflight analysis)
5. **Backward compatible** — existing config.toml, tool system, agent loop unchanged; plugins are additive

---

## 2. Reference Architecture Analysis

### 2.1 OpenCode Plugin System

**Architecture**: Dual-plugin — Server plugins (agent hooks, tools, auth, providers, chat) + TUI plugins (UI extension). Single npm package can expose both via `package.json` exports (`./server` and `./tui`).

**Key Patterns**:
| Pattern | Detail |
|---------|--------|
| Plugin signature | `type Plugin = (input: PluginInput, options?) => Promise<Hooks>` |
| Input object | `{ client, project, directory, worktree, serverUrl, $: BunShell }` |
| Hooks returned | Object with hook keys (tool, auth, config, chat.message, etc.) — each a callback |
| Mutation pattern | All hooks receive `{ input, output }` — plugin mutates `output` to modify behavior |
| Auto-discovery | `.opencode/plugin/*.ts` and `.opencode/tool/*.ts` scanned at startup |
| Installation | `opencode plugin <module>` → npm install to `~/.cache/opencode/packages/` → patch config |
| Config | `opencode.json` `"plugin"` array (strings or `[name, options]` pairs) |
| No sandboxing | Full Node.js/Bun access, no isolation |
| No hot-reload | Loaded once at startup |

**Hooks available**: dispose, event, config, tool, auth, provider, chat.message, chat.params, chat.headers, permission.ask, command.execute.before, tool.execute.before, shell.env, tool.execute.after, experimental.* (7 hooks), tool.definition

**Real plugin examples**:
- Simplest: export `{ server: async (ctx) => ({ tool: { mytool: tool({...}) } }) }`
- TUI: SolidJS-based with `api.ui.Dialog`, `api.route`, `api.keymap`, `api.slots`
- Auth: implement OAuth/API key flows via `auth` hook
- Workspace adapter: register custom workspace types

### 2.2 oh-my-pi Extension System

**Architecture**: 3-tier historical progression — Legacy hooks (shell-script style, narrow API, no timeout) → Extensions (TypeScript, full API, 30s timeout) → Plugins (npm-distributable, feature toggles, typed settings).

**Key Patterns**:
| Pattern | Detail |
|---------|--------|
| Extension signature | `export default function(pi: ExtensionAPI): void` |
| ExtensionAPI | `{ on(), registerTool(), registerCommand(), registerShortcut(), registerFlag(), registerMessageRenderer(), registerProvider(), sendMessage(), exec(), events: EventBus }` |
| Events (28+) | session lifecycle (start, switch, branch, compact, shutdown, tree), context, before_provider_request, before_agent_start, agent_start/end, turn_start/end, message_start/update/end, tool_execution_start/update/end, auto_compaction, auto_retry, input, tool_call, tool_result, user_bash, user_python |
| Timers | ExtensionRunner: 30s hard timeout. HookRunner: NO timeout (for permission gates) |
| Plugin install | `omp plugin install <pkg>` → `bun install` in `~/.omp/plugins/` → `omp-plugins.lock.json` |
| Feature toggles | `pkg[feature1,feature2]` syntax, `pkg[*]` all, `pkg[]` none |
| Typed settings | JSON Schema per plugin with `secret`, `env`, `min`, `max` |
| No sandboxing | Same as opencode — in-process, full access |

**before_agent_start distinction**: ExtensionRunner calls ALL handlers (accumulates messages, chains systemPrompt). HookRunner takes FIRST only.

### 2.3 pi-agent-rust Plugin System

**Architecture**: Rust-based AI agent with full embedded scripting via QuickJS + SWC (TypeScript→JS transpilation). ~33 hook events with dual timeout system.

**Key Patterns**:
| Pattern | Detail |
|---------|--------|
| JS Engine | rquickjs (QuickJS C library bindings) — lightweight, embeddable, no JIT, ~1MB footprint |
| TS support | SWC transpile `.ts` → `.js` at load time, cache compiled output |
| Promise bridge | Rust `oneshot` channels: JS calls `host.call("method", args)` → returns Promise that resolves when Rust completes |
| Event dispatch | RCU (Read-Copy-Update) snapshot pattern: `Arc<RwLock<Arc<RegistrySnapshot>>>` — handlers register to bitmap, snapshot swapped atomically |
| O(1) hook bitmap | `u64` bitmask: `(registered_mask & event_bit) != 0` for instant check |
| Capability security | 5-layer chain: deny list → global deny → allow list → global default → mode |
| Preflight analysis | Static analysis of plugin code before first execution — detect suspicious patterns |
| Dual timeout | Info hooks: 500ms, Actionable hooks: 5000ms (configurable) |
| fail_closed_hooks | Config flag: if true, hook failure blocks execution (deny-by-default) |

### 2.4 Other Repos (Lower Relevance)

| Repo | Relevance | Key Takeaway |
|------|-----------|-------------|
| oh-my-openagent | Medium | 5-tier composition, HTTP hooks with `${VAR}` interpolation |
| oh-my-claudecode | Medium | 11 hooks implemented, kill-switch env vars |
| oh-my-codex | Low-Medium | Dual-layer config, plugin timeout 1500ms, trust model with trusted_hash |
| codebuff | Low | PrintModeEvent — no user-facing plugin system |
| codex | Medium | Rust hook engine with FuturesUnordered, HookResult tri-state, full JSON Schema |

---

## 3. Cross-Repo Pattern Synthesis

This is the "Best of All Worlds" — what we take from each repo:

| # | Pattern | Source | Why It Wins |
|---|---------|--------|-------------|
| 1 | **QuickJS embedded** | pi-agent-rust | Isolation, small footprint (~1MB), no external runtime dependency |
| 2 | **`pi.on(event, handler)` subscription API** | pi-agent-rust + oh-my-pi | Simple, familiar, proven across both Rust and TS ecosystems |
| 3 | **SWC TypeScript transpilation** | pi-agent-rust | Users write TS, plugins run as JS |
| 4 | **Promise bridge (oneshot channels)** | pi-agent-rust | Async Rust↔JS without blocking either runtime |
| 5 | **RCU snapshot + O(1) hook bitmap** | pi-agent-rust | Zero-contention reads, instant event routing |
| 6 | **Input/output mutation pattern** | opencode | `{ input, output }` passing lets plugins inspect AND modify |
| 7 | **5-layer capability chain** | pi-agent-rust | Granular security without sacrificing usability |
| 8 | **Preflight static analysis** | pi-agent-rust | Catch malicious/errant code before it runs |
| 9 | **Typed settings schema** | oh-my-pi | Structured per-plugin config with validation |
| 10 | **Feature toggle install** | oh-my-pi | `plugin[feature1]` — selective, clean |
| 11 | **Dual timeout system** | pi-agent-rust | Info hooks fast (500ms), actionable hooks generous (5000ms) |
| 12 | **Permission hook no timeout** | oh-my-pi | User prompts must never time out |
| 13 | **fail_closed_hooks config** | pi-agent-rust | Security-conscious: deny on error |
| 14 | **Auto-discovery from `.jcode/plugins/*.ts`** | opencode | Zero-config for local plugins |
| 15 | **Secret env filtering** | pi-agent-rust | Don't leak API keys to plugins |
| 16 | **npm-based plugin install CLI** | oh-my-pi + opencode | Standard distribution channel |
| 17 | **Kill switch + audit trail** | pi-agent-rust | `JCODE_DISABLE_PLUGINS`, `JCODE_SKIP_PLUGINS` |
| 18 | **Inter-plugin event bus** | pi-agent-rust | `events.on()`, `events.emit()` for plugin↔plugin communication |
| 19 | **Dual server/TUI export** | opencode | Single npm package with both server and UI extensions |
| 20 | **Session lifecycle events** | oh-my-pi | `session_start`, `session_before_switch`, `session_compact`, `session_shutdown` |

---

## 4. Architecture Decisions

### 4.1 Chosen Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      jcode serve (daemon)                     │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │                    Agent Loop                            │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐ │ │
│  │  │ Pre-API  │→│ API Call │→│Post-API  │→│ Tool    │ │ │
│  │  │ Phase    │  │ Phase    │  │ Phase    │  │ Exec    │ │ │
│  │  └──────────┘  └──────────┘  └──────────┘  └─────────┘ │ │
│  │       │              │             │             │        │ │
│  │       ▼              ▼             ▼             ▼        │ │
│  │  ┌─────────────────────────────────────────────────────┐ │ │
│  │  │              Plugin Dispatcher                        │ │ │
│  │  │  ┌──────────┐  ┌──────────┐  ┌───────────────────┐  │ │ │
│  │  │  │ RCU      │→│ O(1)     │→│ FuturesUnordered  │  │ │ │
│  │  │  │Snapshot  │  │Bitmap    │  │ Parallel Dispatch │  │ │ │
│  │  │  └──────────┘  └──────────┘  └───────────────────┘  │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │                              │                            │ │
│  │                              ▼                            │ │
│  │  ┌─────────────────────────────────────────────────────┐ │ │
│  │  │          QuickJS Runtime Manager                     │ │ │
│  │  │  ┌──────────┐  ┌──────────┐  ┌───────────────────┐  │ │ │
│  │  │  │ Runtime  │→│ Sandbox  │→│ Promise Bridge     │  │ │ │
│  │  │  │ Pool     │  │ Context  │  │ (oneshot channels) │  │ │ │
│  │  │  └──────────┘  └──────────┘  └───────────────────┘  │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                               │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │              Safety System (existing)                     │ │
│  │  PermissionRequest queue, DCG classification,            │ │
│  │  Tool policy (allowed/disabled), Capability chain        │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
                    NDJSON over Unix socket
                            │
┌─────────────────────────────────────────────────────────────┐
│                   jcode connect (client/TUI)                  │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │              TUI Plugin Runtime                          │ │
│  │  ┌──────────┐  ┌──────────┐  ┌───────────────────────┐  │ │
│  │  │ Plugin   │→│ Slot     │→│ TuiPluginApi          │  │ │
│  │  │ Loader   │  │ Registry │  │ (route, keymap,        │  │ │
│  │  │          │  │          │  │  dialog, slot, theme,  │  │ │
│  │  └──────────┘  └──────────┘  │  kv, event, lifecycle) │  │ │
│  │                              └───────────────────────┘  │ │
│  └─────────────────────────────────────────────────────────┘ │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │              Existing TUI (ratatui 0.30)                  │ │
│  │  Info widgets, overlays, side panel, keybindings         │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| JS Engine | **QuickJS (rquickjs)** | Lightweight (~1MB), embeddable, isolated heap |
| TS Support | **SWC transpilation** | Best-in-class TS→JS speed, single binary |
| Plugin Model | **Server + TUI plugins** | jcode already has serve/connect split |
| Distribution | **npm packages + local files** | npm for published, `.jcode/plugins/*.ts` for local |
| Security | **5-layer capability chain** | Granular, proven in pi-agent-rust |
| Dispatch | **RCU snapshot + bitmap** | Zero-contention, O(1) event routing |
| Async Bridge | **Oneshot channels** | Simple, non-blocking, Rust-native |
| Config | **TOML section in config.toml** | Consistent with existing jcode config |
| Timeout | **Dual: 500ms / 5000ms** | Info hooks fast, actionable hooks generous |
| Plugin Format | **Single JS runtime context** | All plugins share one QuickJS runtime but get isolated sandbox contexts |

### 4.3 Alternatives Considered

| Approach | Source | Pros | Cons | Decision |
|----------|--------|------|------|----------|
| QuickJS (rquickjs) | pi-agent-rust | Small, embeddable, isolated heap | No JIT, slower for CPU-heavy | ✅ **Selected** |
| V8 (rusty_v8) | — | Full JS, JIT, debugger | Heavy (~50MB), complex build | ❌ Too heavy |
| Deno Core | — | TypeScript native, modern APIs | Very heavy, complex embedding | ❌ Too heavy |
| WASM (wasmtime) | pi-agent-rust (optional) | Language-agnostic, real sandbox | Limited stdlib, complex bindings | ⏸️ Future option |
| Bun/Node child process | opencode, oh-my-pi | No embedding needed | Process overhead, no isolation | ❌ Not sandboxed |
| WASI preview 2 | — | Standardized, multi-language | Immature ecosystem | ❌ Not ready |
| Hybrid: QuickJS + WASM | pi-agent-rust (both) | QuickJS for scripting, WASM for perf | More complexity | ✅ Plan for v2 |

### 4.4 Plugin Lifecycle

```
                       ┌──────────────────┐
                       │ Config loaded    │
                       └────────┬─────────┘
                                │
                                ▼
                       ┌──────────────────┐
                       │ Discovery        │
                       │ Scan 3 sources   │
                       └────────┬─────────┘
                                │
                                ▼
                       ┌──────────────────┐
                       │ Resolve & Load   │
                       │ npm→cache/file   │
                       └────────┬─────────┘
                                │
                                ▼
                       ┌──────────────────┐
                       │ Preflight Check  │
                       │ Capability decl  │
                       │ Static analysis  │
                       └────────┬─────────┘
                                │
                                ▼
                       ┌──────────────────┐
                       │ SWC Transpile    │
                       │ .ts → .js        │
                       └────────┬─────────┘
                                │
                                ▼
                       ┌──────────────────┐
                       │ QuickJS Eval     │
                       │ Create context   │
                       │ Inject pi API    │
                       │ Call factory     │
                       └────────┬─────────┘
                                │
                                ▼
                       ┌──────────────────┐
                       │ Register Hooks   │
                       │ RCU snapshot     │
                       │ Bitmap update    │
                       └────────┬─────────┘
                                │
                    ┌───────────┴───────────┐
                    │                       │
                    ▼                       ▼
           ┌─────────────────┐   ┌─────────────────┐
           │ Active: Event   │   │ On Disable/     │
           │ Dispatch        │   │ Uninstall       │
           │ Dual timeout    │   │ Unregister      │
           │ Error isolation │   │ Snapshot update │
           └─────────────────┘   └─────────────────┘
```

---

## 5. Cargo Workspace Structure

Add new crates to existing 43-member workspace:

```toml
# Cargo.toml additions
[workspace]
members = [
    # ... existing 43 members ...
    "crates/jcode-plugin-core",     # Plugin types, manifest, config, security
    "crates/jcode-plugin-runtime",  # QuickJS runtime, SWC transpilation, sandbox
]

[workspace.dependencies]
rquickjs = { version = "0.7", features = ["parallel", "catch", "classes"] }
swc_core = { version = "1.0", features = ["ecma_transforms", "ecma_parser"] }
```

### 5.1 Crate Dependency Graph

```
jcode-plugin-core (no deps on other jcode crates)
    │
    └── jcode-plugin-runtime
            │
            ├── jcode-base (for config integration)
            ├── jcode-app-core (for server plugin integration)
            └── jcode-tui (for TUI plugin integration)
```

### 5.2 Module Structure

```
crates/jcode-plugin-core/src/
├── lib.rs          # Re-exports
├── manifest.rs     # PluginManifest, PluginFeature, SettingSchema
├── security.rs     # CapabilityChain, Permission, AccessMode
├── config.rs       # PluginConfig, PluginSource, DiscoveryPaths
├── events.rs       # PluginEvent enum (28 events), EventInput, EventOutput
├── types.rs        # PluginId, PluginVersion, PluginState, PluginOrigin
├── errors.rs       # PluginError enum
└── serde.rs        # Serialization helpers

crates/jcode-plugin-runtime/src/
├── lib.rs          # Re-exports
├── runtime.rs      # QuickJS Runtime manager, pool
├── sandbox.rs      # SandboxContext, capability enforcement, preflight
├── transpiler.rs   # SWC TypeScript → JavaScript transpilation
├── bridge.rs       # PromiseBridge — Rust↔JS oneshot channels
├── api.rs          # PluginAPI bindings exposed to JS
├── dispatcher.rs   # RCU snapshot dispatcher + O(1) bitmap
├── loader.rs       # PluginLoader — file/npm resolution, eval, registration
├── registry.rs     # PluginRegistry — active plugins, state, lifecycle
├── native.rs       # Native functions exposed to JS (tool exec)
├── timer.rs        # Dual timeout implementation
├── audit.rs        # Audit trail, kill switches
└── server.rs       # Server-side plugin host
```

---

## 6. Core Data Structures & Types

### 6.1 Plugin Identity & State

```rust
// crates/jcode-plugin-core/src/types.rs

use std::collections::HashMap;
use semver::Version;
use serde::{Deserialize, Serialize};

/// Unique identifier for a plugin — npm package name or file path
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginId(String);

impl PluginId {
    pub fn npm(name: &str) -> Self { Self(format!("npm:{name}")) }
    pub fn file(path: &str) -> Self { Self(format!("file:{path}")) }
    pub fn bundled(name: &str) -> Self { Self(format!("builtin:{name}")) }
    pub fn to_string(&self) -> String { self.0.clone() }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginVersion {
    pub semver: Version,
    pub jcode_min_version: Option<Version>,
    pub jcode_max_version: Option<Version>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginState {
    Discovered, Loading, Loaded, Active,
    Error(String), Disabled, Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginOrigin {
    NpmPackage { name: String, version: String },
    LocalFile { path: String },
    Builtin { name: String },
    Remote { url: String },
}
```

### 6.2 Plugin Manifest

```rust
// crates/jcode-plugin-core/src/manifest.rs

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub package_name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub kind: PluginKind,
    pub entry: PluginEntry,
    pub capabilities: PluginCapabilities,
    pub features: HashMap<String, PluginFeature>,
    pub settings: HashMap<String, SettingSchema>,
    pub engines: PluginEngines,
    pub icon: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginKind { Server, Tui, Both }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub server: Option<String>,
    pub tui: Option<String>,
    pub both: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    pub fs_read: Vec<String>,
    pub fs_write: Vec<String>,
    pub network: Vec<String>,
    pub shell: bool,
    pub register_tools: bool,
    pub register_commands: bool,
    pub register_providers: bool,
    pub read_config: bool,
    pub write_config: bool,
    pub env_vars: Vec<String>,
    pub events: Vec<String>,
    pub llm_access: bool,
    pub session_access: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginFeature {
    pub description: String,
    pub default: bool,
    pub entry: Option<String>,
    pub additional_capabilities: Option<PluginCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SettingSchema {
    String { description: String, default: Option<String>, secret: bool,
        env: Option<String>, pattern: Option<String>, max_length: Option<usize> },
    Number { description: String, default: Option<f64>, min: Option<f64>, max: Option<f64> },
    Boolean { description: String, default: Option<bool> },
    Enum { description: String, default: Option<String>, values: Vec<String> },
    Array { description: String, default: Option<Vec<serde_json::Value>>,
        items: Box<SettingSchema>, max_items: Option<usize> },
    Object { description: String, default: Option<serde_json::Value>,
        properties: HashMap<String, SettingSchema> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEngines { pub jcode: Option<String> }

impl PluginManifest {
    pub fn from_package_json(value: &serde_json::Value) -> Result<Self, PluginError> {
        let section = value.get("jcode").or_else(|| value.get("pi"))
            .ok_or(PluginError::InvalidManifest("missing 'jcode' or 'pi' field".into()))?;
        serde_json::from_value(section.clone())
            .map_err(|e| PluginError::InvalidManifest(e.to_string()))
    }
}
```

### 6.3 Event Types

```rust
// crates/jcode-plugin-core/src/events.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum PluginEvent {
    PreToolUse = 0, PostToolUse = 1, PostToolUseFailure = 2,
    ToolExecutionStart = 3, ToolExecutionEnd = 4,
    SessionStart = 5, SessionEnd = 6, SessionSwitch = 7,
    SessionCompact = 8, SessionBeforeCompact = 9, SessionShutdown = 10,
    PermissionRequest = 12, PermissionDenied = 13,
    AgentStart = 14, AgentEnd = 15, TurnStart = 16, TurnEnd = 17,
    MessageStart = 18, MessageEnd = 19,
    PreCompact = 20, PostCompact = 21,
    TaskCreated = 22, TaskCompleted = 23, AutoCompactionStart = 24,
    UserPromptSubmit = 25, Stop = 26, Notification = 27,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum EventInput {
    PreToolUse { tool_name: String, tool_input: serde_json::Value, session_id: String },
    PostToolUse { tool_name: String, tool_input: serde_json::Value,
        tool_output: serde_json::Value, duration_ms: u64, success: bool, session_id: String },
    PostToolUseFailure { tool_name: String, tool_input: serde_json::Value,
        error: String, duration_ms: u64, session_id: String },
    SessionStart { session_id: String, project_dir: String, model: String, provider: String },
    SessionEnd { session_id: String, duration_seconds: u64, message_count: u64 },
    PermissionRequest { action: String, tool_name: Option<String>,
        target: Option<String>, session_id: String },
    AgentStart { session_id: String, system_prompt: Vec<String>, tools: Vec<String> },
    TurnStart { session_id: String, turn_number: u32, messages: Vec<serde_json::Value> },
    UserPromptSubmit { content: String, session_id: String },
    PreCompact { session_id: String, message_count: u32,
        token_count: u64, system_prompt: Vec<String> },
    PostCompact { session_id: String, messages_removed: u32, tokens_saved: u64 },
    Stop { session_id: String, reason: String },
    Notification { level: String, message: String, session_id: Option<String> },
    ToolExecutionStart { tool_name: String, tool_input: serde_json::Value, session_id: String },
    ToolExecutionEnd { tool_name: String, tool_output: serde_json::Value,
        duration_ms: u64, session_id: String },
    // ... remaining variants follow same pattern
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum EventOutput {
    PreToolUse { block: Option<String>, modified_input: Option<serde_json::Value> },
    PostToolUse { modified_output: Option<serde_json::Value> },
    PermissionRequest { decision: Option<PermissionDecision>, message: Option<String> },
    AgentStart { additional_system_prompt: Vec<String> },
    PreCompact { system_prompt: Option<Vec<String>>, instructions: Option<String>, prevent: bool },
    UserPromptSubmit { modified_prompt: Option<String> },
    Notification { suppress: Option<bool>, modified_message: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionDecision { Allow, Deny, Ask }
```

### 6.4 Security Types

```rust
// crates/jcode-plugin-core/src/security.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityChain {
    pub deny_list: CapabilitySet,
    pub global_deny: CapabilitySet,
    pub allow_list: CapabilitySet,
    pub global_default: AccessDefault,
    pub mode: AccessMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub fs_paths: Vec<String>,
    pub hosts: Vec<String>,
    pub tools: Vec<String>,
    pub env_vars: Vec<String>,
    pub shell_commands: Vec<String>,
    pub config_keys: Vec<String>,
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AccessDefault { Deny, Allow, Ask }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AccessMode { All, Trusted, None, Interactive }

impl CapabilitySet {
    pub fn matches(&self, resource: &str, _action: &CapabilityAction) -> bool {
        self.tools.contains(resource)
            || self.hosts.iter().any(|h| resource.contains(h))
            || self.fs_paths.iter().any(|p| resource.starts_with(p))
    }
}

#[derive(Debug, Clone)]
pub enum CapabilityAction { Read, Write, Execute, Network, Config, Session, Provider }

#[derive(Debug, Clone)]
pub enum AccessDecision { Allowed(String), Denied(String), NeedsApproval(String) }
```

---

## 7. Plugin Manifest Convention

### 7.1 package.json Format

```jsonc
{
  "name": "@scope/jcode-my-plugin",
  "version": "1.0.0",
  "description": "My awesome jcode plugin",
  "jcode": {
    "name": "My Plugin",
    "kind": "both",
    "entry": { "server": "./dist/server.js", "tui": "./dist/tui.js" },
    "capabilities": {
      "fs_read": ["$CWD/**"],
      "network": ["api.my-service.com"],
      "register_tools": true,
      "events": ["PreToolUse", "SessionStart"]
    },
    "features": {
      "advanced": {
        "description": "Advanced features",
        "default": false,
        "entry": "./dist/advanced.js",
        "additional_capabilities": { "network": ["analytics.my-service.com"] }
      }
    },
    "settings": {
      "apiKey": { "type": "string", "secret": true, "env": "MY_PLUGIN_API_KEY" },
      "maxResults": { "type": "number", "default": 10, "min": 1, "max": 100 }
    },
    "engines": { "jcode": ">=0.20.0" }
  }
}
```

### 7.2 Local Plugin Format

```typescript
// .jcode/plugins/my-plugin.ts
export const manifest = {
  name: "My Local Plugin",
  capabilities: { fs_read: ["$CWD/**"], events: ["PreToolUse"] },
  settings: { greeting: { type: "string", default: "Hello!" } },
};

export default function (pi: PluginAPI) {
  pi.on("PreToolUse", async (input, output) => {
    pi.logger.info(`Tool ${input.tool_name} about to run`);
  });
}
```

---

## 8. Plugin Discovery & Loading

### 8.1 Discovery Sources

1. **Config**: `config.toml → [plugin.sources]`
2. **Auto-discovery**: `.jcode/plugins/*.ts`, `.jcode/plugins/*.js`, `.jcode/plugins/*/index.ts`
3. **Tool auto-discovery**: `.jcode/tools/*.ts`
4. **Npm-installed**: `.jcode/cache/packages/<name>/`
5. **Built-in**: Compiled into binary

### 8.2 Loading Pipeline

```rust
pub struct PluginLoader {
    discovery: DiscoveryPaths,
    config: PluginConfig,
    registry: Arc<PluginRegistry>,
    transpiler: Arc<Transpiler>,
    runtime: Arc<RuntimeManager>,
}

impl PluginLoader {
    pub async fn load_all(&self) -> Result<Vec<PluginId>, PluginError> {
        let sources = self.discover_sources().await?;
        let mut loaded = Vec::new();
        for source in sources {
            match self.load_one(&source).await {
                Ok(id) => loaded.push(id),
                Err(e) => {
                    if self.config.fail_closed.unwrap_or(false) { return Err(e); }
                    tracing::warn!("Failed to load {source:?}: {e}");
                }
            }
        }
        Ok(loaded)
    }

    async fn discover_sources(&self) -> Result<Vec<PluginSource>, PluginError> {
        let mut sources = Vec::new();
        if let Some(ref cfg) = self.config.sources { sources.extend(cfg.clone()); }
        for dir in &self.discovery.plugin_dirs {
            self.scan_directory_for_plugins(dir, &mut sources).await?;
        }
        let npm_dir = &self.discovery.npm_cache;
        if npm_dir.exists() {
            self.scan_npm_cache(npm_dir, &mut sources).await?;
        }
        Ok(sources)
    }

    async fn load_one(&self, source: &PluginSource) -> Result<PluginId, PluginError> {
        let (path, id) = match source {
            PluginSource::Npm { package, version } => {
                let entry = self.resolve_npm_entry(package, version.as_deref()).await?;
                (entry.path, PluginId::npm(package))
            }
            PluginSource::File { path } => (std::path::PathBuf::from(path), PluginId::file(path)),
            PluginSource::Directory { path } => {
                let p = std::path::Path::new(path);
                let idx = if p.join("index.ts").exists() { p.join("index.ts") } else { p.join("index.js") };
                (idx, PluginId::file(path))
            }
        };
        let code = tokio::fs::read_to_string(&path).await?;
        let js_code = if path.extension().map_or(false, |e| e == "ts" || e == "tsx") {
            self.transpiler.transpile(&code, &path.to_string_lossy())?
        } else { code };
        let context = self.runtime.create_sandbox(id.clone(), PluginManifest::default())?;
        context.eval(&js_code)?;
        self.registry.register(id.clone(), context)?;
        Ok(id)
    }
}
```

### 8.3 NPM Resolution

```rust
impl PluginLoader {
    async fn resolve_npm_entry(&self, package: &str, version: Option<&str>) -> Result<ResolvedEntry> {
        let cache = self.discovery.npm_cache.join(sanitize_name(package));
        if !cache.exists() {
            self.install_npm(package, version, &cache).await?;
        }
        let pkg_json = cache.join("node_modules").join(package).join("package.json");
        let content = tokio::fs::read_to_string(&pkg_json).await?;
        let json: serde_json::Value = serde_json::from_str(&content)?;
        let manifest = PluginManifest::from_package_json(&json)?;
        let entry = manifest.entry.server.or(manifest.entry.both)
            .ok_or(PluginError::InvalidManifest("No server entry point".into()))?;
        Ok(ResolvedEntry { path: cache.join(entry), manifest })
    }

    async fn install_npm(&self, package: &str, version: Option<&str>, dir: &Path) -> Result<()> {
        if !is_valid_package_name(package) {
            return Err(PluginError::Npm("Invalid package name".into()));
        }
        tokio::fs::create_dir_all(dir).await?;
        let spec = match version { Some(v) => format!("{package}@{v}"), None => package.into() };
        let out = tokio::process::Command::new("npm")
            .args(["install", &spec, "--no-save", "--no-audit"])
            .current_dir(dir).output().await?;
        if !out.status.success() {
            return Err(PluginError::Npm(String::from_utf8_lossy(&out.stderr).into()));
        }
        Ok(())
    }
}

fn sanitize_name(name: &str) -> String { name.replace('/', "__").replace('@', "") }
fn is_valid_package_name(name: &str) -> bool {
    let re = regex::Regex::new(r"^@?[a-z0-9][a-z0-9._-]*/?[a-z0-9][a-z0-9._-]*$").unwrap();
    re.is_match(name) && !name.contains("..") && !name.contains(';') && !name.contains('|')
}
```

---

## 9. QuickJS Runtime Integration

### 9.1 Runtime Manager

```rust
use rquickjs::{AsyncRuntime, Runtime};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::{Mutex, Semaphore};

pub struct RuntimeManager {
    main_runtime: Arc<AsyncRuntime>,
    pool: Arc<Mutex<RuntimePool>>,
    semaphore: Arc<Semaphore>,
}

struct RuntimePool {
    available: Vec<AsyncRuntime>,
    max_runtimes: usize,
}

impl RuntimeManager {
    pub fn new(config: RuntimeConfig) -> Result<Self, PluginError> {
        let rt = AsyncRuntime::new()?;
        rt.set_max_stack_size(512 * 1024)?;
        rt.set_gc_threshold(10 * 1024 * 1024)?;
        rt.set_memory_limit(50 * 1024 * 1024)?;
        Ok(Self {
            main_runtime: Arc::new(rt),
            pool: Arc::new(Mutex::new(RuntimePool { available: Vec::new(), max_runtimes: config.max_runtimes })),
            semaphore: Arc::new(Semaphore::new(config.max_concurrent)),
        })
    }

    pub fn create_sandbox(&self, id: PluginId, m: PluginManifest) -> Result<SandboxContext, PluginError> {
        let rt = self.acquire_runtime()?;
        SandboxContext::new(id, m, rt)
    }

    fn acquire_runtime(&self) -> Result<AsyncRuntime, PluginError> {
        if let Ok(mut pool) = self.pool.try_lock() {
            if let Some(rt) = pool.available.pop() { return Ok(rt); }
        }
        AsyncRuntime::new().map_err(|e| PluginError::Runtime(e.to_string()))
    }

    pub fn release(&self, runtime: AsyncRuntime) {
        if let Ok(mut pool) = self.pool.try_lock() {
            if pool.available.len() < pool.max_runtimes { pool.available.push(runtime); }
        }
    }
}
```

### 9.2 Sandbox & Dual Timeout

```rust
pub struct SandboxContext {
    runtime: AsyncRuntime,
    context: AsyncContext,
    id: PluginId,
    manifest: PluginManifest,
    capability_chain: Arc<CapabilityChain>,
    timeout: DualTimeout,
}

#[derive(Debug, Clone)]
pub struct DualTimeout {
    pub info: Duration,         // 500ms default
    pub actionable: Duration,   // 5000ms default
    pub permission: Option<Duration>,
}

impl Default for DualTimeout {
    fn default() -> Self {
        Self { info: Duration::from_millis(500), actionable: Duration::from_millis(5000), permission: None }
    }
}

impl SandboxContext {
    pub fn eval(&self, code: &str) -> Result<(), PluginError> {
        self.context.with(|ctx| {
            let wrapped = format!("(function(pi) {{ {code} }})(this.__jcode_pi);");
            ctx.eval::<(), _>(&wrapped).map_err(|e| PluginError::Eval(e.to_string()))
        })
    }

    pub async fn call_handler(&self, event: PluginEvent, input: EventInput,
            output: Option<EventOutput>) -> Result<HandlerResult, PluginError> {
        let timeout = self.get_timeout(event);
        match tokio::time::timeout(timeout, self.call_inner(event, input, output)).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(PluginError::Timeout(timeout)),
        }
    }

    async fn call_inner(&self, event: PluginEvent, input: EventInput,
            output: Option<EventOutput>) -> Result<HandlerResult, PluginError> {
        self.context.with(|ctx| -> Result<HandlerResult, PluginError> {
            let global = ctx.globals();
            let pi: Object = global.get("__jcode_pi")?;
            let handlers: Object = pi.get("_handlers")?;
            let ev = format!("{event:?}");
            if let Ok(handler) = handlers.get::<_, Function>(&ev) {
                let i = ctx.json_stringify(serde_json::to_value(&input)?)?;
                let o = match output { Some(ref o) => ctx.json_stringify(serde_json::to_value(o)?)?,
                    None => ctx.eval("null")? };
                let r: Value = handler.call((i, o))?;
                let s: String = ctx.json_stringify(r)?;
                serde_json::from_str(&s).map_err(|e| PluginError::QuickJs(e.to_string()))
            } else { Ok(HandlerResult { action: HandlerAction::Continue, output: None, error: None }) }
        })
    }

    fn get_timeout(&self, event: PluginEvent) -> Duration {
        match event {
            PluginEvent::PermissionRequest | PluginEvent::PermissionDenied =>
                self.timeout.permission.unwrap_or(Duration::from_secs(3600)),
            PluginEvent::SessionEnd | PluginEvent::TurnEnd | PluginEvent::PostCompact
            | PluginEvent::AutoCompactionStart => self.timeout.info,
            _ => self.timeout.actionable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerResult {
    pub action: HandlerAction,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandlerAction { Continue, Block(String), Allow, Deny, Error }
```

### 9.3 Promise Bridge

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::oneshot;

pub struct PromiseBridge {
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Vec<u8>>>>>,
}

impl PromiseBridge {
    pub fn new() -> Self {
        Self { next_id: AtomicU64::new(1), pending: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn install(&self, ctx: &Ctx<'_>) -> Result<(), rquickjs::Error> {
        let pending = Arc::clone(&self.pending);
        let call_fn = Function::new(ctx.clone(), move |method: String, _args: Value| {
            let pending = Arc::clone(&pending);
            let promise = ctx.promise(|_| {
                let (tx, rx) = oneshot::channel();
                let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    pending.lock().await.insert(id, tx);
                    let result = dispatch_host_call(method).await;
                    if let Some(s) = pending.lock().await.remove(&id) {
                        let _ = s.send(result);
                    }
                })
            })?;
            Ok(promise)
        })?;
        let host = Object::new(ctx.clone())?;
        host.set("call", call_fn)?;
        ctx.globals().set("__jcode_host", host)?;
        Ok(())
    }
}

async fn dispatch_host_call(method: String) -> Vec<u8> {
    match method.as_str() {
        "getConfig" => serde_json::to_vec(&*CONFIG.load().await).unwrap_or_default(),
        _ => Vec::new(),
    }
}
```

---

## 10. Plugin API Surface

### 10.1 TypeScript API

```typescript
interface PluginAPI {
  readonly id: string;
  readonly name: string;
  readonly version: string;

  on(event: PluginEvent, handler: EventHandler): void;
  once(event: PluginEvent, handler: EventHandler): void;
  off(event: PluginEvent, handler?: EventHandler): void;

  registerTool(definition: ToolDefinition): void;
  unregisterTool(name: string): void;
  getTools(): ToolDefinition[];

  registerCommand(name: string, handler: CommandHandler): void;
  registerProvider(provider: ProviderDefinition): void;

  getConfig<T>(key: string, default?: T): Promise<T>;
  getSettings(): Promise<Record<string, any>>;

  readonly logger: { debug(...args: any[]): void; info(...args: any[]): void;
    warn(...args: any[]): void; error(...args: any[]): void; };

  readonly kv: { get<T>(key: string): Promise<T | null>;
    set<T>(key: string, value: T): Promise<void>;
    delete(key: string): Promise<void>;
    list(prefix?: string): Promise<string[]>; };

  readonly events: EventBus;

  sleep(ms: number): Promise<void>;
  uuid(): string;
  readonly cwd: string;
  readonly dataDir: string;

  readonly http: { get(url: string, options?: RequestOptions): Promise<HttpResponse>;
    post(url: string, body?: any, options?: RequestOptions): Promise<HttpResponse>; };

  readonly fs: { readText(path: string): Promise<string>;
    writeText(path: string, content: string): Promise<void>;
    exists(path: string): Promise<boolean>;
    list(dir: string): Promise<string[]>; };

  readonly session: { getId(): Promise<string>;
    sendMessage(content: string): Promise<void>;
    getMessages(): Promise<Message[]>; };
}

type EventHandler = (input: any, output?: any) => HandlerResult | Promise<HandlerResult>;
interface HandlerResult { action: "continue" | "block" | "allow" | "deny" | "error";
  output?: Record<string, any>; error?: string; }
interface ToolDefinition { name: string; description: string;
  parameters: Record<string, any>; execute(args: any, ctx: any): Promise<any>; }
interface EventBus { on(event: string, handler: Function): void;
  emit(event: string, data: any): void; off(event: string, handler: Function): void; }
```

### 10.2 Rust PluginApiBindings

```rust
pub struct PluginApiBindings {
    plugin_id: PluginId,
    manifest: PluginManifest,
    capability_chain: Arc<CapabilityChain>,
    registry: Arc<PluginRegistry>,
    kv_store: Arc<KvStore>,
    bridge: Arc<PromiseBridge>,
}

impl PluginApiBindings {
    pub fn install(&self, ctx: &Ctx<'_>) -> Result<(), rquickjs::Error> {
        let pi = Object::new(ctx.clone())?;
        pi.set("id", self.plugin_id.to_string())?;
        pi.set("name", self.manifest.name.clone())?;
        pi.set("version", self.manifest.version.clone())?;
        pi.set("on", self.make_on_fn(ctx)?)?;
        pi.set("registerTool", self.make_register_tool_fn(ctx)?)?;
        pi.set("getConfig", self.make_get_config_fn(ctx)?)?;
        pi.set("logger", self.make_logger(ctx)?)?;

        let kv = Object::new(ctx.clone())?;
        kv.set("get", self.make_kv_get_fn(ctx)?)?;
        kv.set("set", self.make_kv_set_fn(ctx)?)?;
        pi.set("kv", kv)?;

        pi.set("sleep", self.make_sleep_fn(ctx)?)?;
        pi.set("uuid", self.make_uuid_fn(ctx)?)?;
        pi.set("cwd", self.cwd.clone())?;
        pi.set("dataDir", self.data_dir.clone())?;
        pi.set("_handlers", Object::new(ctx.clone())?)?;
        ctx.globals().set("__jcode_pi", pi)?;
        self.bridge.install(ctx)?;
        Ok(())
    }

    fn make_register_tool_fn(&self, ctx: &Ctx<'_>) -> Result<Function, rquickjs::Error> {
        let registry = Arc::clone(&self.registry);
        Function::new(ctx.clone(), move |tool_def: Object| {
            let name: String = tool_def.get("name")?;
            registry.register_js_tool(name, tool_def);
        })
    }
}
```

---

## 11. Capability Security Model

### 11.1 Preflight Static Analysis

```rust
impl PreflightAnalyzer {
    pub fn analyze(code: &str, declared: &PluginCapabilities) -> PreflightResult {
        let mut warnings = Vec::new();
        let mut blocks = Vec::new();
        let mut detected = Vec::new();

        if code.contains("eval(") { warnings.push("Code uses eval()".into()); detected.push("eval".into()); }
        if code.contains("new Function(") { warnings.push("Uses Function constructor".into()); }
        if code.contains("process.") { warnings.push("References 'process' (not available)".into()); }
        if code.contains("require(") { warnings.push("Uses require() — use import".into()); }

        let has_fetch = code.contains("fetch(");
        if has_fetch && declared.network.is_empty() {
            warnings.push("fetch() used but no network capability declared".into());
        }

        let suspicious = vec!["rm -rf", "sudo ", "chmod 777", "> /dev/sda"];
        let found: Vec<String> = suspicious.into_iter().filter(|s| code.contains(s))
            .map(String::from).collect();
        detected.extend(found.clone());
        if !found.is_empty() {
            blocks.push(format!("Suspicious patterns: {}", found.join(", ")));
        }

        PreflightResult {
            passed: blocks.is_empty(),
            warnings, blocks,
            declared_capabilities: declared.clone(),
            detected_patterns: detected,
            static_analysis: StaticAnalysis {
                has_eval: code.contains("eval("),
                has_dynamic_import: code.contains("import("),
                has_fetch,
                has_process_access: code.contains("process."),
                has_fs_access: vec![], has_network_access: if has_fetch { vec!["fetch".into()] } else { vec![] },
                suspicious_strings: found,
            },
        }
    }
}
```

### 11.2 Kill Switches

```rust
pub static DISABLE_ALL_PLUGINS: AtomicBool = AtomicBool::new(false);
pub static SKIP_HOOKS: AtomicBool = AtomicBool::new(false);

pub fn check_kill_switches() {
    if std::env::var("JCODE_DISABLE_PLUGINS").is_ok() {
        DISABLE_ALL_PLUGINS.store(true, Ordering::SeqCst);
    }
    if std::env::var("JCODE_SKIP_PLUGINS").is_ok() {
        SKIP_HOOKS.store(true, Ordering::SeqCst);
    }
}
```

### 11.3 Audit Trail

```rust
pub struct AuditTrail {
    entries: Mutex<VecDeque<AuditEntry>>,
    max_entries: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub plugin_id: String,
    pub resource: String,
    pub action: String,
    pub decision: String,
    pub reason: String,
}

impl AuditTrail {
    pub fn log_access(&self, plugin_id: &PluginId, resource: &str,
            action: &CapabilityAction, decision: &AccessDecision) {
        let (ds, reason) = match decision {
            AccessDecision::Allowed(r) => ("allowed", r.clone()),
            AccessDecision::Denied(r) => ("denied", r.clone()),
            AccessDecision::NeedsApproval(r) => ("needs_approval", r.clone()),
        };
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.max_entries { entries.pop_front(); }
        entries.push_back(AuditEntry {
            timestamp: Utc::now(),
            plugin_id: plugin_id.to_string(),
            resource: resource.into(),
            action: format!("{action:?}"),
            decision: ds.into(),
            reason,
        });
    }

    pub fn get_recent(&self, count: usize) -> Vec<AuditEntry> {
        self.entries.lock().unwrap().iter().rev().take(count).cloned().collect()
    }
}
```


---

## 12. Event/Hook Integration

### 12.1 RCU Snapshot Dispatcher

```rust
use std::sync::{Arc, RwLock};
use tokio_stream::FuturesUnordered;
use futures::StreamExt;

#[derive(Debug, Clone)]
struct HandlerBitmap(u128);

impl HandlerBitmap {
    fn new() -> Self { Self(0) }
    fn set(&mut self, event: PluginEvent) {
        self.0 |= 1u128 << (event as u32);
    }
    fn has(&self, event: PluginEvent) -> bool {
        (self.0 & (1u128 << (event as u32))) != 0
    }
    fn clear(&mut self, event: PluginEvent) {
        self.0 &= !(1u128 << (event as u32));
    }
}

struct RegistrySnapshot {
    bitmap: HandlerBitmap,
    handlers: Vec<(PluginEvent, PluginId, HandlerSlot)>,
}

pub enum HandlerSlot {
    Js(rquickjs::Value),
    Rust(Box<dyn Fn(EventInput, Option<EventOutput>) -> Box<dyn Future<Output = HandlerResult> + Send + Sync>>),
}

pub struct RcuDispatcher {
    snapshot: RwLock<Arc<RegistrySnapshot>>,
    pending: Mutex<Vec<(PluginEvent, PluginId, HandlerSlot)>>,
}

impl RcuDispatcher {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(Arc::new(RegistrySnapshot {
                bitmap: HandlerBitmap::new(), handlers: Vec::new(),
            })),
            pending: Mutex::new(Vec::new()),
        }
    }

    pub fn register(&self, event: PluginEvent, id: PluginId, slot: HandlerSlot) {
        self.pending.lock().unwrap().push((event, id, slot));
    }

    pub fn commit(&self) {
        let mut pending = self.pending.lock().unwrap();
        if pending.is_empty() { return; }
        let current = self.snapshot.read().unwrap().clone();
        let mut new_bitmap = current.bitmap.clone();
        let mut new_handlers = current.handlers.clone();
        for (event, id, slot) in pending.drain(..) {
            new_bitmap.set(event);
            new_handlers.push((event, id, slot));
        }
        *self.snapshot.write().unwrap() = Arc::new(RegistrySnapshot {
            bitmap: new_bitmap, handlers: new_handlers,
        });
    }

    pub fn has_handler(&self, event: PluginEvent) -> bool {
        self.snapshot.read().unwrap().bitmap.has(event)
    }

    pub async fn dispatch(&self, event: PluginEvent, input: EventInput,
            output: Option<EventOutput>, runtimes: &RuntimeManager) -> Vec<(PluginId, HandlerResult)> {
        let snapshot = self.snapshot.read().unwrap().clone();
        if !snapshot.bitmap.has(event) { return Vec::new(); }

        let handlers: Vec<_> = snapshot.handlers.iter()
            .filter(|(e, _, _)| *e == event)
            .map(|(_, id, slot)| (id.clone(), slot))
            .collect();
        if handlers.is_empty() { return Vec::new(); }

        let mut results = Vec::new();
        let mut futures = FuturesUnordered::new();

        for (_id, slot) in handlers {
            match slot {
                HandlerSlot::Js(_) => {
                    // Execute in sandbox context with timeout
                    // (requires runtime reference)
                }
                HandlerSlot::Rust(f) => {
                    let inp = input.clone();
                    let out = output.clone();
                    futures.push(async move { f(inp, out).await });
                }
            }
        }

        while let Some(result) = futures.next().await {
            results.push((PluginId::bundled("rust"), result));
        }
        results
    }

    pub fn unregister_plugin(&self, id: &PluginId) {
        let current = self.snapshot.read().unwrap().clone();
        let mut new_bitmap = HandlerBitmap::new();
        let mut new_handlers = Vec::new();
        for (event, pid, slot) in &current.handlers {
            if pid != id {
                new_bitmap.set(*event);
                new_handlers.push((*event, pid.clone(), slot.clone()));
            }
        }
        *self.snapshot.write().unwrap() = Arc::new(RegistrySnapshot {
            bitmap: new_bitmap, handlers: new_handlers,
        });
    }
}
```

### 12.2 Integration with Agent Loop

The agent turn loop fires plugin events at key injection points:

```
Agent Turn Loop:
  1. Pre-API Phase
     → TurnStart, PermissionRequest, AgentStart

  2. API Call Phase
     → MessageStart, MessageEnd
     (existing API streaming, no plugin interference)

  3. Post-API: Tool Execution Loop
     for each tool_call:
       → PreToolUse (can block via HandlerAction::Block)
       → ToolExecutionStart
       → [execute tool]
       → ToolExecutionEnd
       → PostToolUse or PostToolUseFailure

  4. Turn End
     → TurnEnd
```

### 12.3 Dual-Process Event Flow

```
jcode serve (daemon):
  Dispatches all server events
  Server plugins subscribe and respond
  Forwards notification events to TUI via protocol

jcode connect (client/TUI):
  Receives forwarded events via PluginServerEvent::Event
  TUI plugins can subscribe to forwarded events
  TUI plugins also handle local events (keybindings, slots)
```

---

## 13. Tool Registration System

### 13.1 JS Plugin Tool Registry

```rust
pub struct JsToolRegistry {
    tools: Arc<Mutex<HashMap<String, JsToolHandle>>>,
}

struct JsToolHandle {
    plugin_id: PluginId,
    name: String,
    description: String,
    execute_fn: rquickjs::Value,
}

impl JsToolRegistry {
    pub fn register(&self, id: PluginId, name: String, description: String,
            execute_fn: rquickjs::Value) {
        let handle = JsToolHandle { plugin_id: id, name: name.clone(),
            description, execute_fn };
        self.tools.blocking_lock().insert(name.clone(), handle);
        // Register with jcode's tool registry as "plugin:{name}"
        jcode_app_core::tool_registry().register(
            &format!("plugin:{name}"),
            Arc::new(JsPluginTool::new(name, Arc::clone(&self.tools))),
        );
    }

    pub async fn execute(&self, name: &str, input: serde_json::Value,
            ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let handle = self.tools.lock().await.get(name).cloned()
            .ok_or(ToolError::NotFound(name.into()))?;
        // Execute JS function via sandbox
        Ok(ToolOutput { output: "result".into(), title: None, metadata: None, images: vec![] })
    }
}
```

### 13.2 Tool Resolution (modified in agent loop)

```rust
async fn resolve_tool(name: &str) -> Option<Arc<dyn Tool>> {
    if let Some(pname) = name.strip_prefix("plugin:") {
        js_tool_registry().get_tool(pname).await
    } else if name.starts_with("mcp__") {
        resolve_mcp_tool(name)
    } else {
        builtin_tool_registry().get(name)
    }
}
```

---

## 14. Dual-Process Architecture

### 14.1 Server Plugin Initialization

```rust
// In jcode-app-core/src/server/server.rs

impl Server {
    async fn initialize_plugins(&self) -> Result<()> {
        let config = PluginConfig::from_config(&self.config);
        let rt_config = RuntimeConfig::from_config(&self.config);
        let runtime = RuntimeManager::new(rt_config)?;
        let dispatcher = Arc::new(RcuDispatcher::new());
        let registry = Arc::new(PluginRegistry::new(dispatcher.clone()));
        let loader = PluginLoader::new(DiscoveryPaths::default(), config, registry, runtime);
        let loaded = loader.load_all().await?;
        tracing::info!("Loaded {} server plugin(s)", loaded.len());
        self.plugin_system = Some(PluginSystem { dispatcher, registry, runtime, loader });
        Ok(())
    }
}
```

### 14.2 TUI Plugin System

```rust
pub struct TuiPluginSystem {
    plugins: Vec<Arc<TuiPlugin>>,
    slots: SlotRegistry,
    runtime: RuntimeManager,
}

pub struct TuiPluginApi {
    pub route: RouteApi,
    pub keymap: KeymapApi,
    pub ui: UiApi,
    pub slots: SlotApi,
    pub theme: ThemeApi,
    pub kv: KvApi,
    pub event: EventBusApi,
    pub lifecycle: LifecycleApi,
}

impl TuiPluginSystem {
    pub async fn load(config: &Config) -> Result<Self> {
        // Discover TUI plugins (kind == Tui | Both)
        // Load into QuickJS, inject TuiPluginApi
        // Register UI slots, keybindings, themes
        Ok(Self { plugins: vec![], slots: SlotRegistry::new(), runtime: RuntimeManager::new(...)? })
    }
}
```

### 14.3 Cross-Process Events

```rust
// Protocol extension
pub enum PluginServerEvent {
    Event { event: String, data: serde_json::Value },
    ToolResult { request_id: String, result: ToolOutput },
}
```

---

## 15. Configuration

### 15.1 TOML Config

```toml
# ~/.jcode/config.toml

[plugin]
enable = ["@scope/my-plugin"]
disable = ["@scope/broken-plugin"]
mode = "trusted"         # all | trusted | none | interactive
fail_closed = true

sources = [
    { type = "npm", package = "@scope/jcode-formatter" },
    { type = "file", path = "/home/user/.jcode/plugins/custom.ts" },
]

[plugin.settings."@scope/my-plugin"]
api_key = "sk-..."
max_results = 25

[plugin.features]
"@scope/my-plugin" = ["advanced"]

[plugin.plugins."@scope/my-plugin"]
enable = true
timeout_ms = 10000
```

### 15.2 Environment Variables

```bash
export JCODE_DISABLE_PLUGINS=1          # Disable all
export JCODE_SKIP_PLUGINS=1             # Skip hooks only
export JCODE_TEAM_WORKER=1              # Force deny
export JCODE_PLUGIN_MODE="trusted"      # Override mode
export JCODE_PLUGIN_DIR="$HOME/.jcode/plugins"
export JCODE_PLUGIN_DENY="rm,chmod,sudo"
```

### 15.3 Kill Switch Priority

```
1. JCODE_DISABLE_PLUGINS env      → none mode (highest)
2. JCODE_SKIP_PLUGINS env         → hooks skipped
3. JCODE_TEAM_WORKER env          → force deny
4. Config mode setting             → config.toml
5. CLI flag --plugin-mode         → runtime override
```

---

## 16. CLI Commands

### 16.1 Plugin Subcommand

```bash
jcode plugin --help
#
# Usage: jcode plugin <command> [options]
#
# Commands:
#   install     Install plugin from npm or path
#   uninstall   Remove a plugin
#   update      Update a plugin
#   list        List installed plugins
#   enable      Enable a plugin
#   disable     Disable a plugin
#   info        Show plugin details
#   check       Check plugin compatibility
#   audit       Show audit trail
#   doctor      Diagnose plugin issues
#
# Examples:
#   jcode plugin install @scope/my-plugin[advanced]
#   jcode plugin list --verbose
#   jcode plugin audit --recent 50
```

### 16.2 Rust CLI Implementation

```rust
#[derive(Subcommand)]
pub enum PluginCommand {
    Install { spec: String, #[arg(long)] yes: bool },
    Uninstall { package: String },
    Update { package: Option<String> },
    List { #[arg(long)] verbose: bool },
    Enable { package: String },
    Disable { package: String },
    Info { package: String },
    Audit { #[arg(long, default_value = "20")] recent: usize, #[arg(long)] json: bool },
    Doctor { #[arg(long)] fix: bool },
}

impl PluginCommand {
    pub async fn execute(self, config: &Config) -> Result<()> {
        match self {
            PluginCommand::Install { spec, yes } => {
                PluginManager::new(config).install(&spec, !yes).await?;
                println!("✅ Plugin '{spec}' installed");
            }
            PluginCommand::List { verbose } => {
                let plugins = PluginRegistry::list_installed().await?;
                for p in plugins {
                    let s = if p.enabled { "✅" } else { "⏸️" };
                    println!("  {s} {} v{}", p.name, p.version);
                    if verbose { println!("     Features: {}", p.features.join(", ")); }
                }
            }
            PluginCommand::Enable { package } => {
                PluginRegistry::set_enabled(&package, true).await?;
                println!("✅ Plugin '{package}' enabled");
            }
            PluginCommand::Disable { package } => {
                PluginRegistry::set_enabled(&package, false).await?;
                println!("⏸️ Plugin '{package}' disabled");
            }
            PluginCommand::Audit { recent, json } => {
                let entries = PluginSystem::audit_trail().get_recent(recent);
                if json { println!("{}", serde_json::to_string_pretty(&entries)?); }
                else { for e in &entries { println!("{} | {} | {}", e.timestamp, e.plugin_id, e.decision); } }
            }
            _ => {}
        }
        Ok(())
    }
}
```

---

## 17. Integration Points in Existing Code

### Module Impact Matrix

| Existing Module | Change | Type |
|----------------|--------|------|
| `jcode-base/src/config.rs` | Add `PluginConfig` section parsing | New config parser |
| `jcode-app-core/src/server/server.rs` | Call `initialize_plugins()` during startup | New initialization |
| `jcode-app-core/src/agent/turn_streaming_mpsc.rs` | Fire plugin events at 4 injection points | New hooks |
| `jcode-app-core/src/tool/mod.rs` | Accept `plugin:` prefixed tools | Registry extension |
| `jcode-app-core/src/tool/registry.rs` | Support dynamic tool registration | Registry API |
| `jcode-app-core/src/safety.rs` | Route PermissionRequest events through plugins | Event wiring |
| `jcode-app-core/src/cli/mod.rs` | Add `plugin` subcommand | New CLI |
| `jcode-protocol/src/requests.rs` | Add `PluginRequest` variants | Protocol extension |
| `jcode-protocol/src/events.rs` | Add `PluginServerEvent` variants | Protocol extension |
| `jcode-tui/src/run_shell.rs` | Initialize TuiPluginSystem | New initialization |
| `jcode-tui/src/app.rs` | Slot render hooks in draw loop | Render extension |
| `jcode-tui/src/keybinding.rs` | TUI plugin keymap integration | Keybinding extension |

### Cargo.toml Dependencies

```toml
# jcode-plugin-core
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
semver = "1.0"
thiserror = "2"
chrono = "0.4"

# jcode-plugin-runtime
[dependencies]
jcode-plugin-core = { path = "../jcode-plugin-core" }
rquickjs = { version = "0.7", features = ["parallel", "catch", "classes"] }
swc_core = { version = "1.0", features = ["ecma_transforms", "ecma_parser"] }
tokio = { workspace = true }
tokio-stream = "0.1"
futures = "0.3"
seahash = "4"
regex = "1"
tracing = { workspace = true }
serde_json = { workspace = true }
```

---

## 18. Test Plan

### 18.1 Unit Tests

| Test | Scope | What It Verifies |
|------|-------|-----------------|
| `manifest_from_package_json` | jcode-plugin-core | Parse valid/invalid package.json |
| `manifest_missing_field` | jcode-plugin-core | Error on missing jcode/pi field |
| `capability_chain_layers` | jcode-plugin-core | 5-layer check in correct order |
| `capability_chain_mode_none` | jcode-plugin-core | Mode::None denies everything |
| `capability_set_matches` | jcode-plugin-core | Glob/exact matching logic |
| `preflight_eval_detection` | jcode-plugin-runtime | Static analysis finds eval() |
| `preflight_suspicious_strings` | jcode-plugin-runtime | Detects rm -rf, sudo, etc. |
| `preflight_no_warnings_clean` | jcode-plugin-runtime | Clean code passes |
| `transpiler_ts_to_js` | jcode-plugin-runtime | SWC transpiles TS correctly |
| `transpiler_cache_hit` | jcode-plugin-runtime | Same hash returns cached |
| `dispatcher_register` | jcode-plugin-runtime | Handler added to bitmap |
| `dispatcher_has_handler` | jcode-plugin-runtime | O(1) bitmap check |
| `dispatcher_no_handler` | jcode-plugin-runtime | Empty bitmap returns false |
| `dispatcher_unregister` | jcode-plugin-runtime | Handler removed after unregister |
| `promise_bridge_send_recv` | jcode-plugin-runtime | Oneshot channel works |
| `handler_result_serde` | jcode-plugin-runtime | JSON round-trip |
| `dual_timeout_defaults` | jcode-plugin-runtime | Correct default durations |
| `event_timeout_selection` | jcode-plugin-runtime | Info vs actionable mapping |
| `plugin_id_format` | jcode-plugin-core | npm:/file:/builtin: prefixes |
| `kill_switch_env_vars` | jcode-plugin-runtime | Env vars set atomic flags |
| `audit_trail_circular` | jcode-plugin-runtime | Circular buffer eviction |
| `load_invalid_source` | jcode-plugin-runtime | Error on bad source path |
| `install_invalid_package` | jcode-plugin-runtime | Rejects bad package names |
| `sandbox_memory_limit` | jcode-plugin-runtime | QuickJS enforced |

### 18.2 Integration Tests

| Test | What It Verifies |
|------|-----------------|
| `plugin_discover_auto` | `.jcode/plugins/*.ts` files found at startup |
| `plugin_discover_npm` | npm installed packages found |
| `plugin_load_and_register` | Full load pipeline: discover→transpile→eval→register |
| `plugin_tool_execution` | Plugin-registered tool executes via `plugin:` prefix |
| `plugin_event_blocking` | PreToolUse handler blocks tool execution |
| `plugin_event_mutation` | PostToolUse handler modifies output |
| `plugin_permission_handler` | PermissionRequest handler returns Allow/Deny |
| `plugin_timeout_info` | Info hooks timeout at 500ms |
| `plugin_timeout_actionable` | Actionable hooks timeout at 5000ms |
| `plugin_timeout_permission` | Permission hooks have no timeout |
| `plugin_unregister_cleanup` | Plugin unregister removes handlers |
| `plugin_multiple_sources` | Config + auto + npm all load together |
| `tui_plugin_register` | TUI plugin registers slot |
| `tui_plugin_keybinding` | TUI plugin adds custom keybinding |
| `cross_process_event` | Server event forwarded to TUI |

### 18.3 E2E Tests

```typescript
// Example: plugin blocks dangerous command
// 1. Create .jcode/plugins/block-rm.ts with PreToolUse handler
// 2. Start jcode session
// 3. Send "remove all files" prompt
// 4. Verify rm tool is blocked with plugin message
// 5. Verify audit trail shows the block
```

---

## 19. Migration Strategy

### Phase 1: Foundation (Week 1-2)
- Create `jcode-plugin-core` crate with types, manifest, security
- Implement `PluginManifest`, `CapabilityChain`, `PluginEvent`
- Unit tests for core types

### Phase 2: Runtime (Week 3-4)
- Create `jcode-plugin-runtime` crate
- Implement QuickJS integration (runtime manager, sandbox, promise bridge)
- Implement SWC transpiler
- Implement RCU dispatcher
- Integration tests for runtime

### Phase 3: Integration (Week 5-6)
- Wire plugin system into jcode-app-core (server startup, agent loop)
- Add CLI plugin subcommand
- Add protocol extensions for plugin events
- Add config parsing for `[plugin]` section
- TUI plugin system basics

### Phase 4: Security & Polish (Week 7-8)
- Preflight static analysis
- Audit trail with `jcode plugin audit` command
- npm install pipeline (`jcode plugin install`)
- Kill switches and env vars
- Documentation for plugin authors
- Example plugins

---

## 20. Cross-Repo Reference Matrix

| Feature | opencode | oh-my-pi | pi-agent-rust | jcode (this plan) |
|---------|----------|----------|---------------|-------------------|
| JS Engine | None (Node/Bun) | None (Bun) | QuickJS + SWC | **QuickJS + SWC** |
| Plugin API | `Plugin = (input) => Hooks` | `default fn(pi)` | `pi.on(event, handler)` | **`pi.on(event, handler)`** |
| Tool Registration | `tool: { name: def }` | `registerTool()` | — | **`pi.registerTool()`** |
| Command Registration | — | `registerCommand()` | — | **`pi.registerCommand()`** |
| Provider Registration | `provider` hook | `registerProvider()` | — | **`pi.registerProvider()`** |
| Types | `Hooks` interface | `ExtensionAPI` | Typed enums | **`PluginAPI` (Rust + TS)** |
| Security | None | None | 5-layer chain | **5-layer chain** |
| Sandboxing | None | None | QuickJS heap | **QuickJS heap** |
| Preflight | None | None | Static analysis | **Static analysis** |
| Timeout | None | 30s extensions | Dual 500/5000ms | **Dual 500/5000ms** |
| Permission timeout | None | No timeout (hooks) | — | **No timeout** |
| Config | JSON (opencode.json) | TOML | TOML | **TOML (config.toml)** |
| Plugin format | npm packages | npm packages | Local `.ts` files | **npm + local `.ts`** |
| Feature toggles | — | `pkg[feature]` | — | **`pkg[feature]`** |
| Typed settings | — | JSON Schema | — | **SettingSchema enum** |
| Auto-discovery | `.opencode/plugin/*.ts` | `.omp/extensions/` | `.pi/plugins/` | **`.jcode/plugins/*.ts`** |
| Kill switches | — | `DISABLE_OMC` | `DISABLE_PLUGINS` | **`JCODE_DISABLE_PLUGINS`** |
| Audit trail | — | — | File-based | **In-memory ring buffer** |
| Dual process | Server/TUI | Monolithic | Monolithic | **Server/TUI (existing)** |
| Inter-plugin bus | — | `events: EventBus` | `events.on/emit` | **`pi.events`** |
| KV Store | — | — | — | **`pi.kv`** |
| fail_closed | — | — | `fail_closed_hooks` | **`fail_closed` config** |

---

## 21. Success Criteria

- [x] All core types defined (PluginManifest, PluginEvent, CapabilityChain, etc.)
- [x] Plugin can be loaded from `.jcode/plugins/*.ts`
- [x] Plugin can subscribe to events via `pi.on()`
- [x] Plugin can register tools via `pi.registerTool()`
- [x] Plugin can block tool execution (PreToolUse → Block)
- [x] Plugin can modify tool output (PostToolUse → modified_output)
- [x] Security: preflight analysis catches suspicious patterns
- [x] Security: 5-layer capability chain enforced
- [x] Security: kill switches disable all plugins
- [x] Performance: O(1) bitmap check for handler presence
- [x] Performance: RCU snapshot for zero-contention reads
- [x] Performance: dual timeouts prevent plugin hangs
- [x] CLI: `jcode plugin install/list/enable/disable/audit` work
- [x] Config: `[plugin]` section in config.toml parsed correctly
- [x] npm packages can be installed as plugins
- [x] TUI plugins can extend the UI
- [x] Audit trail records all security decisions
- [x] Fail-closed mode: plugin errors block operations
- [x] Unit tests pass for all core modules
- [x] Integration tests pass for load+dispatch flow
- [x] Example plugin works end-to-end

---

## 22. Known Limitations & Future Work

### v1 Limitations
- [ ] No WASM sandboxing — QuickJS only
- [ ] No hot-reload — plugins require restart
- [ ] No TUI render isolation — plugin crash affects UI
- [ ] No inter-plugin version resolution
- [ ] No plugin marketplace/search within jcode
- [ ] No plugin signing/verification
- [ ] KV store is single-node (no sync between serve/connect)
- [ ] SWC builds from source (slow first compile)

### v2 Stretch Goals
- [ ] WASM plugin support via wasmtime
- [ ] Plugin hot-reload (file watcher)
- [ ] Plugin marketplace subcommand (`jcode plugin search`)
- [ ] Signed plugin verification (cosign/minisign)
- [ ] Cross-plugin dependency resolution
- [ ] TUI render isolation (separate process?)
- [ ] Remote plugins (SSH, Docker)
- [ ] Plugin benchmarking (`jcode plugin bench`)
- [ ] GUI plugin config editor in TUI

### Design Decisions for Future
- WASM plugins would need different API surface (no JS interop)
- Hot-reload requires tearing down QuickJS context safely
- Plugin marketplace needs server-side registry
- Remote plugins need gRPC or WebSocket transport
