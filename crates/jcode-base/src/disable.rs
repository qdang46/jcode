//! Centralized disable registry for all `JCODE_DISABLE_*` env vars.
//!
//! A `LazyLock<DisableRegistry>` singleton is loaded once at first access,
//! caching boolean flags and comma-separated skip lists. Env vars do not
//! change during process lifetime, so a one-shot load is safe and efficient.

use std::collections::HashSet;
use std::sync::LazyLock;

// ─── DisableFlag Enum ───────────────────────────────────────────────

/// Each variant corresponds to a `JCODE_DISABLE_<NAME>=1` env var.
/// Parsed once at startup, cached forever.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DisableFlag {
    /// Master kill-switch: `JCODE_DISABLE_ALL=1` disables everything.
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
    /// All subsystem flags (excludes `All` — `All` is meta).
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

    /// Map flag → env var name.
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
/// Loaded ONCE at first access via `LazyLock`.
/// Env vars do not change during process lifetime.
pub struct DisableRegistry {
    /// Boolean disable flags.
    flags: HashSet<DisableFlag>,
    /// Selective skip lists (parsed from comma-separated env vars).
    disabled_hooks: HashSet<String>,
    disabled_tools: HashSet<String>,
    disabled_animations: HashSet<String>,
    disabled_features: HashSet<String>,
    /// Cached value of deprecated `JCODE_DISABLE_BASE_TOOLS` env var.
    base_tools_legacy_disabled: bool,
}

impl DisableRegistry {
    /// Global singleton, lazily initialized.
    pub fn global() -> &'static Self {
        static INSTANCE: LazyLock<DisableRegistry> = LazyLock::new(DisableRegistry::load_from_env);
        &INSTANCE
    }

    /// Load and cache ALL `JCODE_DISABLE_*` state at once.
    fn load_from_env() -> Self {
        let mut flags: HashSet<DisableFlag> = HashSet::new();

        // Scan all known DisableFlag env vars.
        let all_flag = DisableFlag::All;
        for flag in std::iter::once(&all_flag).chain(DisableFlag::all_subsystems()) {
            if is_env_truthy(flag.env_var()) {
                flags.insert(*flag);
            }
        }

        // Master kill overrides all subsystems.
        if flags.contains(&DisableFlag::All) {
            flags.extend(DisableFlag::all_subsystems());
        }

        // Selective skip lists.
        let disabled_hooks = parse_comma_list("JCODE_DISABLE_HOOK");
        let disabled_tools = parse_comma_list("JCODE_DISABLE_TOOL");
        let disabled_animations = parse_comma_list("JCODE_DISABLE_ANIMATION");
        let disabled_features = parse_comma_list("JCODE_DISABLE_FEATURE");

        let base_tools_legacy_disabled = std::env::var("JCODE_DISABLE_BASE_TOOLS")
            .ok()
            .map(|v| is_env_truthy_raw(&v))
            .unwrap_or(false);

        Self {
            flags,
            disabled_hooks,
            disabled_tools,
            disabled_animations,
            disabled_features,
            base_tools_legacy_disabled,
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

    /// Get all disabled hook names (for error/doctor output).
    pub fn all_disabled_hooks(&self) -> Vec<&str> {
        let mut hooks: Vec<&str> = self.disabled_hooks.iter().map(String::as_str).collect();
        hooks.sort();
        hooks
    }

    /// Get all disabled tool names (for config population / doctor output).
    pub fn all_disabled_tools(&self) -> &HashSet<String> {
        &self.disabled_tools
    }

    /// Get all disabled animation names (for config population / doctor output).
    pub fn all_disabled_animations(&self) -> &HashSet<String> {
        &self.disabled_animations
    }

    /// Is the `JCODE_DISABLE_BASE_TOOLS` legacy flag active?
    /// This checks both the new `JCODE_DISABLE_TOOL=base` path and the
    /// deprecated `JCODE_DISABLE_BASE_TOOLS=1` env var.
    pub fn base_tools_disabled(&self) -> bool {
        self.disabled_tools.contains("base") || self.base_tools_legacy_disabled
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Parse a boolean env var: `"1"`, `"true"`, `"yes"`, `"on"` → `true`.
pub fn is_env_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| is_env_truthy_raw(&v))
        .unwrap_or(false)
}

/// Parse a boolean string value: `"1"`, `"true"`, `"yes"`, `"on"` → `true`.
pub fn is_env_truthy_raw(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Parse a comma-separated list env var into a `HashSet`.
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

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_not_disabled() {
        // No env vars set → nothing disabled.
        // We call load_from_env directly to avoid polluting the global singleton.
        let registry = DisableRegistry::load_from_env();
        assert!(!registry.disabled(DisableFlag::Hooks));
        assert!(!registry.disabled(DisableFlag::All));
        assert!(!registry.hook_disabled("pre_tool_use"));
        assert!(!registry.tool_disabled("bash"));
    }

    #[test]
    fn test_master_kill_disables_all() {
        crate::env::set_var("JCODE_DISABLE_ALL", "1");
        let registry = DisableRegistry::load_from_env();
        assert!(registry.disabled(DisableFlag::All));
        assert!(registry.disabled(DisableFlag::Hooks));
        assert!(registry.disabled(DisableFlag::Plugins));
        assert!(registry.disabled(DisableFlag::Memory));
        crate::env::remove_var("JCODE_DISABLE_ALL");
    }

    #[test]
    fn test_hook_kill_disables_all_hooks() {
        crate::env::set_var("JCODE_DISABLE_HOOKS", "true");
        let registry = DisableRegistry::load_from_env();
        assert!(registry.hook_disabled("pre_tool_use"));
        assert!(registry.hook_disabled("stop"));
        crate::env::remove_var("JCODE_DISABLE_HOOKS");
    }

    #[test]
    fn test_selective_hook_skip() {
        crate::env::set_var("JCODE_DISABLE_HOOK", "pre_tool_use,stop");
        let registry = DisableRegistry::load_from_env();
        assert!(registry.hook_disabled("pre_tool_use"));
        assert!(registry.hook_disabled("stop"));
        assert!(!registry.hook_disabled("post_tool_use"));
        assert!(!registry.hook_disabled("session_start"));
        crate::env::remove_var("JCODE_DISABLE_HOOK");
    }

    #[test]
    fn test_selective_tool_disable() {
        crate::env::set_var("JCODE_DISABLE_TOOL", "bash,edit");
        let registry = DisableRegistry::load_from_env();
        assert!(registry.tool_disabled("bash"));
        assert!(registry.tool_disabled("edit"));
        assert!(!registry.tool_disabled("read"));
        crate::env::remove_var("JCODE_DISABLE_TOOL");
    }

    #[test]
    fn test_is_env_truthy() {
        crate::env::set_var("TEST_TRUTHY_1", "1");
        crate::env::set_var("TEST_TRUTHY_TRUE", "true");
        crate::env::set_var("TEST_TRUTHY_YES", "yes");
        crate::env::set_var("TEST_TRUTHY_ON", "on");
        crate::env::set_var("TEST_FALSY_0", "0");
        crate::env::set_var("TEST_FALSY_FALSE", "false");
        crate::env::set_var("TEST_FALSY_NO", "no");
        crate::env::set_var("TEST_FALSY_OFF", "off");

        assert!(is_env_truthy("TEST_TRUTHY_1"));
        assert!(is_env_truthy("TEST_TRUTHY_TRUE"));
        assert!(is_env_truthy("TEST_TRUTHY_YES"));
        assert!(is_env_truthy("TEST_TRUTHY_ON"));
        assert!(!is_env_truthy("TEST_FALSY_0"));
        assert!(!is_env_truthy("TEST_FALSY_FALSE"));
        assert!(!is_env_truthy("TEST_FALSY_NO"));
        assert!(!is_env_truthy("TEST_FALSY_OFF"));
        assert!(!is_env_truthy("NONEXISTENT_ENV_VAR"));

        crate::env::remove_var("TEST_TRUTHY_1");
        crate::env::remove_var("TEST_TRUTHY_TRUE");
        crate::env::remove_var("TEST_TRUTHY_YES");
        crate::env::remove_var("TEST_TRUTHY_ON");
        crate::env::remove_var("TEST_FALSY_0");
        crate::env::remove_var("TEST_FALSY_FALSE");
        crate::env::remove_var("TEST_FALSY_NO");
        crate::env::remove_var("TEST_FALSY_OFF");
    }

    #[test]
    fn test_empty_selective_lists() {
        crate::env::set_var("JCODE_DISABLE_HOOK", "");
        crate::env::set_var("JCODE_DISABLE_TOOL", "");
        let registry = DisableRegistry::load_from_env();
        assert!(!registry.hook_disabled("any"));
        assert!(!registry.tool_disabled("any"));
        crate::env::remove_var("JCODE_DISABLE_HOOK");
        crate::env::remove_var("JCODE_DISABLE_TOOL");
    }

    #[test]
    fn test_flag_env_var_names() {
        assert_eq!(DisableFlag::All.env_var(), "JCODE_DISABLE_ALL");
        assert_eq!(DisableFlag::Hooks.env_var(), "JCODE_DISABLE_HOOKS");
        assert_eq!(DisableFlag::Plugins.env_var(), "JCODE_DISABLE_PLUGINS");
        assert_eq!(DisableFlag::Memory.env_var(), "JCODE_DISABLE_MEMORY");
        assert_eq!(
            DisableFlag::PowerInhibit.env_var(),
            "JCODE_DISABLE_POWER_INHIBIT"
        );
    }

    #[test]
    fn test_truthy_case_insensitive() {
        crate::env::set_var("TEST_TRUTHY_UPPER", "TRUE");
        crate::env::set_var("TEST_TRUTHY_MIXED", "Yes");
        assert!(is_env_truthy("TEST_TRUTHY_UPPER"));
        assert!(is_env_truthy("TEST_TRUTHY_MIXED"));
        crate::env::remove_var("TEST_TRUTHY_UPPER");
        crate::env::remove_var("TEST_TRUTHY_MIXED");
    }

    #[test]
    fn test_whitespace_trimmed() {
        crate::env::set_var("TEST_TRUTHY_SPACES", "  1  ");
        assert!(is_env_truthy("TEST_TRUTHY_SPACES"));
        crate::env::remove_var("TEST_TRUTHY_SPACES");
    }

    #[test]
    fn test_comma_list_whitespace_handling() {
        crate::env::set_var("JCODE_DISABLE_TOOL", "  bash , edit  ");
        let registry = DisableRegistry::load_from_env();
        assert!(registry.tool_disabled("bash"));
        assert!(registry.tool_disabled("edit"));
        assert!(!registry.tool_disabled("read"));
        crate::env::remove_var("JCODE_DISABLE_TOOL");
    }
}
