//! Reasoning effort levels for agents.
//!
//! Mirrors the OpenAI/Anthropic reasoning effort knobs. When an agent
//! definition specifies a reasoning effort, the agent runtime forwards it
//! to the provider request (where supported). Models that don't support
//! reasoning ignore the field.

use serde::{Deserialize, Serialize};
use std::fmt;

/// How much reasoning the model should use for this agent.
///
/// Maps roughly to:
///   - `Minimal` → `effort: "minimal"` (gpt-5 family) / no thinking budget (Claude)
///   - `Low`     → `effort: "low"` / small thinking budget
///   - `Medium`  → `effort: "medium"` / default thinking budget
///   - `High`    → `effort: "high"` / large thinking budget (~32k tokens)
///
/// Default is `Medium` because that matches most agents' baseline behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Minimal,
    Low,
    #[default]
    Medium,
    High,
}

impl ReasoningEffort {
    /// String representation matching the wire format used by major providers
    /// (OpenAI Responses API `reasoning.effort`, OpenRouter `reasoning.effort`).
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasoningEffort::Minimal => "minimal",
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }

    /// Numeric rank for threshold comparison (matches `model_routing.rs`).
    /// Higher = more reasoning.
    pub fn rank(&self) -> u8 {
        match self {
            ReasoningEffort::Minimal => 0,
            ReasoningEffort::Low => 1,
            ReasoningEffort::Medium => 2,
            ReasoningEffort::High => 3,
        }
    }

    /// Parse a string value, accepting common aliases. Returns `None` for
    /// unknown input so the caller can decide whether to error or default.
    pub fn parse(s: &str) -> Option<ReasoningEffort> {
        match s.trim().to_ascii_lowercase().as_str() {
            "minimal" | "none" | "off" => Some(ReasoningEffort::Minimal),
            "low" => Some(ReasoningEffort::Low),
            "medium" | "default" => Some(ReasoningEffort::Medium),
            "high" | "max" => Some(ReasoningEffort::High),
            _ => None,
        }
    }
}

impl fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_common_aliases() {
        assert_eq!(
            ReasoningEffort::parse("minimal"),
            Some(ReasoningEffort::Minimal)
        );
        assert_eq!(
            ReasoningEffort::parse("OFF"),
            Some(ReasoningEffort::Minimal)
        );
        assert_eq!(ReasoningEffort::parse("max"), Some(ReasoningEffort::High));
        assert_eq!(
            ReasoningEffort::parse("default"),
            Some(ReasoningEffort::Medium)
        );
        assert_eq!(ReasoningEffort::parse(""), None);
        assert_eq!(ReasoningEffort::parse("absurd"), None);
    }

    #[test]
    fn rank_orders_efforts_correctly() {
        assert!(ReasoningEffort::Minimal.rank() < ReasoningEffort::Low.rank());
        assert!(ReasoningEffort::Low.rank() < ReasoningEffort::Medium.rank());
        assert!(ReasoningEffort::Medium.rank() < ReasoningEffort::High.rank());
    }

    #[test]
    fn default_is_medium() {
        assert_eq!(ReasoningEffort::default(), ReasoningEffort::Medium);
    }

    #[test]
    fn serde_roundtrip_via_lowercase() {
        let s = serde_json::to_string(&ReasoningEffort::High).unwrap();
        assert_eq!(s, "\"high\"");
        let back: ReasoningEffort = serde_json::from_str("\"medium\"").unwrap();
        assert_eq!(back, ReasoningEffort::Medium);
    }
}
