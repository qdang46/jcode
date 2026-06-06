//! Model tier abstraction.
//!
//! A "tier" is a **user-defined named slot** that maps to a concrete model id.
//! It is intentionally NOT an opinionated catalog — jcode does not maintain
//! per-provider tier defaults like Codebuff/OpenRouter does.
//!
//! ## Why slots, not catalog?
//!
//! jcode users connect a single provider via OAuth (Claude Pro, ChatGPT Plus,
//! Gemini Advanced, etc.) and pay through that subscription. Auto-downgrading
//! to a "cheaper tier" without their consent is wrong — they already chose
//! the model they want. So the default is: agents inherit the session's
//! current model.
//!
//! Power users (pay-per-token API keys, multi-account setups) can opt in by
//! setting two env vars, exactly mirroring `model_routing.rs` (#100):
//!
//! ```bash
//! JCODE_ROUTING_ROUTINE=claude-haiku-4-5
//! JCODE_ROUTING_THINKING=claude-opus-4-7
//! ```
//!
//! Agent definitions reference tiers by name:
//!
//! ```toml
//! [agent]
//! id = "file-picker"
//! prefer_tier = "routine"   # uses JCODE_ROUTING_ROUTINE if set
//! ```
//!
//! ## Resolution order
//!
//! 1. `agent.model_override` (explicit, highest priority)
//! 2. `agent.prefer_tier` + corresponding env var set
//! 3. Caller-provided `current_session_model` fallback
//!
//! No catalog. No magic. The only "magic" is reading the env var, which is
//! the existing #100 contract.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A user-defined tier slot. Currently only two are supported because that
/// matches `model_routing.rs` (#100). Adding tiers later is additive — the
/// env var name pattern is `JCODE_ROUTING_<UPPER_TIER>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    /// Cheap / fast / lower-effort work: file pickers, basher,
    /// summarizers. Reads `JCODE_ROUTING_ROUTINE`.
    Routine,
    /// Premium / reasoning work: editor, reviewer, planner.
    /// Reads `JCODE_ROUTING_THINKING`.
    Thinking,
}

impl ModelTier {
    /// The env var name that backs this tier slot. Returns the same string
    /// shape as `model_routing.rs` (#100) so the two systems stay aligned.
    pub fn env_var(&self) -> &'static str {
        match self {
            ModelTier::Routine => "JCODE_ROUTING_ROUTINE",
            ModelTier::Thinking => "JCODE_ROUTING_THINKING",
        }
    }

    /// Read the user-configured model id for this tier from the environment.
    /// Returns `None` when the env var is unset, blank, or whitespace-only —
    /// callers should fall back to the session's current model.
    pub fn read_user_override(&self) -> Option<String> {
        std::env::var(self.env_var())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Parse a tier name from a string, accepting common variants.
    pub fn parse(s: &str) -> Option<ModelTier> {
        match s.trim().to_ascii_lowercase().as_str() {
            "routine" | "fast" | "cheap" | "lite" => Some(ModelTier::Routine),
            "thinking" | "reasoning" | "premium" | "deep" => Some(ModelTier::Thinking),
            _ => None,
        }
    }
}

impl fmt::Display for ModelTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelTier::Routine => f.write_str("routine"),
            ModelTier::Thinking => f.write_str("thinking"),
        }
    }
}

/// Resolve which model id to use for a given tier preference + override pair.
///
/// Priority:
/// 1. `model_override` — explicit, highest priority.
/// 2. `prefer_tier` + corresponding env var set.
/// 3. `current_session_model` — caller-provided fallback.
///
/// `current_session_model` is required because there's no other safe default:
/// the runtime doesn't know which provider/model the session is using.
pub fn resolve_model(
    model_override: Option<&str>,
    prefer_tier: Option<ModelTier>,
    current_session_model: &str,
) -> String {
    if let Some(override_id) = model_override.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }) {
        return override_id;
    }

    if let Some(tier) = prefer_tier
        && let Some(tier_model) = tier.read_user_override()
    {
        return tier_model;
    }

    current_session_model.to_string()
}

/// Diagnostic-friendly explanation of which slot was used. Useful for
/// `jcode doctor` output so users can see exactly why a given agent picked
/// the model it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionSource {
    /// Used `agent.model_override` directly.
    Override(String),
    /// Used the env var backing `tier`.
    Tier { tier: ModelTier, model: String },
    /// Tier was preferred but the env var was unset, so fell back to the
    /// session's current model.
    TierFallback { tier: ModelTier, model: String },
    /// No override or tier preference; using the session's current model.
    SessionDefault(String),
}

impl ResolutionSource {
    pub fn model_id(&self) -> &str {
        match self {
            ResolutionSource::Override(m)
            | ResolutionSource::Tier { model: m, .. }
            | ResolutionSource::TierFallback { model: m, .. }
            | ResolutionSource::SessionDefault(m) => m,
        }
    }
}

/// Same as `resolve_model` but returns provenance information for diagnostics.
pub fn resolve_model_with_source(
    model_override: Option<&str>,
    prefer_tier: Option<ModelTier>,
    current_session_model: &str,
) -> ResolutionSource {
    if let Some(override_id) = model_override.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }) {
        return ResolutionSource::Override(override_id);
    }

    if let Some(tier) = prefer_tier {
        match tier.read_user_override() {
            Some(model) => return ResolutionSource::Tier { tier, model },
            None => {
                return ResolutionSource::TierFallback {
                    tier,
                    model: current_session_model.to_string(),
                };
            }
        }
    }

    ResolutionSource::SessionDefault(current_session_model.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mutex to serialize env-var manipulation across tests in this module.
    /// Without this, `cargo test` runs tests in parallel and they trample
    /// each other's `JCODE_ROUTING_*` state.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env_lock<F: FnOnce()>(f: F) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Snapshot + restore env vars we mutate so test order is irrelevant.
        let saved_routine = std::env::var_os("JCODE_ROUTING_ROUTINE");
        let saved_thinking = std::env::var_os("JCODE_ROUTING_THINKING");
        unsafe {
            std::env::remove_var("JCODE_ROUTING_ROUTINE");
            std::env::remove_var("JCODE_ROUTING_THINKING");
        }
        f();
        unsafe {
            match saved_routine {
                Some(v) => std::env::set_var("JCODE_ROUTING_ROUTINE", v),
                None => std::env::remove_var("JCODE_ROUTING_ROUTINE"),
            }
            match saved_thinking {
                Some(v) => std::env::set_var("JCODE_ROUTING_THINKING", v),
                None => std::env::remove_var("JCODE_ROUTING_THINKING"),
            }
        }
        drop(guard);
    }

    #[test]
    fn parse_tier_accepts_aliases() {
        assert_eq!(ModelTier::parse("routine"), Some(ModelTier::Routine));
        assert_eq!(ModelTier::parse("Routine"), Some(ModelTier::Routine));
        assert_eq!(ModelTier::parse("FAST"), Some(ModelTier::Routine));
        assert_eq!(ModelTier::parse("thinking"), Some(ModelTier::Thinking));
        assert_eq!(ModelTier::parse("reasoning"), Some(ModelTier::Thinking));
        assert_eq!(ModelTier::parse("deep"), Some(ModelTier::Thinking));
        assert_eq!(ModelTier::parse(""), None);
        assert_eq!(ModelTier::parse("nonsense"), None);
    }

    #[test]
    fn override_wins_over_tier_and_session_default() {
        with_env_lock(|| {
            unsafe {
                std::env::set_var("JCODE_ROUTING_THINKING", "should-be-ignored");
            }
            let got = resolve_model(
                Some("explicit-model"),
                Some(ModelTier::Thinking),
                "session-default",
            );
            assert_eq!(got, "explicit-model");
        });
    }

    #[test]
    fn tier_uses_env_var_when_set() {
        with_env_lock(|| {
            unsafe {
                std::env::set_var("JCODE_ROUTING_ROUTINE", "haiku-4-5");
            }
            let got = resolve_model(None, Some(ModelTier::Routine), "session-default");
            assert_eq!(got, "haiku-4-5");
        });
    }

    #[test]
    fn tier_falls_back_when_env_unset() {
        with_env_lock(|| {
            // env var explicitly removed by lock setup
            let got = resolve_model(None, Some(ModelTier::Thinking), "session-default");
            assert_eq!(got, "session-default");
        });
    }

    #[test]
    fn no_tier_no_override_uses_session_default() {
        with_env_lock(|| {
            let got = resolve_model(None, None, "session-default");
            assert_eq!(got, "session-default");
        });
    }

    #[test]
    fn empty_override_string_treated_as_unset() {
        with_env_lock(|| {
            let got = resolve_model(Some("   "), None, "session-default");
            assert_eq!(got, "session-default");
        });
    }

    #[test]
    fn resolution_source_reports_override() {
        with_env_lock(|| {
            let src = resolve_model_with_source(Some("forced"), None, "session");
            assert!(matches!(src, ResolutionSource::Override(ref m) if m == "forced"));
            assert_eq!(src.model_id(), "forced");
        });
    }

    #[test]
    fn resolution_source_reports_tier_hit() {
        with_env_lock(|| {
            unsafe {
                std::env::set_var("JCODE_ROUTING_THINKING", "opus-4-7");
            }
            let src = resolve_model_with_source(None, Some(ModelTier::Thinking), "fallback");
            match src {
                ResolutionSource::Tier { tier, model } => {
                    assert_eq!(tier, ModelTier::Thinking);
                    assert_eq!(model, "opus-4-7");
                }
                other => panic!("expected Tier, got {:?}", other),
            }
        });
    }

    #[test]
    fn resolution_source_reports_tier_fallback() {
        with_env_lock(|| {
            // env unset
            let src = resolve_model_with_source(None, Some(ModelTier::Routine), "session");
            match src {
                ResolutionSource::TierFallback { tier, model } => {
                    assert_eq!(tier, ModelTier::Routine);
                    assert_eq!(model, "session");
                }
                other => panic!("expected TierFallback, got {:?}", other),
            }
        });
    }
}
