# Implementation Plan: Disable Env Vars (JCODE_DISABLE_*)
> Generated from research across 9 repos + jcode codebase deep-dive
> Goal: Centralize all JCODE_DISABLE_* env vars into a cached DisableRegistry, restructure scattered env var patterns

---

## 1. Executive Summary

jcode has ~90 `JCODE_*` env vars but they are **unstructured**: config overrides, feature toggles, and disable/kill-switches mixed in one monolithic `apply_env_overrides()` function, plus more env vars checked ad-hoc across the codebase (remote_diff.rs, app.rs, power_inhibit.rs, ui_frame_metrics.rs, etc.). There is no master kill-switch, no caching, no centralized pattern.

This plan creates a **`DisableRegistry`** — a singleton loaded once at startup via `LazyLock`, caching all `JCODE_DISABLE_*` env vars, with a master kill-switch (`JCODE_DISABLE_ALL=1`), selective skip lists (`JCODE_DISABLE_HOOK=...`, `JCODE_DISABLE_TOOL=...`), and a unified API (`disabled(DisableFlag) -> bool`, `hook_disabled("name") -> bool`). Every scattered env var check is consolidated into this registry. The scope is strictly **env var restructuring** — no behavioral changes to hooks, plugins, or features (those come in separate implementation phases).

The design is inspired by oh-my-claudecode's `DISABLE_OMC`/`OMC_SKIP_HOOKS`/`OMC_TEAM_WORKER` 3-tier system (bridge.ts:3024-3031) and claude-code's `isEnvTruthy()` helper pattern.

---

## 2. Architecture Decision

### Chosen Approach
**Centralized DisableRegistry** — a cached singleton loaded once at process start, providing `disabled()` and `*_disabled()` methods. Env vars never change during process lifetime, so a `LazyLock<DisableRegistry>` is safe, efficient, and thread-safe.

### Alternatives Considered

| Approach | Source Repo | Pros | Cons | Decision |
|----------|-------------|------|------|----------|
| Centralized cached registry (chosen) | oh-my-claudecode `getSkipHooks()` | O(1) lookups, single source of truth, thread-safe via LazyLock | Must restart to change env vars | ✅ Best for env nature |
| Fresh read on every access | jcode current | Reacts to env var changes | Costly syscalls, scattered pattern | ❌ No benefit |
| Config file section | Codex features.toml | Persistent, documented | Slower to toggle at deploy time | ❌ Env var is faster for infra |
| Middleware check at injection points | — | Fine-grained | Duplicated logic | ❌ Worse than centralized |

### Pattern Synthesis from Reference Repos

| Pattern | Source | Applied in this plan |
|---------|--------|---------------------|
| Master kill-switch at hook entry | oh-my-claudecode bridge.ts:3025 | `JCODE_DISABLE_ALL=1` disables all subsystems |
| Comma-separated skip lists | oh-my-claudecode getSkipHooks():3002 | `JCODE_DISABLE_HOOK=...`, `JCODE_DISABLE_TOOL=...` |
| Cached skip list (env vars static) | oh-my-claudecode `_cachedSkipHooks` | `LazyLock<DisableRegistry>` loads once |
| Truthy boolean parser | claude-code isEnvTruthy() | `is_env_truthy()`: "1"\|"true"\|"yes"\|"on" |
| Config env separate from kill-switch | — | `env_overrides.rs` → pure config only |
| Feature-level kill flags | Codex + oh-my-claudecode team_server_disable | Per-subsystem: Hooks, Plugins, Memory, Swarm, Mcp, etc. |

---

## 3. Data Structures & Types

### File: `crates/jcode-base/src/disable.rs`

```rust
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::LazyLock;
use strum::{Display, EnumString};

// ─── DisableFlag Enum ───────────────────────────────────────────────

/// Each variant corresponds to a `JCODE_DISABLE_<NAME>=1` env var.
/// Parsed once at startup, cached forever.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Display, EnumString)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum DisableFlag {
    /// Master kill-switch: JCODE_DISABLE_ALL=1 disables everything
    All,
    Hooks,
    Plugins,
    Memory,
    Swarm,
    Ambient,
    Mcp,
    Compaction,
    Telemetry,
    Mermaid,
    DesktopAnimation,
    PowerInhibit,
}

impl DisableFlag {
    /// All subsystem flags (excludes All — All is meta)
    pub fn all_subsystems() -> &'static [DisableFlag] {
        &[
            DisableFlag::Hooks,
            DisableFlag::Plugins,
            DisableFlag::Memory,
            DisableFlag::Swarm,
            DisableFlag::Ambient,
            DisableFlag::Mcp,
            DisableFlag::Compaction,
            DisableFlag::Telemetry,
            DisableFlag::Mermaid,
            DisableFlag::DesktopAnimation,
            DisableFlag::PowerInhibit,
        ]
    }

    /// Map flag → env var name
    pub fn env_var(&self) -> &'static str {
        match self {
            DisableFlag::All => "JCODE_DISABLE_ALL",
            DisableFlag::Hooks => "JCODE_DISABLE_HOOKS",
            DisableFlag::Plugins => "JCODE_DISABLE_PLUGINS",
            DisableFlag::Memory => "JCODE_DISABLE_MEMORY",
            DisableFlag::Swarm => "JCODE_DISABLE_SWARM",
            DisableFlag::Ambient => "JCODE_DISABLE_AMBIENT",
            DisableFlag::Mcp => "JCODE_DISABLE_MCP",
            DisableFlag::Compaction => "JCODE_DISABLE_COMPACTION",
            DisableFlag::Telemetry => "JCODE_DISABLE_TELEMETRY",
            DisableFlag::Mermaid => "JCODE_DISABLE_MERMAID",
            DisableFlag::DesktopAnimation => "JCODE_DISABLE_DESKTOP_ANIMATION",
            DisableFlag::PowerInhibit => "JCODE_DISABLE_POWER_INHIBIT",
        }
    }
}

// ─── DisableRegistry ────────────────────────────────────────────────

/// Centralized, cached, thread-safe registry of all disabled features.
/// Loaded ONCE at first access via LazyLock.
/// Env vars do not change during process lifetime.
pub struct DisableRegistry {
    /// Boolean disable flags
    flags: HashSet<DisableFlag>,
    /// Selective skip lists (parsed from comma-separated env vars)
    disabled_hooks: HashSet<String>,
    disabled_tools: HashSet<String>,
    disabled_animations: HashSet<String>,
    disabled_features: HashSet<String>,
}

impl DisableRegistry {
    /// Global singleton, lazily initialized.
    pub fn global() -> &'static Self {
        static INSTANCE: LazyLock<DisableRegistry> = LazyLock::new(Self::load_from_env);
        &INSTANCE
    }

    /// Load and cache ALL JCODE_DISABLE_* state at once.
    fn load_from_env() -> Self {
        let mut flags: HashSet<DisableFlag> = HashSet::new();

        // Scan all known DisableFlag env vars
        let all_flag = DisableFlag::All;
        for flag in std::iter::once(&all_flag).chain(DisableFlag::all_subsystems()) {
            if is_env_truthy(flag.env_var()) {
                flags.insert(*flag);
            }
        }

        // Master kill overrides all subsystems
        if flags.contains(&DisableFlag::All) {
            flags.extend(DisableFlag::all_subsystems());
        }

        // Selective skip lists
        let disabled_hooks = parse_comma_list("JCODE_DISABLE_HOOK");
        let disabled_tools = parse_comma_list("JCODE_DISABLE_TOOL");
        let disabled_animations = parse_comma_list("JCODE_DISABLE_ANIMATION");
        let disabled_features = parse_comma_list("JCODE_DISABLE_FEATURE");

        Self {
            flags,
            disabled_hooks,
            disabled_tools,
            disabled_animations,
            disabled_features,
        }
    }

    /// Is a whole subsystem disabled?
    pub fn disabled(&self, flag: DisableFlag) -> bool {
        self.flags.contains(&flag)
    }

    /// Is a specific hook type disabled (via selective skip or subsystem kill)?
    pub fn hook_disabled(&self, hook_name: &str) -> bool {
        self.disabled(DisableFlag::Hooks) || self.disabled_hooks.contains(hook_name)
    }

    /// Is a specific tool disabled?
    pub fn tool_disabled(&self, tool_name: &str) -> bool {
        self.disabled(DisableFlag::All) || self.disabled_tools.contains(tool_name)
    }

    /// Is a specific animation disabled?
    pub fn animation_disabled(&self, anim_name: &str) -> bool {
        self.disabled(DisableFlag::DesktopAnimation) || self.disabled_animations.contains(anim_name)
    }

    /// Is a specific experiment/feature disabled?
    pub fn feature_disabled(&self, feature_name: &str) -> bool {
        self.disabled_features.contains(feature_name)
    }

    /// Get all disabled hook names (for error/doctor output)
    pub fn all_disabled_hooks(&self) -> Vec<&str> {
        let mut hooks: Vec<&str> = self.disabled_hooks.iter().map(String::as_str).collect();
        hooks.sort();
        hooks
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Parse a boolean env var: "1", "true", "yes", "on" → true
fn is_env_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

/// Parse a comma-separated list env var into a HashSet
fn parse_comma_list(key: &str) -> HashSet<String> {
    std::env::var(key)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

// ─── Resettable for testing ─────────────────────────────────────────

#[cfg(test)]
impl DisableRegistry {
    /// Reset the global singleton (for test isolation).
    /// NOTE: This creates a race window if other threads are calling global().
    /// Only safe in single-threaded test contexts.
    pub fn reset_for_testing(registry: Self) {
        unsafe {
            let static_ref: &'static LazyLock<DisableRegistry> = &std::mem::transmute::<_, &'static LazyLock<DisableRegistry>>(
                &INSTANCE as *const LazyLock<DisableRegistry>
            );
            *static_ref = LazyLock::new(|| registry);
        }
    }
}
```

### Migration Map: Existing Env Vars → New System

| Current Env Var | Location | → New Home | Action |
|----------------|----------|-----------|--------|
| `JCODE_DISABLE_BASE_TOOLS` | env_overrides.rs:115 | `JCODE_DISABLE_TOOL=base` | Migrate and deprecate |
| `JCODE_DISABLED_TOOLS` | env_overrides.rs:112 | `JCODE_DISABLE_TOOL=...` | Rename check |
| `JCODE_DISABLED_ANIMATIONS` | env_overrides.rs:216 | `JCODE_DISABLE_ANIMATION=...` | Rename check |
| `JCODE_NO_TELEMETRY` | onboarding.rs:53 | `JCODE_DISABLE_TELEMETRY` | Migrate and deprecate |
| `JCODE_DISABLE_POWER_INHIBIT` | power_inhibit.rs:3 | `DisableFlag::PowerInhibit` | Redirect to registry |
| `JCODE_DESKTOP_REDUCED_MOTION` | animation.rs:10 | `JCODE_DISABLE_DESKTOP_ANIMATION` | Migrate and deprecate |
| `JCODE_SHOW_DIFFS` | remote_diff.rs:83 | `JCODE_DIFF_MODE` (config) | No change (is config) |
| `JCODE_TUI_SLOW_FRAME_MS` | ui_frame_metrics.rs:266 | → disable.rs | Migrate |
| `JCODE_TUI_FLICKER_DETECTION` | ui_frame_metrics.rs:284 | → disable.rs | Migrate |
| `JCODE_MEMORY_ENABLED` | env_overrides.rs:250 | Stay (is config toggle) | No change |
| `JCODE_SWARM_ENABLED` | env_overrides.rs:255 | Stay (is config toggle) | No change |

---

## 4. Core Implementation

### Phase 1: Create `crates/jcode-base/src/disable.rs`

Full implementation as described in Section 3.

### Phase 2: Add `pub mod disable;` to `crates/jcode-base/src/lib.rs`

```rust
// In lib.rs, add alongside existing modules:
pub mod disable;
```

### Phase 3: Inject DisableRegistry into Config (first-use at Config::load)

```rust
// In config.rs, at the start of Config::load():
impl Config {
    pub fn load() -> Arc<Self> {
        // ⚡ Trigger DisableRegistry initialization early
        // This ensures env vars are read before any config-dependent code runs
        let _ = disable::DisableRegistry::global();

        // ... existing load logic ...
    }
}
```

### Phase 4: Migrate `env_overrides.rs` — remove disable vars

```rust
// IN env_overrides.rs: REMOVE these blocks
// Line 112-113: JCODE_DISABLED_TOOLS → moved to DisableRegistry
// Line 115-119: JCODE_DISABLE_BASE_TOOLS → migrated to JCODE_DISABLE_TOOL=base
// Line 216-218: JCODE_DISABLED_ANIMATIONS → moved to DisableRegistry

// These fields in Config struct can remain (for backward compat),
// but they should also check DisableRegistry as a second source.
// OR: deprecate them in favor of DisableRegistry entirely.
```

### Phase 5: Migrate each scattered env var

**power_inhibit.rs example:**
```rust
// BEFORE:
const DISABLE_ENV: &str = "JCODE_DISABLE_POWER_INHIBIT";
pub fn is_enabled() -> bool {
    !std::env::var_os(DISABLE_ENV).is_some()
}

// AFTER:
pub fn is_enabled() -> bool {
    // Backward compat: check both old env var and new registry
    let old_kill = std::env::var_os("JCODE_DISABLE_POWER_INHIBIT").is_some();
    let new_kill = jcode_base::disable::DisableRegistry::global()
        .disabled(jcode_base::disable::DisableFlag::PowerInhibit);
    !(old_kill || new_kill)
}
```

**Then in a follow-up PR**, migrate callers to only use `DisableRegistry` and remove the old env var checks.

### Phase 6: Deprecation Warnings

In `env_overrides.rs`, add a deprecation logger for old env vars:

```rust
// At top of apply_env_overrides():
// Deprecated env vars — still supported but log warning
#[cfg(not(test))]
{
    if std::env::var("JCODE_DISABLE_BASE_TOOLS").is_ok() {
        tracing::warn!("JCODE_DISABLE_BASE_TOOLS is deprecated, use JCODE_DISABLE_TOOL=base instead");
    }
    if std::env::var("JCODE_NO_TELEMETRY").is_ok() {
        tracing::warn!("JCODE_NO_TELEMETRY is deprecated, use JCODE_DISABLE_TELEMETRY=1 instead");
    }
}
```

---

## 5. Migration from Scattered Env Vars

### Current State Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                   Env Var System (Current)                    │
│                                                              │
│  env_overrides.rs (615 lines, 90+ vars mixed)                │
│  ├── Keybindings (JCODE_*_KEY)                               │
│  ├── Dictation                                               │
│  ├── Tools (JCODE_DISABLE_BASE_TOOLS, JCODE_DISABLED_TOOLS)  │
│  ├── Display + Animations (JCODE_DISABLED_ANIMATIONS)        │
│  ├── Features (JCODE_MEMORY_ENABLED, etc.)                   │
│  ├── Provider, ACP, Shell, Safety, Gateway...                │
│  └── ...                                                     │
│                                                              │
│  Scattered (outside config system):                          │
│  ├── power_inhibit.rs  → JCODE_DISABLE_POWER_INHIBIT         │
│  ├── animation.rs      → JCODE_DESKTOP_REDUCED_MOTION        │
│  ├── ui_frame_metrics → JCODE_TUI_SLOW_FRAME_MS / FLICKER    │
│  ├── onboarding.rs    → JCODE_NO_TELEMETRY                   │
│  └── ...                                                     │
└──────────────────────────────────────────────────────────────┘
```

### Target State Diagram

```
┌─────────────────────────────────────────────┐
│           DisableRegistry (disable.rs)       │
│                                              │
│  LazyLock<DisableRegistry>                   │
│  ├── flags: HashSet<DisableFlag>             │
│  ├── disabled_hooks: HashSet<String>         │
│  ├── disabled_tools: HashSet<String>         │
│  ├── disabled_animations: HashSet<String>    │
│  └── disabled_features: HashSet<String>      │
│                                              │
│  Methods:                                    │
│  ├── disabled(flag) -> bool                  │
│  ├── hook_disabled(name) -> bool             │
│  ├── tool_disabled(name) -> bool             │
│  └── animation_disabled(name) -> bool        │
└───────────────────┬─────────────────────────┘
                    │ global() cached reference
                    ▼
┌─────────────────────────────────────────────┐
│           Config System (clean)              │
│                                              │
│  env_overrides.rs (only CONFIG overrides)    │
│  ├── Keybindings                             │
│  ├── Dictation                               │
│  ├── Display (no disable/animations)         │
│  ├── Features (toggles only, not disable)    │
│  ├── Provider, ACP, Shell, Safety, Gateway   │
│  └── ...                                     │
└─────────────────────────────────────────────┘
```

### Migration Steps

| Step | What | Files Changed | Risk |
|------|------|--------------|------|
| 1 | Create disable.rs | 1 new file | None (no consumers yet) |
| 2 | Register in lib.rs | 1 | None |
| 3 | Init at Config::load() | 1 | Low |
| 4 | Migrate JCODE_DISABLE_BASE_TOOLS → JCODE_DISABLE_TOOL | env_overrides.rs + tool_registry | Medium |
| 5 | Migrate JCODE_DISABLED_TOOLS → JCODE_DISABLE_TOOL | env_overrides.rs | Low |
| 6 | Migrate JCODE_DISABLED_ANIMATIONS → JCODE_DISABLE_ANIMATION | env_overrides.rs | Low |
| 7 | Migrate JCODE_DISABLE_POWER_INHIBIT → DisableFlag | power_inhibit.rs | Low |
| 8 | Migrate JCODE_DESKTOP_REDUCED_MOTION → JCODE_DISABLE_DESKTOP_ANIMATION | animation.rs | Low |
| 9 | Migrate JCODE_NO_TELEMETRY → JCODE_DISABLE_TELEMETRY | onboarding.rs + telemetry | Low |
| 10 | Migrate JCODE_TUI_SLOW_FRAME_MS / FLICKER → disable.rs | ui_frame_metrics.rs | Low |
| 11 | Remove deprecated env var checks from env_overrides.rs | env_overrides.rs | Low |
| 12 | Add deprecation warnings for old vars | env_overrides.rs | None |

---

## 6. Configuration & Wiring

### Env Var Reference (all JCODE_DISABLE_*)

| Env Var | Values | Default | Effect |
|---------|--------|---------|--------|
| `JCODE_DISABLE_ALL` | 1/true/yes/on | (unset) | Master kill — disables ALL features below |
| `JCODE_DISABLE_HOOKS` | 1/true/yes/on | (unset) | Kill all hooks |
| `JCODE_DISABLE_PLUGINS` | 1/true/yes/on | (unset) | Kill all plugins |
| `JCODE_DISABLE_MEMORY` | 1/true/yes/on | (unset) | Disable memory system |
| `JCODE_DISABLE_SWARM` | 1/true/yes/on | (unset) | Disable swarm mode |
| `JCODE_DISABLE_AMBIENT` | 1/true/yes/on | (unset) | Disable ambient mode |
| `JCODE_DISABLE_MCP` | 1/true/yes/on | (unset) | Disable MCP tools |
| `JCODE_DISABLE_COMPACTION` | 1/true/yes/on | (unset) | Disable compaction |
| `JCODE_DISABLE_TELEMETRY` | 1/true/yes/on | (unset) | Disable telemetry |
| `JCODE_DISABLE_MERMAID` | 1/true/yes/on | (unset) | Disable mermaid rendering |
| `JCODE_DISABLE_DESKTOP_ANIMATION` | 1/true/yes/on | (unset) | Disable desktop animations |
| `JCODE_DISABLE_POWER_INHIBIT` | 1/true/yes/on | (unset) | Disable power inhibit |
| `JCODE_DISABLE_HOOK` | comma-separated | (unset) | Selective hook skip |
| `JCODE_DISABLE_TOOL` | comma-separated | (unset) | Selective tool disable |
| `JCODE_DISABLE_ANIMATION` | comma-separated | (unset) | Selective animation disable |
| `JCODE_DISABLE_FEATURE` | comma-separated | (unset) | Selective feature/experiment disable |

### Integration into hooks v2 (future)

When hooks v2 is implemented, the hook dispatcher checks:

```rust
// In hooks/dispatch.rs or hooks/execute.rs:
fn execute_hooks(event: HookEvent, ctx: &HookContext) -> Result<HookOutcome> {
    // Disable check — fast O(1) bitmap check
    if DisableRegistry::global().hook_disabled(event.name()) {
        return Ok(HookOutcome::Skipped);
    }
    // ... dispatch logic ...
}
```

### Integration into plugin runtime (future)

```rust
// In plugins/runtime.rs or plugin loader:
fn load_plugins() -> Result<()> {
    if DisableRegistry::global().disabled(DisableFlag::Plugins) {
        tracing::info!("Plugins disabled via JCODE_DISABLE_PLUGINS");
        return Ok(());
    }
    // ... load plugins ...
}
```

### Integration into config fingerpinting

The `CONFIG_ENV_KEYS` array in `config.rs` currently tracks ~90 env vars for cache invalidation. After this change, we need:

1. Remove `JCODE_DISABLE_*` vars from `CONFIG_ENV_KEYS` (they're handled by `DisableRegistry`)
2. Keep config-only vars in `CONFIG_ENV_KEYS`

---

## 7. CLI Commands

### New: `jcode doctor --env`

Shows current env var state (useful for debugging):

```
$ jcode doctor --env

Disable Env Vars:
  JCODE_DISABLE_ALL=1           Master kill-switch
  JCODE_DISABLE_HOOKS=1         Hooks disabled
  JCODE_DISABLE_HOOK=pre_tool_use,stop
                                Hooks skipped: pre_tool_use, stop
  JCODE_DISABLE_TOOL=bash       Tools disabled: bash
  JCODE_DISABLE_TELEMETRY=1     Telemetry disabled

Deprecated (still supported):
  JCODE_DISABLE_BASE_TOOLS=1    → use JCODE_DISABLE_TOOL=base
  JCODE_NO_TELEMETRY=1          → use JCODE_DISABLE_TELEMETRY=1
```

---

## 8. Repo References

| Feature Aspect | Repo | File | Link |
|----------------|------|------|------|
| DISABLE_OMC master kill-switch | oh-my-claudecode | src/hooks/bridge.ts:3024-3027 | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/src/hooks/bridge.ts#L3024-L3027 |
| Cached skip hooks (OMC_SKIP_HOOKS) | oh-my-claudecode | src/hooks/bridge.ts:2996-3006 | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/src/hooks/bridge.ts#L2996-L3006 |
| OMC_TEAM_WORKER role-based bypass | oh-my-claudecode | src/hooks/persistent-mode/index.ts:1885-1891 | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/src/hooks/persistent-mode/index.ts#L1885-L1891 |
| isEnvTruthy() helper | claude-code | src/utils/envUtils.ts | — |
| OMC_DISABLE_TOOLS category gating | oh-my-claudecode | src/mcp/omc-tools-server.ts:62-82 | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/src/mcp/omc-tools-server.ts#L62-L82 |
| OMC_TEAM_SERVER_DISABLE_AUTOSTART | oh-my-claudecode | src/mcp/team-server.ts:650 | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/src/mcp/team-server.ts#L650 |
| Feature flag env gating | Claude Code | tools.ts:250 | `ENABLE_LSP_TOOL`, `CLAUDE_CODE_SIMPLE`, etc. |
| jcode current env_overrides.rs | jcode | crates/jcode-base/src/config/env_overrides.rs | local |
| jcode current config.rs CONFIG_ENV_KEYS | jcode | crates/jcode-base/src/config.rs:28 | local |

---

## 9. Test Cases

### Unit Tests (in `crates/jcode-base/src/disable.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_not_disabled() {
        // No env vars set → nothing disabled
        let registry = DisableRegistry::load_from_env();
        assert!(!registry.disabled(DisableFlag::Hooks));
        assert!(!registry.disabled(DisableFlag::All));
        assert!(!registry.hook_disabled("pre_tool_use"));
        assert!(!registry.tool_disabled("bash"));
    }

    #[test]
    fn test_master_kill_disables_all() {
        unsafe { std::env::set_var("JCODE_DISABLE_ALL", "1"); }
        let registry = DisableRegistry::load_from_env();
        assert!(registry.disabled(DisableFlag::All));
        assert!(registry.disabled(DisableFlag::Hooks));
        assert!(registry.disabled(DisableFlag::Plugins));
        assert!(registry.disabled(DisableFlag::Memory));
        unsafe { std::env::remove_var("JCODE_DISABLE_ALL"); }
    }

    #[test]
    fn test_hook_kill_disables_all_hooks() {
        unsafe { std::env::set_var("JCODE_DISABLE_HOOKS", "true"); }
        let registry = DisableRegistry::load_from_env();
        assert!(registry.hook_disabled("pre_tool_use"));
        assert!(registry.hook_disabled("stop"));
        unsafe { std::env::remove_var("JCODE_DISABLE_HOOKS"); }
    }

    #[test]
    fn test_selective_hook_skip() {
        unsafe { std::env::set_var("JCODE_DISABLE_HOOK", "pre_tool_use,stop"); }
        let registry = DisableRegistry::load_from_env();
        assert!(registry.hook_disabled("pre_tool_use"));
        assert!(registry.hook_disabled("stop"));
        assert!(!registry.hook_disabled("post_tool_use"));
        assert!(!registry.hook_disabled("session_start"));
        unsafe { std::env::remove_var("JCODE_DISABLE_HOOK"); }
    }

    #[test]
    fn test_selective_tool_disable() {
        unsafe { std::env::set_var("JCODE_DISABLE_TOOL", "bash,edit"); }
        let registry = DisableRegistry::load_from_env();
        assert!(registry.tool_disabled("bash"));
        assert!(registry.tool_disabled("edit"));
        assert!(!registry.tool_disabled("read"));
        unsafe { std::env::remove_var("JCODE_DISABLE_TOOL"); }
    }

    #[test]
    fn test_is_env_truthy() {
        unsafe {
            std::env::set_var("TEST_TRUTHY_1", "1");
            std::env::set_var("TEST_TRUTHY_TRUE", "true");
            std::env::set_var("TEST_TRUTHY_YES", "yes");
            std::env::set_var("TEST_TRUTHY_ON", "on");
            std::env::set_var("TEST_FALSY_0", "0");
            std::env::set_var("TEST_FALSY_FALSE", "false");
            std::env::set_var("TEST_FALSY_NO", "no");
            std::env::set_var("TEST_FALSY_OFF", "off");
        }
        assert!(is_env_truthy("TEST_TRUTHY_1"));
        assert!(is_env_truthy("TEST_TRUTHY_TRUE"));
        assert!(is_env_truthy("TEST_TRUTHY_YES"));
        assert!(is_env_truthy("TEST_TRUTHY_ON"));
        assert!(!is_env_truthy("TEST_FALSY_0"));
        assert!(!is_env_truthy("TEST_FALSY_FALSE"));
        assert!(!is_env_truthy("TEST_FALSY_NO"));
        assert!(!is_env_truthy("TEST_FALSY_OFF"));
        assert!(!is_env_truthy("NONEXISTENT_ENV_VAR"));
        unsafe {
            std::env::remove_var("TEST_TRUTHY_1");
            std::env::remove_var("TEST_TRUTHY_TRUE");
            std::env::remove_var("TEST_TRUTHY_YES");
            std::env::remove_var("TEST_TRUTHY_ON");
            std::env::remove_var("TEST_FALSY_0");
            std::env::remove_var("TEST_FALSY_FALSE");
            std::env::remove_var("TEST_FALSY_NO");
            std::env::remove_var("TEST_FALSY_OFF");
        }
    }

    #[test]
    fn test_empty_selective_lists() {
        unsafe {
            std::env::set_var("JCODE_DISABLE_HOOK", "");
            std::env::set_var("JCODE_DISABLE_TOOL", "");
        }
        let registry = DisableRegistry::load_from_env();
        assert!(!registry.hook_disabled("any"));
        assert!(!registry.tool_disabled("any"));
        unsafe {
            std::env::remove_var("JCODE_DISABLE_HOOK");
            std::env::remove_var("JCODE_DISABLE_TOOL");
        }
    }

    #[test]
    fn test_flag_env_var_names() {
        assert_eq!(DisableFlag::All.env_var(), "JCODE_DISABLE_ALL");
        assert_eq!(DisableFlag::Hooks.env_var(), "JCODE_DISABLE_HOOKS");
        assert_eq!(DisableFlag::Plugins.env_var(), "JCODE_DISABLE_PLUGINS");
        assert_eq!(DisableFlag::Memory.env_var(), "JCODE_DISABLE_MEMORY");
        assert_eq!(DisableFlag::PowerInhibit.env_var(), "JCODE_DISABLE_POWER_INHIBIT");
    }
}
```

### Integration Test: `crates/jcode-base/src/disable_tests.rs`

```rust
// Test: DisableRegistry + Config integration
#[test]
fn test_disable_registry_initialized_early() {
    // Config::load() should trigger DisableRegistry init
    unsafe { std::env::set_var("JCODE_DISABLE_ALL", "1"); }
    let _config = Config::load();
    assert!(disable::DisableRegistry::global().disabled(disable::DisableFlag::All));
    unsafe { std::env::remove_var("JCODE_DISABLE_ALL"); }
}
```

### Backward Compatibility Tests

```rust
// Test: old JCODE_DISABLE_BASE_TOOLS still works (deprecated)
#[test]
fn test_backward_compat_disable_base_tools() {
    unsafe { std::env::set_var("JCODE_DISABLE_BASE_TOOLS", "1"); }
    // Should still disable base tools via backward compat path
    // ...
    unsafe { std::env::remove_var("JCODE_DISABLE_BASE_TOOLS"); }
}
```

---

## 10. Success Criteria Checklist

- [ ] `DisableRegistry` exists in `crates/jcode-base/src/disable.rs`
- [ ] `DisableFlag` enum covers all planned subsystems (11 variants + All)
- [ ] `DisableRegistry::global()` returns cached singleton via `LazyLock`
- [ ] `JCODE_DISABLE_ALL=1` disables all subsystems
- [ ] `JCODE_DISABLE_HOOKS=1` disables all hooks
- [ ] `JCODE_DISABLE_HOOK=pre_tool_use,stop` selectively skips hooks
- [ ] `JCODE_DISABLE_TOOL=bash` disables specific tools
- [ ] All scattered env vars consolidated into `DisableRegistry`
- [ ] Old env vars still work with deprecation warnings
- [ ] No `std::env::var("JCODE_DISABLE_*")` calls remain outside `disable.rs`
- [ ] `env_overrides.rs` no longer handles disable/kill-switch logic
- [ ] `CONFIG_ENV_KEYS` no longer tracks disable env vars
- [ ] All unit tests pass (>10 tests)
- [ ] `cargo check` passes with no warnings
- [ ] `cargo test` in affected crates passes

---

## 11. Known Limitations & Future Work

- **Env vars static for process lifetime**: If users need to toggle at runtime without restart, a future version could add `JCODE.reload disable` command that re-reads env vars
- **`JCODE_ROLE=worker/leader`**: Role-based bypass is designed into the naming scheme but deferred — it needs the team/worker system to be built first. When that's ready, add `DisableFlag::Worker` and check it in the registry.
- **Config file equivalent**: A future `[disable]` section in `config.toml` could mirror the env vars for persistence. For now, env vars are the canonical source.
- **`fail_closed` mode**: pi-agent-rust has `fail_closed_hooks` config flag (if hook fails, abort instead of continue). This is related but not in scope — it's a behavior mode, not a disable flag. Plan for hooks v2.
