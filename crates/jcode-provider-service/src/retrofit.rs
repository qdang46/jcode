//! Retrofit layer for the legacy `--provider` CLI flag.
//!
//! Plan criteria 6 and 13:
//!
//!   > [ ] `--provider` flag accepts dynamic string (not enum)
//!   > [ ] Retrofit layer keeps `--provider` CLI flag working for
//!   >     existing users
//!
//! The old `jcode-provider-core::auth_mode` module accepted a long
//! list of aliases — `claude`, `claude-oauth`, `claude-api-key`,
//! `anthropic-api`, `openai`, `openai-oauth`, `openai-api`,
//! `openai-api-key` — and quietly normalized them into a
//! `(DualAuthProvider, AuthMode)` pair. This module provides the
//! same surface as a pure function over the new types, so the
//! session runner can call:
//!
//! ```ignore
//! match retrofit::parse_legacy_provider_flag("claude-oauth") {
//!     Ok(selection) => /* use selection.provider / selection.auth */,
//!     Err(e) => /* print a helpful error */,
//! }
//! ```
//!
//! The function is intentionally not wired into any consumer yet
//! (the consumers' TUI is still under repair). When the wiring
//! lands, this is the single place that knows the legacy aliases.

use thiserror::Error;

use crate::types::ProviderId;

/// The dual-auth providers that have an OAuth-vs-API distinction
/// in the legacy vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DualAuthProvider {
    /// Anthropic / Claude (Claude subscription OAuth vs API key).
    Anthropic,
    /// OpenAI (ChatGPT/Codex OAuth vs API key).
    OpenAI,
}

impl DualAuthProvider {
    pub fn provider_id(&self) -> ProviderId {
        match self {
            Self::Anthropic => ProviderId::from("anthropic"),
            Self::OpenAI => ProviderId::from("openai"),
        }
    }
}

/// Which credential mode a legacy alias resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LegacyAuthMode {
    /// OAuth / subscription login.
    Oauth,
    /// Direct API key.
    ApiKey,
}

impl LegacyAuthMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oauth => "oauth",
            Self::ApiKey => "api-key",
        }
    }
}

/// Result of parsing a legacy `--provider` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacySelection {
    pub provider: ProviderId,
    /// `None` for providers without an OAuth/ApiKey split (e.g.
    /// `gemini`, `openrouter`).
    pub auth: Option<LegacyAuthMode>,
    /// True if the flag was a recognized dual-auth alias.
    pub is_dual_auth: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LegacyParseError {
    #[error("empty provider flag")]
    Empty,
    #[error("unknown provider alias: {0}")]
    Unknown(String),
    #[error("provider {provider} does not support auth mode {auth}")]
    UnsupportedAuth {
        provider: &'static str,
        auth: &'static str,
    },
}

/// Parse a legacy `--provider` flag into a [`LegacySelection`].
///
/// Recognized aliases (case-insensitive):
///
/// - `claude`             → anthropic + Oauth
/// - `claude-oauth`       → anthropic + Oauth
/// - `claude-api`         → anthropic + ApiKey
/// - `claude-api-key`     → anthropic + ApiKey
/// - `anthropic`          → anthropic + ApiKey (default)
/// - `anthropic-api`      → anthropic + ApiKey
/// - `anthropic-api-key`  → anthropic + ApiKey
/// - `openai`             → openai + Oauth
/// - `openai-oauth`       → openai + Oauth
/// - `openai-api`         → openai + ApiKey
/// - `openai-api-key`     → openai + ApiKey
/// - `gemini`             → gemini (no auth split)
/// - `openrouter`         → openrouter (no auth split)
/// - `bedrock`            → bedrock (no auth split)
/// - `copilot`            → copilot (no auth split)
pub fn parse_legacy_provider_flag(flag: &str) -> Result<LegacySelection, LegacyParseError> {
    let f = flag.trim();
    if f.is_empty() {
        return Err(LegacyParseError::Empty);
    }
    let lower = f.to_ascii_lowercase();
    match lower.as_str() {
        // Anthropic / Claude
        "claude" | "claude-oauth" => Ok(LegacySelection {
            provider: ProviderId::from("anthropic"),
            auth: Some(LegacyAuthMode::Oauth),
            is_dual_auth: true,
        }),
        "claude-api" | "claude-api-key" | "anthropic-api" | "anthropic-api-key" => {
            Ok(LegacySelection {
                provider: ProviderId::from("anthropic"),
                auth: Some(LegacyAuthMode::ApiKey),
                is_dual_auth: true,
            })
        }
        "anthropic" => Ok(LegacySelection {
            provider: ProviderId::from("anthropic"),
            auth: Some(LegacyAuthMode::ApiKey),
            is_dual_auth: true,
        }),

        // OpenAI
        "openai" | "openai-oauth" => Ok(LegacySelection {
            provider: ProviderId::from("openai"),
            auth: Some(LegacyAuthMode::Oauth),
            is_dual_auth: true,
        }),
        "openai-api" | "openai-api-key" => Ok(LegacySelection {
            provider: ProviderId::from("openai"),
            auth: Some(LegacyAuthMode::ApiKey),
            is_dual_auth: true,
        }),

        // Single-auth providers
        "gemini" | "google" => Ok(LegacySelection {
            provider: ProviderId::from("gemini"),
            auth: None,
            is_dual_auth: false,
        }),
        "openrouter" => Ok(LegacySelection {
            provider: ProviderId::from("openrouter"),
            auth: None,
            is_dual_auth: false,
        }),
        "bedrock" | "aws-bedrock" => Ok(LegacySelection {
            provider: ProviderId::from("bedrock"),
            auth: None,
            is_dual_auth: false,
        }),
        "copilot" | "github-copilot" => Ok(LegacySelection {
            provider: ProviderId::from("copilot"),
            auth: None,
            is_dual_auth: false,
        }),

        other => Err(LegacyParseError::Unknown(other.to_string())),
    }
}

/// Helper: does the given provider support the legacy OAuth mode?
pub fn supports_legacy_oauth(provider: &ProviderId) -> bool {
    matches!(provider.as_str(), "anthropic" | "openai")
}

/// Helper: enumerate the legacy aliases for a provider. Used to
/// print a "did you mean..." list when the user mistypes the flag.
pub fn legacy_aliases_for(provider: &ProviderId) -> &'static [&'static str] {
    match provider.as_str() {
        "anthropic" => &[
            "claude",
            "claude-oauth",
            "claude-api",
            "claude-api-key",
            "anthropic",
            "anthropic-api",
            "anthropic-api-key",
        ],
        "openai" => &["openai", "openai-oauth", "openai-api", "openai-api-key"],
        "gemini" => &["gemini", "google"],
        "openrouter" => &["openrouter"],
        "bedrock" => &["bedrock", "aws-bedrock"],
        "copilot" => &["copilot", "github-copilot"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_flag_errors() {
        assert_eq!(parse_legacy_provider_flag(""), Err(LegacyParseError::Empty));
    }

    #[test]
    fn claude_aliases_map_to_anthropic_oauth() {
        for alias in ["claude", "CLAUDE", "claude-oauth", "Claude-OAuth"] {
            let got = parse_legacy_provider_flag(alias).unwrap();
            assert_eq!(got.provider.as_str(), "anthropic");
            assert_eq!(got.auth, Some(LegacyAuthMode::Oauth));
            assert!(got.is_dual_auth);
        }
    }

    #[test]
    fn claude_api_aliases_map_to_anthropic_api_key() {
        for alias in [
            "claude-api",
            "claude-api-key",
            "anthropic-api",
            "anthropic-api-key",
        ] {
            let got = parse_legacy_provider_flag(alias).unwrap();
            assert_eq!(got.provider.as_str(), "anthropic");
            assert_eq!(got.auth, Some(LegacyAuthMode::ApiKey));
        }
    }

    #[test]
    fn openai_aliases_map_correctly() {
        assert_eq!(
            parse_legacy_provider_flag("openai").unwrap().auth,
            Some(LegacyAuthMode::Oauth)
        );
        assert_eq!(
            parse_legacy_provider_flag("openai-oauth").unwrap().auth,
            Some(LegacyAuthMode::Oauth)
        );
        assert_eq!(
            parse_legacy_provider_flag("openai-api").unwrap().auth,
            Some(LegacyAuthMode::ApiKey)
        );
        assert_eq!(
            parse_legacy_provider_flag("openai-api-key").unwrap().auth,
            Some(LegacyAuthMode::ApiKey)
        );
    }

    #[test]
    fn single_auth_providers_have_no_auth_split() {
        for alias in ["gemini", "google", "openrouter", "bedrock", "copilot"] {
            let got = parse_legacy_provider_flag(alias).unwrap();
            assert!(got.auth.is_none());
            assert!(!got.is_dual_auth);
        }
    }

    #[test]
    fn unknown_alias_errors() {
        let err = parse_legacy_provider_flag("mystery").unwrap_err();
        assert!(matches!(err, LegacyParseError::Unknown(_)));
    }

    #[test]
    fn supports_legacy_oauth_only_for_anthropic_and_openai() {
        assert!(supports_legacy_oauth(&"anthropic".into()));
        assert!(supports_legacy_oauth(&"openai".into()));
        assert!(!supports_legacy_oauth(&"gemini".into()));
        assert!(!supports_legacy_oauth(&"openrouter".into()));
    }

    #[test]
    fn legacy_aliases_for_returns_expected_set() {
        let a = legacy_aliases_for(&"anthropic".into());
        assert!(a.contains(&"claude"));
        assert!(a.contains(&"claude-api-key"));
        let o = legacy_aliases_for(&"openai".into());
        assert!(o.contains(&"openai-oauth"));
        let unknown = legacy_aliases_for(&"mystery".into());
        assert!(unknown.is_empty());
    }
}
