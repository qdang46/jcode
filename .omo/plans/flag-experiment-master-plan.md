# Implementation Plan: Experiment Flag System
> Generated from Codex deep-dive research + jcode codebase integration analysis
> Goal: A lifecycle-managed experiment flag system (inspired by Codex `codex_features`) for gradual, safe rollout of new features in jcode

---

## 1. Executive Summary

We will implement a centralized experiment flag system for jcode modeled after Codex's `codex_features` crate. The system introduces an `ExperimentFlag` enum with lifecycle stages (UnderDevelopment → Experimental → Stable → Deprecated → Removed), a TOML-based `[experiments]` config section for user toggles, CLI subcommands (`jcode experiment list/enable/disable`), runtime protocol API for TUI clients, and a checkbox list TUI popup. The system integrates into jcode's existing config layer (crates/jcode-config-types), CLI dispatch (src/cli/), protocol (crates/jcode-protocol), and TUI (crates/jcode-tui). This is a new `jcode-experiment-flags` crate plus modifications to 6 existing crates/modules.

---

## 2. Architecture Decision

### Chosen Approach

**Codex-style centralized enum + Stage lifecycle + TOML config**, adapted for jcode's existing `FeatureConfig`:

| Aspect | Decision | Why |
|--------|----------|-----|
| Core pattern | `ExperimentFlag` enum + `FEATURES` static `&[FeatureSpec]` | Codex proven pattern in Rust, compile-time enum safety |
| Stage lifecycle | UnderDevelopment → Experimental → Stable → Deprecated → Removed | Codex pattern, matches semantic versioning philosophy |
| Config storage | `[experiments]` TOML section alongside existing `[features]` | jcode already has `FeatureConfig` for mature toggles; experiments are separate |
| Config persistence | Written back to `config.toml` via existing config save path | Follows existing `jcode config` save mechanism |
| CLI | `jcode experiment list/enable/disable` | Mirror of `SkillsCommand` pattern (exists in jcode) |
| Protocol | `ExperimentFlagList` request/response + `ExperimentFlagEnablementSet` | NDJSON over Unix sockets, same pattern as existing wire types |
| TUI | Modal popup with checkbox list (spacebar toggle) | Follows `OverlayAction`+`session_picker` patterns already in jcode-tui |
| Runtime check | `experiments.check(ExperimentFlag::X)` returning `bool` | Similar to Codex `features.enabled(Feature::X)` |
| Dependencies | Auto-enable required flags (optional) | Codex `normalize_dependencies()` pattern |
| Enterprise constraints | Optional pinned flag overrides via TOML | Codex `FeatureRequirementsToml` / `pinned_features` pattern |

### Alternatives Considered

| Approach | Source | Pros | Cons | Decision |
|----------|--------|------|------|----------|
| Centralized `Feature` enum + `Stage` | Codex `codex_features` | Compile-time safety, discoverable, one registry | Requires new crate, enum changes need recompile | ✅ Chosen |
| Individual `feature('NAME')` strings | Claude Code Vite plugin | Zero boilerplate to add | No type safety, no lifecycle, runtime errors | ❌ Strings are fragile |
| GrowthBook remote flags | Claude Code | Remote toggle without deploy | Requires server, latency, single point of failure | ❌ Too heavy for CLI tool |
| Env vars only | oh-my-pi | Simplest implementation | No lifecycle, no discoverability, no TUI | ❌ Not sufficient |
| Extend existing `FeatureConfig` | jcode current | No new crate, minimal change | Current `FeatureConfig` is fixed struct fields, not dynamic | ❌ Hard to add new flags without recompile of base crate |

---

## 3. Data Structures & Types

### Core Types (new crate: `crates/jcode-experiment-flags`)

```rust
// ============================================================================
// File: crates/jcode-experiment-flags/src/lib.rs
// ============================================================================

/// Lifecycle stage of an experiment flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Internal-only, not stable enough for users. Emits warning when enabled.
    UnderDevelopment,
    /// Ready for early adopters. Visible in `jcode experiment list` TUI popup.
    /// Shows in `/experimental` command menu with name + description.
    Experimental {
        name: &'static str,
        menu_description: &'static str,
        /// A one-line announcement message shown when the flag becomes experimental.
        announcement: Option<&'static str>,
    },
    /// Stable and enabled by default. No longer shown in experiment list.
    Stable,
    /// Still works but scheduled for removal. Emits deprecation warning.
    Deprecated {
        /// Message explaining what to use instead.
        migration_hint: &'static str,
    },
    /// Removed. Flag still parsed for config backwards compat but always evaluates to false.
    Removed,
}

/// Unique identifier for each experiment flag (enum-based, like Codex).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumIter, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum ExperimentFlag {
    /// Dynamic Context Pruning (currently in FeatureConfig::dcp_enabled)
    DynamicContextPruning,
    /// Swarm coordination (currently in FeatureConfig::swarm)
    SwarmCoordination,
    /// V2 Hooks system (28 events, parallel dispatch)
    HooksV2,
    /// JavaScript plugin runtime (QuickJS embedded)
    JsPlugins,
    /// Persistent memory injection
    PersistMemoryInjection,
    /// Reasoning trace display in TUI
    ReasoningTrace,
    // ... more flags added over time
}

/// Static specification for an experiment flag — single source of truth.
#[derive(Debug, Clone, Copy)]
pub struct FeatureSpec {
    pub id: ExperimentFlag,
    /// TOML key name (e.g., "hooks_v2").
    pub key: &'static str,
    /// Current lifecycle stage.
    pub stage: Stage,
    /// Whether the flag defaults to enabled.
    pub default_enabled: bool,
    /// Feature IDs that must also be enabled for this flag to work.
    pub dependencies: &'static [ExperimentFlag],
}

/// All experiment flags defined in the system.
pub static EXPERIMENT_FLAGS: &[FeatureSpec] = &[
    FeatureSpec {
        id: ExperimentFlag::DynamicContextPruning,
        key: "dcp_enabled",
        stage: Stage::Stable,
        default_enabled: true,
        dependencies: &[],
    },
    FeatureSpec {
        id: ExperimentFlag::SwarmCoordination,
        key: "swarm",
        stage: Stage::Stable,
        default_enabled: true,
        dependencies: &[],
    },
    FeatureSpec {
        id: ExperimentFlag::HooksV2,
        key: "hooks_v2",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
        dependencies: &[],
    },
    FeatureSpec {
        id: ExperimentFlag::JsPlugins,
        key: "js_plugins",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
        dependencies: &[],
    },
    FeatureSpec {
        id: ExperimentFlag::PersistMemoryInjection,
        key: "persist_memory_injections",
        stage: Stage::Experimental {
            name: "Persist Memory Injections",
            menu_description: "Persist auto-recalled memory injections into normal session history",
            announcement: None,
        },
        default_enabled: false,
        dependencies: &[],
    },
    FeatureSpec {
        id: ExperimentFlag::ReasoningTrace,
        key: "reasoning_trace",
        stage: Stage::Experimental {
            name: "Reasoning Trace",
            menu_description: "Show model reasoning trace in TUI output",
            announcement: Some("Reasoning traces now available in TUI — /experimental to enable"),
        },
        default_enabled: false,
        dependencies: &[],
    },
];

/// Runtime representation of flag enablement state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiments {
    /// The set of enabled experiment flags.
    enabled: BTreeSet<ExperimentFlag>,
    /// Tracking for deprecated/renamed flag usages.
    #[serde(skip)]
    legacy_usages: Vec<LegacyUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyUsage {
    pub key: String,
    pub resolved_to: ExperimentFlag,
    pub count: u64,
}

impl Experiments {
    /// Create with default-enabled flags.
    pub fn with_defaults() -> Self {
        let mut enabled = BTreeSet::new();
        for spec in EXPERIMENT_FLAGS {
            if spec.default_enabled {
                enabled.insert(spec.id);
            }
        }
        Self {
            enabled,
            legacy_usages: Vec::new(),
        }
    }

    /// Check if a flag is enabled.
    pub fn check(&self, flag: ExperimentFlag) -> bool {
        self.enabled.contains(&flag)
    }

    /// Enable a flag.
    pub fn enable(&mut self, flag: ExperimentFlag) {
        self.enabled.insert(flag);
    }

    /// Disable a flag.
    pub fn disable(&mut self, flag: ExperimentFlag) {
        self.enabled.remove(&flag);
    }

    /// Set flag state from the provided map.
    pub fn apply_map(&mut self, map: &BTreeMap<String, bool>) {
        for (key, value) in map {
            if let Some(flag) = self.resolve_key(key) {
                if value {
                    self.enabled.insert(flag);
                } else {
                    self.enabled.remove(&flag);
                }
            }
        }
    }

    /// Resolve a string key to an ExperimentFlag (with legacy support).
    fn resolve_key(&self, key: &str) -> Option<ExperimentFlag> {
        // First try direct match
        for spec in EXPERIMENT_FLAGS {
            if spec.key == key {
                return Some(spec.id);
            }
        }
        // Try legacy/renamed keys
        match key {
            "memory" => Some(ExperimentFlag::DynamicContextPruning),
            "collab" => Some(ExperimentFlag::SwarmCoordination),
            _ => None,
        }
    }

    /// Normalize dependencies: auto-enable required flags.
    pub fn normalize_dependencies(&mut self) {
        let mut changed = true;
        while changed {
            changed = false;
            for spec in EXPERIMENT_FLAGS {
                if self.enabled.contains(&spec.id) {
                    for dep in spec.dependencies {
                        if self.enabled.insert(*dep) {
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    /// Get all flags that are currently enabled.
    pub fn enabled_flags(&self) -> Vec<ExperimentFlag> {
        self.enabled.iter().copied().collect()
    }

    /// Get the list of all flags with their current state, for TUI/CLI display.
    pub fn all_flag_states(&self) -> Vec<FlagState> {
        EXPERIMENT_FLAGS
            .iter()
            .map(|spec| FlagState {
                flag: spec.id,
                key: spec.key,
                stage: spec.stage,
                enabled: self.enabled.contains(&spec.id),
                default_enabled: spec.default_enabled,
            })
            .collect()
    }
}

/// Display state for one experiment flag.
#[derive(Debug, Clone, Serialize)]
pub struct FlagState {
    pub flag: ExperimentFlag,
    pub key: &'static str,
    pub stage: Stage,
    pub enabled: bool,
    pub default_enabled: bool,
}

// ============================================================================
// File: crates/jcode-experiment-flags/src/toml.rs
// ============================================================================

/// TOML deserialization of the [experiments] section.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExperimentsToml {
    /// Flattened key-value pairs, e.g. hooks_v2 = true
    #[serde(flatten)]
    pub entries: BTreeMap<String, bool>,
}

impl ExperimentsToml {
    /// Materialize into Experiments struct.
    pub fn materialize(&self) -> Experiments {
        let mut experiments = Experiments::with_defaults();
        experiments.apply_map(&self.entries);
        experiments.normalize_dependencies();
        experiments
    }
}

// ============================================================================
// File: crates/jcode-experiment-flags/src/tui.rs
// ============================================================================

/// Flag information for TUI display (one row in the experimental features popup).
#[derive(Debug, Clone)]
pub struct ExperimentFlagInfo {
    pub flag: ExperimentFlag,
    pub key: String,
    pub name: String,
    pub description: String,
    pub stage: Stage,
    pub enabled: bool,
}
```

### Config Integration

```rust
// File: crates/jcode-config-types/src/lib.rs (MODIFIED)

/// Runtime feature toggles (existing — stays for stable features)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeatureConfig {
    /// Enable memory retrieval/extraction features (default: true)
    pub memory: bool,
    /// Enable swarm coordination features (default: true)
    pub swarm: bool,
    /// Inject timestamps into user messages and tool results (default: true)
    pub message_timestamps: bool,
    /// Persist auto-recalled memory injections (default: false)
    pub persist_memory_injections: bool,
    // NOTE: dcp_enabled and update_channel move to experiments
}

/// NEW: Experiment flags section
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExperimentConfig {
    /// Enable hooks v2 system
    #[serde(default)]
    pub hooks_v2: Option<bool>,
    /// Enable JS plugin runtime
    #[serde(default)]
    pub js_plugins: Option<bool>,
    /// Enable reasoning trace display
    #[serde(default)]
    pub reasoning_trace: Option<bool>,
    /// Catch-all for unknown experiment flags (parsed from flatten)
    #[serde(flatten, skip_serializing)]
    pub extra: BTreeMap<String, bool>,
}
```

Wait — to match Codex's dynamic approach and avoid modifying this struct every time we add a flag, we should use the `BTreeMap<String, bool>` approach directly, not individual `Option<bool>` fields. This matches Codex's `FeaturesToml` with `#[serde(flatten)] entries: BTreeMap<String,bool>`.

```rust
// File: crates/jcode-config-types/src/lib.rs (REVISED)

/// Experiment flags TOML section — dynamically keyed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExperimentConfig {
    /// Dynamic experiment flag entries, e.g.:
    /// [experiments]
    /// hooks_v2 = true
    /// js_plugins = false
    #[serde(flatten)]
    pub entries: BTreeMap<String, bool>,
}
```

---

## 4. Pseudocode — Core Algorithm

```
// Initialization
FUNCTION init_experiments(config_toml):
  // 1. Load [experiments] section from config.toml (dynamically keyed)
  // 2. Materialize into Experiments struct with defaults from EXPERIMENT_FLAGS static
  // 3. Apply user overrides from config
  // 4. Normalize dependencies (auto-enable required flags)
  // 5. Validate: warn if UnderDevelopment flags are enabled
  // 6. Return Experiments instance as singleton

// Runtime checks
FUNCTION check_flag(experiments, flag):
  // 1. Look up flag in enabled set
  // 2. Check pinned constraints (enterprise overrides)
  // 3. Return boolean

// CLI: jcode experiment list
FUNCTION cmd_experiment_list(experiments, json):
  flags = experiments.all_flag_states()
  if json:
    print JSON of flags
  else:
    print table: Key | Stage | Default | Current
    highlight UnderDevelopment with WARN label

// CLI: jcode experiment enable <flag>
FUNCTION cmd_experiment_enable(experiments, key):
  flag = resolve_key(key)
  if flag.stage == Removed:
    print error "Flag removed: use X instead"
    return
  experiments.enable(flag)
  experiments.normalize_dependencies()
  save config to disk
  trigger on_config_reloaded() callbacks

// CLI: jcode experiment disable <flag>
FUNCTION cmd_experiment_disable(experiments, key):
  flag = resolve_key(key)
  experiments.disable(flag)
  save config to disk
  trigger on_config_reloaded() callbacks

// TUI popup
FUNCTION show_experiment_popup(app_state):
  // 1. Open modal overlay in center of screen
  // 2. Fetch current flag states from server via protocol
  // 3. Render checkbox list:
  //    [x] hooks_v2        — V2 Hooks (HooksV2)           [Experimental]
  //    [ ] js_plugins      — JS Plugin Runtime            [UnderDevelopment]
  //    [x] reasoning_trace — Reasoning Trace               [Experimental]
  // 4. Handle input:
  //    Space: toggle selected flag
  //    j/k:  navigate
  //    Enter: apply and close
  //    Esc:  close without changes
  // 5. On apply: send ExperimentFlagEnablementSet to server
  // 6. Server saves config, notifies clients
```

---

## 5. Implementation Code

### Cargo workspace changes

```toml
# File: Cargo.toml (workspace root — ADD member)
members = [
    ...
    "crates/jcode-experiment-flags",
    ...
]
```

### Module tree for `crates/jcode-experiment-flags`

```
crates/jcode-experiment-flags/
├── Cargo.toml
└── src/
    ├── lib.rs           # ExperimentFlag enum, FeatureSpec, FEATURES static, Experiments struct
    ├── toml.rs          # ExperimentsToml serde deserialization
    └── legacy.rs        # Legacy key resolution (renamed/removed flags)
```

### Cargo.toml

```toml
# File: crates/jcode-experiment-flags/Cargo.toml
[package]
name = "jcode-experiment-flags"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
strum = { workspace = true, features = ["derive"] }
```

### Full Implementation

```rust
// File: crates/jcode-experiment-flags/src/lib.rs

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// --- Core Types (shown above in Section 3) ---
// ExperimentFlag enum, Stage enum, FeatureSpec struct, EXPERIMENT_FLAGS static,
// Experiments struct with all methods, FlagState struct

// --- Key Methods Implementation ---

impl Experiments {
    /// Apply user overrides and validate, emitting warnings for unstable flags.
    pub fn from_config(toml_entries: &BTreeMap<String, bool>) -> Self {
        let mut ex = Experiments::with_defaults();
        ex.apply_map(toml_entries);
        ex.normalize_dependencies();
        ex.warn_unstable();
        ex
    }

    fn warn_unstable(&self) {
        for spec in EXPERIMENT_FLAGS {
            if matches!(spec.stage, Stage::UnderDevelopment) && self.enabled.contains(&spec.id) {
                eprintln!(
                    "[jcode] WARNING: UnderDevelopment flag '{}' is enabled. \
                     This feature is not ready for production use.",
                    spec.key
                );
            }
        }
    }
}
```

### jcode Config Integration

```rust
// File: crates/jcode-config-types/src/lib.rs (MODIFIED)

/// Experiment flags section in config.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExperimentConfig {
    /// Dynamic experiment flag entries
    #[serde(flatten)]
    pub entries: BTreeMap<String, bool>,
}
```

```rust
// File: crates/jcode-base/src/config.rs (MODIFIED — add section to Config struct)

pub struct Config {
    // ... existing fields ...
    pub features: FeatureConfig,
    /// Experiment flags section
    #[serde(default)]
    pub experiments: ExperimentConfig,
    // ... remaining fields ...
}
```

### CLI Integration

```rust
// File: src/cli/args.rs (MODIFIED — add FeaturesCommand variant)

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    // ... existing variants ...

    /// Manage experiment flags (list, enable, disable)
    #[command(subcommand)]
    Experiment(ExperimentCommand),
}

#[derive(Subcommand, Debug)]
pub(crate) enum ExperimentCommand {
    /// List all experiment flags and their current state
    List {
        /// Emit JSON instead of human-readable output
        #[arg(long)]
        json: bool,
    },

    /// Enable an experiment flag by key name
    Enable {
        /// Experiment flag key (e.g., "hooks_v2", "js_plugins")
        key: String,
    },

    /// Disable an experiment flag by key name
    Disable {
        /// Experiment flag key (e.g., "hooks_v2", "js_plugins")
        key: String,
    },
}
```

```rust
// File: src/cli/dispatch.rs (MODIFIED — add dispatch arm)

use crate::experiment_flags; // new module

Some(Command::Experiment(subcmd)) => match subcmd {
    ExperimentCommand::List { json } => {
        experiment_flags::run_experiment_list_command(json)?;
    }
    ExperimentCommand::Enable { key } => {
        experiment_flags::run_experiment_enable_command(&key)?;
    }
    ExperimentCommand::Disable { key } => {
        experiment_flags::run_experiment_disable_command(&key)?;
    }
},
```

```rust
// File: src/cli/experiment_flags.rs (NEW)

use anyhow::{Context, Result};
use jcode_experiment_flags::{ExperimentFlag, Experiments, EXPERIMENT_FLAGS};
use std::collections::BTreeMap;

/// Flag display states (UnderDevelopment, Experimental, Stable, Deprecated, Removed)
const STAGE_LABEL: &[&str] = &[
    "UnderDevelopment",
    "Experimental",
    "Stable",
    "Deprecated",
    "Removed",
];

pub fn run_experiment_list_command(json: bool) -> Result<()> {
    let config = crate::config::config();
    let experiments = Experiments::from_config(&config.experiments.entries);

    if json {
        let states = experiments.all_flag_states();
        println!("{}", serde_json::to_string_pretty(&states)?);
    } else {
        println!("{:25} {:20} {:8} {:8}  {}", "Key", "Flag", "Default", "Current", "Stage");
        println!("{}", "-".repeat(90));
        for spec in EXPERIMENT_FLAGS {
            let enabled = experiments.check(spec.id);
            let default_str = if spec.default_enabled { "on" } else { "off" };
            let current_str = if enabled { "ON" } else { "OFF" };
            let stage_label = match spec.stage {
                Stage::UnderDevelopment => format!("{:20}", "UnderDevelopment"),
                Stage::Experimental { name, .. } => format!("{:20}", "Experimental"),
                Stage::Stable => format!("{:20}", "Stable"),
                Stage::Deprecated { .. } => format!("{:20}", "Deprecated"),
                Stage::Removed => format!("{:20}", "Removed"),
            };
            println!(
                "{:25} {:20} {:8} {:8}  {}",
                spec.key,
                format!("{:?}", spec.id),
                default_str,
                current_str,
                stage_label
            );
        }
    }
    Ok(())
}

pub fn run_experiment_enable_command(key: &str) -> Result<()> {
    let mut config = crate::config::config();
    config.experiments.entries.insert(key.to_string(), true);
    crate::config::save_config(&config)?;
    crate::config::invalidate_config_cache();
    eprintln!("[jcode] Experiment '{key}' enabled.");
    Ok(())
}

pub fn run_experiment_disable_command(key: &str) -> Result<()> {
    let mut config = crate::config::config();
    config.experiments.entries.insert(key.to_string(), false);
    crate::config::save_config(&config)?;
    crate::config::invalidate_config_cache();
    eprintln!("[jcode] Experiment '{key}' disabled.");
    Ok(())
}
```

### Protocol Integration

```rust
// File: crates/jcode-protocol/src/wire.rs (MODIFIED)

// Add to Request enum:
#[serde(rename = "experiment_list")]
ExperimentList { id: u64 },

#[serde(rename = "experiment_set")]
ExperimentSet { id: u64, key: String, enabled: bool },

// Add to ServerEvent enum:
#[serde(rename = "experiment_flags")]
ExperimentFlags {
    /// JSON array of FlagState objects
    flags: Vec<serde_json::Value>,
},
```

### TUI Integration

```rust
// File: crates/jcode-tui/src/tui/experiment_flags.rs (NEW)

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub struct ExperimentFlagsPopup {
    flags: Vec<FlagState>,
    selected: usize,
    dirty: bool,
}

impl ExperimentFlagsPopup {
    pub fn new(flags: Vec<FlagState>) -> Self {
        Self { flags, selected: 0, dirty: false }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        // Render checkbox list overlay
        // [x] hooks_v2 — V2 Hook System [Experimental]
        // [ ] js_plugins — JS Plugin Runtime [UnderDevelopment]
        // ...
    }

    pub fn handle_key(&mut self, key: KeyCode) -> OverlayAction {
        match key {
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.flags.len() - 1);
                OverlayAction::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                OverlayAction::Continue
            }
            KeyCode::Char(' ') => {
                if let Some(flag) = self.flags.get_mut(self.selected) {
                    flag.enabled = !flag.enabled;
                    self.dirty = true;
                }
                OverlayAction::Continue
            }
            KeyCode::Esc => {
                if self.dirty {
                    // TODO: send ExperimentSet to server
                }
                OverlayAction::Close
            }
            _ => OverlayAction::Continue,
        }
    }
}
```

### Runtime Check Points

Key places in jcode where `experiments.check(Flag::X)` gates are needed:

```rust
// File: crates/jcode-app-core/src/dcg_bridge.rs (example)
use jcode_experiment_flags::{ExperimentFlag, Experiments};

// Before triggering DCP:
if experiments.check(ExperimentFlag::DynamicContextPruning) {
    // run DCP logic
}

// File: crates/jcode-tui/src/tui/app/turn.rs (example)
// Before showing reasoning trace:
if experiments.check(ExperimentFlag::ReasoningTrace) {
    // render reasoning content
}
```

---

## 6. Configuration & Wiring

### Config.toml format

```toml
# ~/.jcode/config.toml

[features]
# Stable feature toggles (existing)
memory = true
swarm = true
message_timestamps = true
persist_memory_injections = false

[experiments]
# Experiment flags — lifecycle managed, dynamic keys
# UnderDevelopment: warn when enabled, not in TUI
# Experimental: show in "jcode experiment list" and TUI popup
# Stable: enabled by default, not shown in experiment views
# Deprecated: warn on use, scheduled for removal
# Removed: parsed for backwards compat, always evaluates to false

# hooks_v2 = true
# js_plugins = false
# reasoning_trace = true
```

### Env var overrides

```bash
# JCODE_EXPERIMENTS can override any experiment flag at runtime
# Format: comma-separated key=value pairs
# Priority: env var > config file > defaults
JCODE_EXPERIMENTS="hooks_v2=true,js_plugins=true" jcode serve

# Individual flag env vars (higher priority)
JCODE_HOOKS_V2=true JCODE_JS_PLUGINS=true jcode serve
```

### Init flow

```
jcode startup
  → config::load() reads config.toml
    → experiments: ExperimentConfig (BTreeMap<String,bool>)
  → Experiments::from_config(&config.experiments.entries)
    → load defaults from EXPERIMENT_FLAGS static
    → apply user overrides
    → normalize dependencies
    → warn on UnderDevelopment flags
  → store as singleton (or alongside existing config singleton)
  → subsystems call experiments.check(Flag::X) at runtime
```

---

## 7. Repo References

| Feature Aspect | Repo | File | Link |
|----------------|------|------|------|
| Feature enum (~80 variants) | codex | codex_features/src/feature.rs | https://github.com/openai/codex/blob/main/codex_features/src/feature.rs |
| Stage enum lifecycle | codex | codex_features/src/feature.rs | https://github.com/openai/codex/blob/main/codex_features/src/feature.rs#L45 |
| FEATURES static array | codex | codex_features/src/feature.rs | https://github.com/openai/codex/blob/main/codex_features/src/feature.rs#L93 |
| Features struct with methods | codex | codex_features/src/features.rs | https://github.com/openai/codex/blob/main/codex_features/src/features.rs |
| FeaturesToml flatten pattern | codex | codex_features/src/features_toml.rs | https://github.com/openai/codex/blob/main/codex_features/src/features_toml.rs |
| ManagedFeatures + pinned constraints | codex | codex/src/config/managed_features.rs | https://github.com/openai/codex/blob/main/codex/src/config/managed_features.rs |
| normalize_dependencies | codex | codex_features/src/features.rs | https://github.com/openai/codex/blob/main/codex_features/src/features.rs |
| CLI subcommands (list/enable/disable) | codex | codex/src/bin/cli/main.rs | https://github.com/openai/codex/blob/main/codex/src/bin/cli/main.rs |
| ExperimentalFeaturesView TUI popup | codex | codex_tui/src/view/experimental.rs | https://github.com/openai/codex/blob/main/codex_tui/src/view/experimental.rs |
| Protocol API request/response | codex | codex_core/src/protocol/types.rs | https://github.com/openai/codex/blob/main/codex_core/src/protocol/types.rs |
| Legacy key resolution | codex | codex_features/src/legacy.rs | https://github.com/openai/codex/blob/main/codex_features/src/legacy.rs |
| FeatureConfig in jcode (existing) | jcode | crates/jcode-config-types/src/lib.rs | Line 633 |
| Config struct in jcode | jcode | crates/jcode-base/src/config.rs | Line 377 |
| CLI dispatch in jcode | jcode | src/cli/dispatch.rs | Line 58 |
| Command enum in jcode | jcode | src/cli/args.rs | Line 272 |
| SkillsCommand pattern | jcode | src/cli/args.rs | Line 1058 |
| Protocol Request enum | jcode | crates/jcode-protocol/src/wire.rs | Line 13 |
| Protocol ServerEvent enum | jcode | crates/jcode-protocol/src/wire.rs | Line 585 |
| TUI overlays (changelog) | jcode | crates/jcode-tui/src/tui/ui_overlays.rs | Line 13 |
| TUI session picker (checkbox list) | jcode | crates/jcode-tui/src/tui/session_picker.rs | Line 59 |
| TUI OverlayAction pattern | jcode | crates/jcode-tui/src/tui/session_picker.rs | Line 59 |
| Claude Code build-time flags | claude-code | packages/claude-code/src/feature/ | https://github.com/claude-code-best/claude-code |

---

## 8. Test Cases

### Unit Tests (for `jcode-experiment-flags` crate)

```rust
// File: crates/jcode-experiment-flags/src/tests.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_stable_enabled() {
        let ex = Experiments::with_defaults();
        // Stable flags are enabled by default
        assert!(ex.check(ExperimentFlag::DynamicContextPruning));
        assert!(ex.check(ExperimentFlag::SwarmCoordination));
        // UnderDevelopment flags are disabled by default
        assert!(!ex.check(ExperimentFlag::HooksV2));
        assert!(!ex.check(ExperimentFlag::JsPlugins));
    }

    #[test]
    fn test_enable_flag() {
        let mut ex = Experiments::with_defaults();
        ex.enable(ExperimentFlag::HooksV2);
        assert!(ex.check(ExperimentFlag::HooksV2));
    }

    #[test]
    fn test_disable_flag() {
        let mut ex = Experiments::with_defaults();
        ex.disable(ExperimentFlag::DynamicContextPruning);
        assert!(!ex.check(ExperimentFlag::DynamicContextPruning));
    }

    #[test]
    fn test_apply_map() {
        let mut ex = Experiments::with_defaults();
        let mut map = BTreeMap::new();
        map.insert("hooks_v2".to_string(), true);
        map.insert("dcp_enabled".to_string(), false);
        ex.apply_map(&map);
        assert!(ex.check(ExperimentFlag::HooksV2));
        assert!(!ex.check(ExperimentFlag::DynamicContextPruning));
    }

    #[test]
    fn test_dependency_normalization() {
        // If we add dependency relationships, test they get auto-enabled
        let mut ex = Experiments::with_defaults();
        ex.normalize_dependencies();
        // No-op if no deps enabled — just verify it runs
    }

    #[test]
    fn test_legacy_key_resolution() {
        let mut ex = Experiments::with_defaults();
        let mut map = BTreeMap::new();
        map.insert("memory".to_string(), false);
        ex.apply_map(&map);
        assert!(!ex.check(ExperimentFlag::DynamicContextPruning));
    }

    #[test]
    fn test_removed_flag_always_false() {
        // Removed flags should always evaluate to false
        // (test with a hypothetical removed flag scenario)
        let ex = Experiments::with_defaults();
        // ... assert removed flags are handled gracefully
    }

    #[test]
    fn test_all_flag_states_length() {
        let ex = Experiments::with_defaults();
        let states = ex.all_flag_states();
        assert_eq!(states.len(), EXPERIMENT_FLAGS.len());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut ex = Experiments::with_defaults();
        ex.enable(ExperimentFlag::HooksV2);
        let json = serde_json::to_string(&ex).unwrap();
        let deserialized: Experiments = serde_json::from_str(&json).unwrap();
        assert!(deserialized.check(ExperimentFlag::HooksV2));
    }

    #[test]
    fn test_toml_deserialization() {
        let toml_str = r#"
            [experiments]
            hooks_v2 = true
            js_plugins = false
        "#;
        let config: ExperimentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entries.get("hooks_v2"), Some(&true));
        assert_eq!(config.entries.get("js_plugins"), Some(&false));
    }
}
```

### Integration Tests

```rust
// File: src/cli/tests/experiment_tests.rs

#[cfg(test)]
mod tests {
    use assert_cmd::Command;

    #[test]
    fn test_experiment_list_command() {
        // jcode experiment list
        let mut cmd = Command::cargo_bin("jcode").unwrap();
        cmd.arg("experiment").arg("list");
        cmd.assert().success();
    }

    #[test]
    fn test_experiment_enable() {
        // jcode experiment enable hooks_v2
        let mut cmd = Command::cargo_bin("jcode").unwrap();
        cmd.arg("experiment").arg("enable").arg("hooks_v2");
        cmd.assert().success();
    }

    #[test]
    fn test_experiment_disable() {
        // jcode experiment disable hooks_v2
        let mut cmd = Command::cargo_bin("jcode").unwrap();
        cmd.arg("experiment").arg("disable").arg("hooks_v2");
        cmd.assert().success();
    }

    #[test]
    fn test_experiment_list_json() {
        let mut cmd = Command::cargo_bin("jcode").unwrap();
        cmd.arg("experiment").arg("list").arg("--json");
        cmd.assert().success().stdout(predicates::str::contains("flag"));
    }

    #[test]
    fn test_experiment_unknown_flag() {
        // Should handle gracefully
        let mut cmd = Command::cargo_bin("jcode").unwrap();
        cmd.arg("experiment").arg("enable").arg("nonexistent_flag");
        // Might still succeed (just inserts into BTreeMap) — depends on validation
    }
}
```

### E2E Test

```rust
#[tokio::test]
async fn test_experiment_flag_lifecycle() {
    // 1. Start server with experiments config
    // 2. Verify defaults
    // 3. Send ExperimentSet via protocol
    // 4. Verify flag is now enabled
    // 5. Check that gated code path activates
    // 6. Disable flag
    // 7. Verify gated code path deactivates
}
```

---

## 9. Benchmarks

### What to Measure

| Metric | Baseline | Target | How to Measure |
|--------|----------|--------|----------------|
| `Experiments::check()` latency | N/A (new) | <100ns | `criterion` benchmark on `experiments.check(Flag::X)` |
| `Experiments::from_config()` latency | N/A (new) | <50µs | `criterion` on loading 20-50 flags |
| Memory delta per instance | N/A (new) | <2KB | `dhat` heap profiler on `Experiments` struct |
| Config load time increase | ~200µs | +50µs max | Instrument `config::load()` with tracing |

### Benchmark Code

```rust
#[cfg(test)]
mod benchmarks {
    use super::*;
    use criterion::{black_box, criterion_group, criterion_main, Criterion};

    fn bench_check_flag(c: &mut Criterion) {
        let ex = Experiments::with_defaults();
        c.bench_function("check_flag", |b| {
            b.iter(|| {
                black_box(ex.check(ExperimentFlag::DynamicContextPruning));
            })
        });
    }

    fn bench_from_config(c: &mut Criterion) {
        let mut map = BTreeMap::new();
        map.insert("hooks_v2".to_string(), true);
        map.insert("js_plugins".to_string(), false);
        c.bench_function("from_config_20_flags", |b| {
            b.iter(|| {
                black_box(Experiments::from_config(&map));
            })
        });
    }

    criterion_group!(benches, bench_check_flag, bench_from_config);
    criterion_main!(benches);
}
```

---

## 10. Migration / Rollout

### Phase 1: Foundation (Day 1-2)
- Create `jcode-experiment-flags` crate with `ExperimentFlag` enum, `EXPERIMENT_FLAGS` static, `Experiments` struct
- Add `[experiments]` section to config (behind `ExperimentConfig`)
- Wire into config load + store lifecycle
- **No user-visible changes yet**

### Phase 2: CLI + Protocol (Day 3-4)
- Add `jcode experiment list/enable/disable` CLI subcommands
- Add `ExperimentList` / `ExperimentSet` protocol requests
- Add `ExperimentFlags` server event
- **Users can now `jcode experiment list`**

### Phase 3: TUI (Day 5-6)
- Build `ExperimentFlagsPopup` overlay with checkbox list
- Wire into app event handling (keyboard shortcuts)
- On apply: send protocol request to persist
- **Users can toggle flags from `/experimental` TUI menu**

### Phase 4: Gate Integration (Ongoing)
- Replace `FeatureConfig::dcp_enabled` → `ExperimentFlag::DynamicContextPruning`
- Add `ExperimentFlag::HooksV2` checks in agent turn loop
- Add `ExperimentFlag::JsPlugins` checks in startup
- Add `ExperimentFlag::ReasoningTrace` in TUI rendering
- Migrate existing `FeatureConfig` flags to experiments over time

### Deprecation path
- Old `FeatureConfig` fields stay for one release with deprecation warnings
- Config migration: `features.dcp_enabled` → `experiments.dcp_enabled`
- Legacy key resolution handles the transition transparently

---

## 11. Known Limitations & Future Work

- [ ] Enum variants require recompilation to add new flags (intentional — this is a feature, not a bug, for type safety)
- [ ] No remote/telemetry-based flag toggling (GrowthBook equivalent) — could be added on top later
- [ ] No per-session flag overrides yet (all flags are process-wide)
- [ ] No profile-based flag presets (e.g., "stable profile" vs "nightly profile")
- [ ] Enterprise `FeatureRequirementsToml` pinned constraints not implemented yet (Codex pattern for later)
- [ ] Flag dependency resolution is O(n*m) — negligible for current scale but can optimize with a DAG
- [ ] TUI popup doesn't show flag descriptions inline yet (mouse hover or expand)

---

## 12. Success Criteria Checklist

- [ ] `Experiments::check()` returns correct value for enabled/disabled flags
- [ ] `EXPERIMENT_FLAGS` static array is the single source of truth for all flags
- [ ] `jcode experiment list` shows all flags with stage, default, and current state
- [ ] `jcode experiment enable <key>` toggles flag on, persists to config.toml
- [ ] `jcode experiment disable <key>` toggles flag off, persists to config.toml
- [ ] `--json` flag works for script-friendly output
- [ ] UnderDevelopment flags show startup warning when enabled
- [ ] Deprecated flags show deprecation hint when enabled
- [ ] Removed flags always evaluate to false
- [ ] Legacy key resolution works for renamed flags
- [ ] Config round-trips: saving and reloading preserves all experiment states
- [ ] Protocol request/response works over Unix socket
- [ ] TUI popup renders checkbox list with spacebar toggle
- [ ] TUI popup changes persist to server config
- [ ] No regressions in existing `FeatureConfig` behavior
- [ ] Cargo check passes with no new warnings
- [ ] All unit tests pass
- [ ] CLI integration tests pass
