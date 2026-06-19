# jcode Plugin Author Guide

jcode plugins are TypeScript or JavaScript modules that run inside a QuickJS sandbox. They can listen to lifecycle events, register custom tools for the LLM to invoke, and interact with jcode through a constrained API surface -- all without access to the host process or filesystem except through declared capabilities.

The canonical reference implementation is at `examples/plugins/hello-plugin/`.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Package Structure](#package-structure)
- [Manifest Format](#manifest-format)
- [jcode API Reference](#jcode-api-reference)
- [Lifecycle Events](#lifecycle-events)
- [Capability Model](#capability-model)
- [ToolTier Model](#tooltier-model)
- [Distribution](#distribution)
- [Rust Workspace Crate Path](#rust-workspace-crate-path)
- [Testing](#testing)
- [Security Checklist](#security-checklist)

---

## Quick Start

### 1. Copy the hello-plugin scaffold

```bash
cp -r <jcode-repo>/examples/plugins/hello-plugin/ ~/my-plugin/
cd ~/my-plugin
```

### 2. Load the plugin into jcode

```bash
jcode plugin load ./my-plugin
```

On next start, jcode discovers the plugin, transpiles `index.ts` to JavaScript via SWC, evaluates it in a QuickJS sandbox, and injects the `jcode` global object.

### 3. Verify it loaded

```bash
jcode plugin list
```

You should see `file:hello-plugin` in the output.

### 4. Write your own handler

Edit `index.ts`:

```typescript
jcode.on("SessionStart", function(event) {
    jcode.logger.info("my-plugin: session started, id=" + event.sessionId);
});

jcode.registerTool({
    name: "greet",
    description: "Return a friendly greeting",
});
```

---

## Package Structure

A minimal plugin is a directory with two files:

```
my-plugin/
  package.json          # Manifest (jcode metadata under "jcode" key)
  index.ts              # Entry point, gets transpiled by SWC
```

The entry file can also be `index.js` (plain JavaScript, no transpilation). TypeScript is preferred and is stripped of types by SWC during loading.

---

## Manifest Format

Plugins declare identity, capabilities, and entry points in `package.json` under the `"jcode"` key.

```json
{
  "name": "hello-plugin",
  "version": "0.1.0",
  "description": "An example jcode plugin",
  "jcode": {
    "name": "Hello Plugin",
    "package_name": "hello-plugin",
    "version": "0.1.0",
    "kind": "server",
    "entry": { "server": "index.ts" },
    "description": "Real working example -- see index.ts for what it does."
  }
}
```

### Manifest Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | `string` | Yes | Display name |
| `package_name` | `string` | Yes | Unique identifier (must be unique across all loaded plugins) |
| `version` | `string` | Yes | Semver version |
| `kind` | `"server" \| "tui" \| "both"` | No | Where the plugin runs (default: `"server"`) |
| `entry` | `{ server?: string, tui?: string, both?: string }` | No | Entry point paths relative to the package root |
| `description` | `string` | No | Human-readable description |
| `author` | `string` | No | Plugin author |
| `license` | `string` | No | SPDX license identifier |
| `tier` | `"read" \| "write" \| "exec"` | No | Default ToolTier for all tools in this plugin |
| `capabilities` | `object` | No | Declared capabilities (see [Capability Model](#capability-model)) |
| `engines` | `{ jcode?: string }` | No | Required jcode version range |
| `tags` | `string[]` | No | Categorization tags |

The `package_name` field serves as a unique identity check: no two loaded plugins may share the same `package_name`. This prevents spoofing where a malicious plugin claims the identity of a trusted one.

---

## jcode API Reference

All plugin APIs are available through the global `jcode` object, injected into the QuickJS sandbox by the runtime's `PluginApiBindings`. The object is also aliased as `__jcode_api` for internal use.

### `jcode.on(event, handler)`

```typescript
jcode.on(event: string, handler: (event: any) => void | HandlerResult): void
```

Register an event handler. The handler receives an event object whose shape depends on the event type. See [Lifecycle Events](#lifecycle-events) for all events.

```typescript
jcode.on("SessionStart", function(event) {
    jcode.logger.info("session " + event.sessionId + " started");
});
```

### `jcode.registerTool(toolDef)`

```typescript
jcode.registerTool(toolDef: {
    name: string;
    description: string;
    parameters?: JSONSchema;
    handler?: (params: any) => any;
}): void
```

Register a custom tool that the LLM can invoke. The tool definition must have at minimum a `name` and `description`. Parameters follow JSON Schema format when provided.

```typescript
jcode.registerTool({
    name: "hello",
    description: "Say hello and return a greeting",
});
```

Tool names should be prefixed with the plugin name to avoid collisions (e.g., `analytics_report`, not `report`).

### `jcode.logger.*`

```typescript
jcode.logger.info(message: string): void
jcode.logger.warn(message: string): void
jcode.logger.error(message: string): void
jcode.logger.debug(message: string): void
```

Structured logger backed by jcode's `tracing` crate. Messages appear in the jcode debug log at their respective levels. Enable with `RUST_LOG=jcode_plugin_runtime=debug` to see plugin log output.

```typescript
jcode.logger.info("[my-plugin] Starting up");
jcode.logger.warn("[my-plugin] Deprecated config key used");
jcode.logger.error("[my-plugin] Failed to parse input");
jcode.logger.debug("[my-plugin] Internal state: " + JSON.stringify(state));
```

### `jcode.kv.*`

```typescript
jcode.kv.get(key: string): string
jcode.kv.set(key: string, value: string): void
```

Per-plugin durable key-value storage that persists across sessions. Values are strings; serialize objects with `JSON.stringify` / `JSON.parse`.

```typescript
jcode.kv.set("hello-plugin:loaded-at", "test");
var stored = jcode.kv.get("hello-plugin:loaded-at");
```

### `jcode.uuid()`

```typescript
jcode.uuid(): string
```

Generate a UUID v4 string. Backed by the `uuid` crate.

```typescript
var instanceUuid = jcode.uuid();
jcode.logger.info("instance uuid = " + instanceUuid);
```

### `jcode.sleep(ms)`

```typescript
jcode.sleep(ms: number): void
```

Block the sandbox thread for the given number of milliseconds. Hard-capped at 5000 ms (5 seconds) to prevent plugins from blocking the QuickJS thread indefinitely.

```typescript
jcode.sleep(100); // wait 100 ms
```

### `jcode.cwd`

```typescript
jcode.cwd: string
```

Read-only string containing the current working directory of the jcode process at the time the plugin was loaded.

```typescript
jcode.logger.info("Working directory: " + jcode.cwd);
```

### `jcode.getConfig(key)`

```typescript
jcode.getConfig(key: string): string
```

Read a plugin configuration value from jcode's config system. Returns an empty string if the key is not set.

```typescript
var apiKey = jcode.getConfig("my-plugin.apiKey");
```

### `jcode.id`

```typescript
jcode.id: string
```

The plugin's unique identifier, derived from its `package_name`.

### `jcode.name`

```typescript
jcode.name: string
```

The plugin's display name, derived from its manifest `name` field.

### `jcode.version`

```typescript
jcode.version: string
```

The plugin's version string, derived from its manifest `version` field.

---

## Lifecycle Events

Events are dispatched to handlers registered via `jcode.on()`. The full set of supported event names, matched against the Rust `PluginEvent` enum in `jcode-plugin-core`:

| Category | Events | Description |
|----------|--------|-------------|
| **Tool** | `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `ToolExecutionStart`, `ToolExecutionEnd` | Tool execution lifecycle |
| **Session** | `SessionStart`, `SessionEnd`, `SessionSwitch`, `SessionCompact`, `SessionBeforeCompact`, `SessionShutdown` | Session lifecycle |
| **Agent** | `AgentStart`, `AgentEnd` | Agent lifecycle |
| **Turn** | `TurnStart`, `TurnEnd` | Conversation turn lifecycle |
| **Message** | `MessageStart`, `MessageEnd` | Message production lifecycle |
| **Compact** | `PreCompact`, `PostCompact`, `AutoCompactionStart` | Context compaction |
| **Permission** | `PermissionRequest`, `PermissionDenied` | Permission system |
| **Task** | `TaskCreated`, `TaskCompleted` | Task management |
| **Other** | `UserPromptSubmit`, `Stop`, `Notification` | Misc events |

### SessionStart

Fired when a new session begins.

```typescript
// Input:
{
    sessionId: string;        // New session ID
    projectDir: string;       // Project directory path
    model: string;            // Model being used (e.g., "claude-4")
    provider: string;         // Provider name (e.g., "anthropic")
}
```

### SessionEnd

Fired when a session ends. Use for cleanup and state persistence.

```typescript
// Input:
{
    sessionId: string;
    durationSeconds: number;
    messageCount: number;
}
```

### PreToolUse

Fired before a tool is executed. Can block or modify the tool input.

```typescript
// Input:
{
    toolName: string;         // Name of the tool about to run
    toolInput: any;           // Tool parameters
    sessionId: string;        // Current session ID
}

// Return value (to modify behavior):
{
    block?: string;           // If set, tool is blocked with this reason
    modifiedInput?: any;      // If set, replaces the tool input
}
```

### PostToolUse

Fired after a tool returns successfully.

```typescript
// Input:
{
    toolName: string;
    toolInput: any;
    toolOutput: any;
    durationMs: number;
    success: boolean;
    sessionId: string;
}

// Return value:
{
    modifiedOutput?: any;     // Replaces the tool output
}
```

### TurnStart / TurnEnd

Fired at the start and end of a conversation turn.

```typescript
// TurnStart input:
{ sessionId: string, turnNumber: number, messages: any }

// TurnEnd input:
{ sessionId: string, turnNumber: number, durationMs: number }
```

### UserPromptSubmit

Fired when the user submits a prompt. Can modify the prompt.

```typescript
// Input: { content: string, sessionId: string }
// Return: { modifiedPrompt?: string }
```

### Notification

Fired for system notifications. Can suppress or modify.

```typescript
// Input: { level: string, message: string, sessionId?: string }
// Return: { suppress?: boolean, modifiedMessage?: string }
```

### PermissionRequest

Fired when a permission decision is needed. Can approve, deny, or defer to user.

```typescript
// Input: { action: string, toolName?: string, target?: string, sessionId: string }
// Return: { decision?: "allow" | "deny" | "ask", message?: string }
```

### Stop

Fired when the agent stops.

```typescript
// Input: { sessionId: string, reason: string }
// Return: { reason: string }
```

---

## Capability Model

Plugins declare the capabilities they need in their manifest. The runtime enforces these through the `CapabilityChain` (a 5-layer evaluation pipeline):

```
Layer 1: Plugin deny list  -->  Layer 2: Global deny list  -->
Layer 3: Plugin allow list -->  Layer 4: Global allow list -->
Layer 5: Mode fallback (Strict / Permissive / Prompt / Disabled)
```

### Declared Capabilities

The `PluginCapabilities` struct supports these fields in the manifest:

| Capability | Type | Description |
|------------|------|-------------|
| `fs_read` | `string[]` | Filesystem read paths (glob patterns relative to plugin root) |
| `fs_write` | `string[]` | Filesystem write paths (glob patterns) |
| `http_hosts` | `string[]` | HTTP hosts the plugin may call (exact host, `*.suffix`, or `*`) |
| `env_read` | `string[]` | Environment variable names the plugin may read |
| `shell_commands` | `string[]` | Shell commands the plugin may execute (glob patterns, e.g. `git *`) |
| `requires_tools` | `string[]` | Tool names this plugin requires to be present |
| `max_hostcalls_per_sec` | `number` | Maximum host calls per second (quota) |
| `max_tool_duration_secs` | `number` | Maximum wall-clock seconds per tool invocation |
| `max_bytes_written` | `number` | Maximum cumulative bytes the plugin may write to disk |

### Policy Modes

| Mode | Behavior |
|------|----------|
| `Strict` | Deny by default; explicit allow required |
| `Permissive` | Allow by default; audit everything |
| `Prompt` | Deny unknown; prompt for ambiguous (default) |
| `Disabled` | All plugin calls disabled (kill switch) |

Example capability declaration in `package.json`:

```json
{
  "jcode": {
    "capabilities": {
      "fs_read": ["$HOME/.jcode/data"],
      "fs_write": ["$HOME/.jcode/data/my-plugin"],
      "http_hosts": ["api.github.com"],
      "env_read": ["HOME", "USER"],
      "shell_commands": ["git *"],
      "max_hostcalls_per_sec": 100
    }
  }
}
```

---

## ToolTier Model

Every registered tool has a `ToolTier` that describes its risk level. The tier determines what approval prompts the user sees and which permission mode gates apply.

```typescript
enum ToolTier {
    Read,    // Pure read of already-loaded data, no I/O, no mutation.
    Write,   // Mutates workspace or session state but does not spawn processes.
    Exec,    // Spawns subprocesses, makes network calls, or executes code.
}
```

### How Tier Maps to Permission Mode

| ToolTier | Strict | Prompt | Permissive |
|----------|--------|--------|------------|
| **Read** | Ask | Allow | Allow |
| **Write** | Ask | Ask | Allow |
| **Exec** | Deny | Ask | Allow |

- **Read** tools: `grep`, `list_files`, `read_file`, `search_code` -- safe to auto-allow in Prompt mode.
- **Write** tools: `write_file`, `edit`, `rename`, `delete` -- prompt unless in Permissive mode.
- **Exec** tools: `bash`, `fetch`, `browser`, `docker` -- always denied in Strict mode, prompted otherwise.

The default tier for an undeclared tool is `Exec` (fail-closed). Plugin authors can set a default tier in the manifest and override per-tool in the `approval` policy:

```json
{
  "jcode": {
    "tier": "read",
    "approval": {
      "kind": "per_tool",
      "overrides": {
        "my_plugin_deploy": "exec",
        "my_plugin_list": "read"
      }
    }
  }
}
```

---

## Distribution

jcode has exactly **three** distribution paths. There is no npm registry, no marketplace, and no publish step.

### 1. Local Path

Load a plugin from a local directory:

```bash
jcode plugin load ./my-plugin
jcode plugin load /absolute/path/to/my-plugin
```

The directory must contain `package.json` and an entry file referenced by the manifest.

### 2. Git Clone

Clone a plugin from a git repository and load it:

```bash
jcode plugin load https://github.com/user/my-plugin.git
```

jcode clones the repository into its plugin cache and loads the plugin from there. Updates are manual (`jcode plugin update`).

### 3. Rust Workspace Crate

For Rust plugin authors, see the [Rust Workspace Crate Path](#rust-workspace-crate-path) section below.

---

## Rust Workspace Crate Path

Rust developers can write plugins as workspace crates that are compiled directly into jcode. This is the **preferred** distribution path for Rust developers because it avoids the QuickJS sandbox overhead and gives full access to Rust's ecosystem.

### Structure

```
crates/jcode-ext-my-plugin/
  Cargo.toml
  src/
    lib.rs
```

### Cargo.toml

```toml
[package]
name = "jcode-ext-my-plugin"
version = "0.1.0"

[lib]
crate-type = ["lib"]

[dependencies]
jcode-plugin-core = { path = "../jcode-plugin-core" }
inventory = "0.3"
```

### Registration

Use `inventory::submit!` to register the plugin at compile time:

```rust
use jcode_plugin_core::manifest::PluginManifest;
use jcode_plugin_core::PluginEvent;
use jcode_plugin_core::events::{EventInput, HandlerResult};
use jcode_plugin_core::types::PluginId;
use std::sync::Arc;

inventory::submit! {
    MyPlugin::new()
}

pub struct MyPlugin {
    manifest: PluginManifest,
}

impl MyPlugin {
    pub fn new() -> Self {
        Self {
            manifest: PluginManifest {
                name: "My Plugin".into(),
                package_name: "my-plugin".into(),
                version: "0.1.0".into(),
                ..Default::default()
            },
        }
    }
}

// jcode-plugin-core defines traits that plugin crates implement.
// At build time, jcode discovers all inventory::submit! entries
// and registers them in the plugin system.
```

### Adding to the Workspace

Add the crate to the root `Cargo.toml`:

```toml
[dependencies]
jcode-ext-my-plugin = { path = "crates/jcode-ext-my-plugin" }
```

Build jcode and the plugin is compiled in:

```bash
cargo build --bin jcode
```

The plugin appears in `jcode plugin list` alongside file-based plugins.

---

## Testing

### Integration Test Pattern

The canonical integration test pattern lives in `crates/jcode-plugin-runtime/src/integration_tests.rs`. It loads the real hello-plugin example and verifies the full pipeline:

1. `PluginLoader::scan_directory` discovers the plugin directory.
2. `PreflightAnalyzer::analyze` runs static analysis.
3. `Transpiler::transpile` converts TypeScript to JavaScript via SWC.
4. `RuntimeManager::create_sandbox` creates a QuickJS runtime.
5. `PluginApiBindings::install` injects the `jcode` object.
6. `SandboxContext::eval` runs the JavaScript.
7. Plugin code calls `jcode.on(...)`, `jcode.registerTool(...)`, etc.
8. `RcuDispatcher::commit` finalizes handler registration.
9. Assertions verify handlers and tools are registered.

### Debug Logging

```bash
RUST_LOG=jcode_plugin_runtime=debug jcode
```

This enables tracing for:
- Plugin discovery and loading
- Preflight analysis results
- Event dispatch to handlers
- Tool registration
- Capability checks

### Audit Trail

View the plugin audit trail:

```bash
jcode plugin audit
jcode plugin audit --recent 50 --json
```

The audit trail is a ring buffer (default capacity configurable) that logs every capability access with plugin ID, resource, action, and decision.

### Preflight Analysis

Static analysis checks run before any plugin code executes:

| Pattern | Severity | Effect |
|---------|----------|--------|
| `eval()` | Warning | Logged, plugin still loads |
| `new Function()` | Warning | Logged, plugin still loads |
| `require()` | Warning | Not available in sandbox |
| `exec()` / `spawn()` without shell capability | Warning | Declare `shell_commands` capability |
| `rm -rf`, `sudo`, `chmod 777` | **Block** | Plugin loading is prevented |

---

## Security Checklist

Before distributing a plugin, verify each item:

1. **Unique package_name**: The `package_name` must be unique across all plugins. This is enforced at load time and prevents identity spoofing.

2. **Minimum capabilities**: Declare only the capabilities the plugin needs. Prefer specific paths over broad patterns (e.g., `$HOME/.jcode/data/my-plugin` over `$HOME`).

3. **Explicit ToolTier**: Set a `tier` in the manifest. Defaulting to `Exec` is safe but will trigger approval prompts for tools that could be `Read` or `Write`.

4. **Audit trail compatibility**: Every capability access is logged. The plugin should not produce excessive log entries that fill the audit ring buffer.

5. **No shell access without reason**: `shell_commands` capability is powerful. Prefer registering a tool that jcode's built-in `Bash` tool can handle.

6. **Timeout awareness**: Handler execution is subject to timeouts (default 5000 ms for actionable events, 500 ms for informational events). Long-running operations should complete within these limits.

7. **UUID uniqueness**: Use `jcode.uuid()` for identifier generation instead of rolling your own.

8. **Sleep cap**: `jcode.sleep()` is capped at 5000 ms. Do not rely on longer sleeps for timing logic.

9. **No eval**: The preflight analyzer catches `eval()` usage. Use regular function calls instead.

10. **No require / import**: The QuickJS sandbox does not support CommonJS `require()` or dynamic `import()`. All code must be self-contained in the entry file.

11. **Cross-plugin isolation**: Each plugin runs in its own QuickJS context. Plugins cannot access each other's globals or `jcode.kv` namespaces.

12. **Test the full pipeline**: Run `jcode plugin load ./my-plugin` and verify the plugin loads without warnings. Check `jcode plugin audit` after exercising the plugin's functionality.
