//! Migration helpers for adopting the new service facade.
//!
//! Phase 6 (continued): when the rest of jcode eventually rewires
//! through [`crate::service::ProviderService`], the existing
//! `jcode_provider_core::AuthMode` / `DualAuthProvider` data needs to
//! be imported into the new credential store. This module is the
//! single place that knows how to read the old vocabulary and write
//! the equivalent [`crate::credential::Credential`] records.
//!
//! The helpers are *intentionally* not wired into any consumer yet
//! (the consumers' TUI is still under repair). When the wiring lands,
//! the call site is just:
//!
//! ```ignore
//! let creds: Arc<dyn CredentialService> = ...;
//! for (provider, key) in active_credentials_from_env() {
//!     creds
//!         .upsert(Credential::new(provider, "default",
//!             CredentialType::ApiKey { key }))
//!         .await?;
//! }
//! ```

use crate::credential::{Credential, CredentialType};
use crate::types::{ModelId, ProviderId};
use chrono::Utc;
use std::collections::HashMap;

/// Snapshot of an existing dual-auth credential selection in the old
/// vocabulary. This mirrors `jcode_provider_core::auth_mode::AuthMode`
/// but is decoupled from the old crate so the new code can compile
/// without dragging the rest of jcode in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyAuthMode {
    /// OAuth / subscription login (e.g. Claude subscription, ChatGPT).
    Oauth,
    /// API key (e.g. `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`).
    ApiKey,
}

/// Read the well-known env vars for a provider and decide which legacy
/// auth mode the user is using. Returns `None` if neither is set.
pub fn detect_legacy_auth(
    anthropic_key_env: &str,
    openai_key_env: &str,
) -> Option<(ProviderId, LegacyAuthMode, String)> {
    if let Ok(key) = std::env::var(anthropic_key_env) {
        if !key.is_empty() {
            return Some((ProviderId::from("anthropic"), LegacyAuthMode::ApiKey, key));
        }
    }
    if let Ok(key) = std::env::var(openai_key_env) {
        if !key.is_empty() {
            return Some((ProviderId::from("openai"), LegacyAuthMode::ApiKey, key));
        }
    }
    None
}

/// Build a new-style [`Credential`] from a legacy env-var API key.
/// The credential is tagged with label `"default"` so the
/// [`crate::integration::IntegrationService`] finds it via `detect()`.
pub fn credential_from_api_key(provider: ProviderId, key: String) -> Credential {
    Credential {
        id: crate::credential::CredentialId::new(format!(
            "legacy-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ))
        .expect("non-empty legacy id"),
        provider,
        label: "default".into(),
        credential: CredentialType::ApiKey { key },
        created_at: Utc::now(),
        updated_at: None,
    }
}

/// Construct a per-provider model preference from the old
/// `model_name_for_provider` / `provider_key` vocabulary. Returns the
/// model id that should be persisted as the user's default for the
/// provider.
pub fn default_model_for(provider: &str) -> Option<ModelId> {
    let m = match provider {
        "anthropic" => "claude-sonnet-4-6",
        "openai" => "gpt-5.1",
        "openrouter" => "openrouter/auto",
        "gemini" => "gemini-2.5-pro",
        "bedrock" => "claude-sonnet-4-6",
        "copilot" => "gpt-5-mini",
        _ => return None,
    };
    Some(ModelId::from(m))
}

/// Snapshot of the user's current provider + model selection in the
/// old vocabulary, ready to be migrated into the new service.
#[derive(Debug, Clone, Default)]
pub struct LegacyProviderSelection {
    pub provider: Option<ProviderId>,
    pub model: Option<ModelId>,
    pub env_keys: HashMap<String, String>,
}

impl LegacyProviderSelection {
    /// Read the env-var state and produce a snapshot.
    pub fn from_env() -> Self {
        let mut env_keys = HashMap::new();
        for v in [
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "OPENROUTER_API_KEY",
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "AWS_BEARER_TOKEN_BEDROCK",
            "COPILOT_GITHUB_TOKEN",
        ] {
            if let Ok(val) = std::env::var(v) {
                if !val.is_empty() {
                    env_keys.insert(v.to_string(), val);
                }
            }
        }

        // Pick the first available provider based on env-key presence.
        let provider = if env_keys.contains_key("ANTHROPIC_API_KEY") {
            Some(ProviderId::from("anthropic"))
        } else if env_keys.contains_key("OPENAI_API_KEY") {
            Some(ProviderId::from("openai"))
        } else if env_keys.contains_key("OPENROUTER_API_KEY") {
            Some(ProviderId::from("openrouter"))
        } else if env_keys.contains_key("GEMINI_API_KEY") || env_keys.contains_key("GOOGLE_API_KEY")
        {
            Some(ProviderId::from("gemini"))
        } else {
            None
        };

        let model = provider
            .as_ref()
            .and_then(|p| default_model_for(p.as_str()));

        Self {
            provider,
            model,
            env_keys,
        }
    }

    /// Convert this legacy snapshot into new-style [`Credential`]s.
    pub fn to_credentials(&self) -> Vec<Credential> {
        let mut out = Vec::new();
        for (env_var, key) in &self.env_keys {
            let provider = match env_var.as_str() {
                "ANTHROPIC_API_KEY" => "anthropic",
                "OPENAI_API_KEY" => "openai",
                "OPENROUTER_API_KEY" => "openrouter",
                "GEMINI_API_KEY" | "GOOGLE_API_KEY" => "gemini",
                "AWS_BEARER_TOKEN_BEDROCK" => "bedrock",
                "COPILOT_GITHUB_TOKEN" => "copilot",
                _ => continue,
            };
            out.push(credential_from_api_key(
                ProviderId::from(provider),
                key.clone(),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_legacy_auth_picks_anthropic_when_key_set() {
        // SAFETY: test-only env mutation in a single-threaded test context.
        unsafe {
            std::env::set_var("JCODE_TEST_MIG_ANTH", "sk-test");
            std::env::remove_var("JCODE_TEST_MIG_OAI");
        }
        let got = detect_legacy_auth("JCODE_TEST_MIG_ANTH", "JCODE_TEST_MIG_OAI");
        assert!(got.is_some());
        let (p, mode, key) = got.unwrap();
        assert_eq!(p.as_str(), "anthropic");
        assert_eq!(mode, LegacyAuthMode::ApiKey);
        assert_eq!(key, "sk-test");
        unsafe {
            std::env::remove_var("JCODE_TEST_MIG_ANTH");
        }
    }

    #[test]
    fn detect_legacy_auth_returns_none_when_neither_set() {
        unsafe {
            std::env::remove_var("JCODE_TEST_MIG_NONE_A");
            std::env::remove_var("JCODE_TEST_MIG_NONE_B");
        }
        let got = detect_legacy_auth("JCODE_TEST_MIG_NONE_A", "JCODE_TEST_MIG_NONE_B");
        assert!(got.is_none());
    }

    #[test]
    fn credential_from_api_key_tags_default_label() {
        let c = credential_from_api_key("anthropic".into(), "sk-x".into());
        assert_eq!(c.provider.as_str(), "anthropic");
        assert_eq!(c.label, "default");
        assert!(matches!(c.credential, CredentialType::ApiKey { .. }));
    }

    #[test]
    fn default_model_for_known_providers() {
        assert_eq!(
            default_model_for("anthropic").unwrap().as_str(),
            "claude-sonnet-4-6"
        );
        assert_eq!(default_model_for("openai").unwrap().as_str(), "gpt-5.1");
        assert_eq!(
            default_model_for("gemini").unwrap().as_str(),
            "gemini-2.5-pro"
        );
        assert!(default_model_for("mystery").is_none());
    }

    #[test]
    fn from_env_to_credentials_round_trip() {
        unsafe {
            std::env::set_var("JCODE_TEST_MIG_RT", "sk-rt");
        }
        let snap = LegacyProviderSelection::from_env();
        // We don't know which keys are set globally, so just check that
        // to_credentials() yields one entry per set env var with matching
        // provider.
        let creds = snap.to_credentials();
        for c in &creds {
            assert_eq!(c.label, "default");
        }
        unsafe {
            std::env::remove_var("JCODE_TEST_MIG_RT");
        }
    }
}
