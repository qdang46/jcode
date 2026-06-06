//! Per-agent permission mode for tool execution safety.
//!
//! Mirrors `dcg_core::Mode` but is intentionally self-contained in the
//! dependency-light `jcode-agent-runtime` crate. The runtime converts
//! this enum to `dcg_core::Mode` at spawn time.
//!
//! ## Design
//!
//! The permission mode controls how tool calls are evaluated during an
//! agent's execution:
//!
//! - `Default` — rule-based: read-only tools auto-allowed, writes prompt.
//! - `AcceptEdits` — file operations auto-allowed, network/spawn prompt.
//! - `Plan` — read-only: writes denied without prompting.
//! - `DontAsk` — allow-listed tools pass, never prompt.
//! - `BypassPermissions` — skip all evaluation.
//! - `Auto` — LLM-based classifier decides per call.
//!
//! When `AgentDefinition.permission_mode` is `None`, the agent inherits
//! the session's current permission mode (set via CLI `--permission-mode`
//! or cycled at runtime in the TUI).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Per-agent permission mode for tool execution safety.
///
/// This enum intentionally mirrors `dcg_core::Mode` (from the
/// `destructive_command_guard` crate) so that `jcode-agent-runtime`
/// does not need to depend on `dcg-core` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    /// Rule-based classification using the legacy `AUTO_ALLOWED` list.
    /// Read-only tools auto-allowed; writes require permission.
    #[default]
    Default,
    /// File operations (edit, write, patch) auto-allowed. Network,
    /// spawn, and irreversible operations still prompt.
    AcceptEdits,
    /// Read-only mode: write operations denied without prompting.
    /// Useful for reviewer/observer agents.
    Plan,
    /// Only allow-listed tools pass; never prompt the user.
    /// Useful for unattended/CI agents.
    DontAsk,
    /// Skip all permission evaluation. Use with caution.
    BypassPermissions,
    /// LLM-based classifier decides per tool call.
    Auto,
}

impl PermissionMode {
    /// String representation matching the wire format used by TOML
    /// definitions and the CLI.
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionMode::Default => "default",
            PermissionMode::AcceptEdits => "accept-edits",
            PermissionMode::Plan => "plan",
            PermissionMode::DontAsk => "dont-ask",
            PermissionMode::BypassPermissions => "bypass-permissions",
            PermissionMode::Auto => "auto",
        }
    }

    /// Parse a permission mode from a string. Only accepts kebab-case
    /// variants matching the serde wire format for consistency.
    pub fn parse(s: &str) -> Option<PermissionMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" => Some(PermissionMode::Default),
            "accept-edits" => Some(PermissionMode::AcceptEdits),
            "plan" => Some(PermissionMode::Plan),
            "dont-ask" => Some(PermissionMode::DontAsk),
            "bypass-permissions" => Some(PermissionMode::BypassPermissions),
            "auto" => Some(PermissionMode::Auto),
            _ => None,
        }
    }
}

impl fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_kebab_case_only() {
        assert_eq!(
            PermissionMode::parse("default"),
            Some(PermissionMode::Default)
        );
        assert_eq!(
            PermissionMode::parse("accept-edits"),
            Some(PermissionMode::AcceptEdits)
        );
        assert_eq!(PermissionMode::parse("plan"), Some(PermissionMode::Plan));
        assert_eq!(
            PermissionMode::parse("dont-ask"),
            Some(PermissionMode::DontAsk)
        );
        assert_eq!(
            PermissionMode::parse("bypass-permissions"),
            Some(PermissionMode::BypassPermissions)
        );
        assert_eq!(PermissionMode::parse("auto"), Some(PermissionMode::Auto));
        assert_eq!(PermissionMode::parse(""), None);
        assert_eq!(PermissionMode::parse("nonsense"), None);
        // Non-kebab-case variants are rejected for serde consistency
        assert_eq!(PermissionMode::parse("accept_edits"), None);
        assert_eq!(PermissionMode::parse("AcceptEdits"), None);
        assert_eq!(PermissionMode::parse("bypass_permissions"), None);
    }

    #[test]
    fn default_is_default() {
        assert_eq!(PermissionMode::default(), PermissionMode::Default);
    }

    #[test]
    fn serde_roundtrip_kebab_case() {
        // TOML wire format uses kebab-case per serde(rename_all)
        let s = serde_json::to_string(&PermissionMode::AcceptEdits).unwrap();
        assert_eq!(s, "\"accept-edits\"");
        let back: PermissionMode = serde_json::from_str("\"accept-edits\"").unwrap();
        assert_eq!(back, PermissionMode::AcceptEdits);
    }

    #[test]
    fn serde_roundtrip_all_variants() {
        for variant in [
            PermissionMode::Default,
            PermissionMode::AcceptEdits,
            PermissionMode::Plan,
            PermissionMode::DontAsk,
            PermissionMode::BypassPermissions,
            PermissionMode::Auto,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: PermissionMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn display_matches_as_str() {
        for variant in [
            PermissionMode::Default,
            PermissionMode::AcceptEdits,
            PermissionMode::Plan,
            PermissionMode::DontAsk,
            PermissionMode::BypassPermissions,
            PermissionMode::Auto,
        ] {
            assert_eq!(format!("{variant}"), variant.as_str());
        }
    }
}
