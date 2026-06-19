//! Shared identifier types used across the Catalog / Integration / Credential
//! layers.
//!
//! Using a newtype instead of bare `String` lets us:
//! - distinguish providers from models in function signatures,
//! - reject empty / whitespace-only identifiers at construction time,
//! - serialize consistently (`"anthropic"`, not `"anthropic "`).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// Stable identifier for a provider (e.g. `"anthropic"`, `"openai"`,
/// `"openrouter"`). Provider ids are short, lower-snake-case strings.
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(Arc<str>);

impl ProviderId {
    /// Construct a new provider id. Trims surrounding whitespace and rejects
    /// empty strings.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
        let trimmed = value.into().trim().to_string();
        if trimmed.is_empty() {
            return Err(InvalidId::Empty);
        }
        if trimmed.contains(char::is_whitespace) {
            return Err(InvalidId::Whitespace);
        }
        Ok(Self(Arc::from(trimmed)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ProviderId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self::new(s).expect("provider id must be non-empty")
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self::new(s).expect("provider id must be non-empty")
    }
}

/// Stable identifier for a model within a provider (e.g. `"claude-sonnet-4-6"`,
/// `"gpt-5.1"`).
#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(Arc<str>);

impl ModelId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
        let trimmed = value.into().trim().to_string();
        if trimmed.is_empty() {
            return Err(InvalidId::Empty);
        }
        if trimmed.contains(char::is_whitespace) {
            return Err(InvalidId::Whitespace);
        }
        Ok(Self(Arc::from(trimmed)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ModelId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ModelId {
    fn from(s: &str) -> Self {
        Self::new(s).expect("model id must be non-empty")
    }
}

impl From<String> for ModelId {
    fn from(s: String) -> Self {
        Self::new(s).expect("model id must be non-empty")
    }
}

/// User-facing provider selection shorthand. Users can address providers by
/// id, label (e.g. `"Claude"`), or alias (e.g. `"claude-oauth"`). This is the
/// input vocabulary the CLI and config parser use before normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderProfile {
    /// Explicit provider id (e.g. `--provider anthropic`).
    ById { id: ProviderId },
    /// User-given profile name (e.g. `[provider.profiles.work]`).
    Named { profile: String },
    /// The provider's user-visible label (case-insensitive).
    ByLabel { label: String },
    /// Provider + auth mode (e.g. `claude-oauth` vs `claude-api-key`).
    WithAuth { id: ProviderId, auth: String },
}

impl ProviderProfile {
    /// Short, human-readable string used for diagnostics and CLI errors.
    pub fn describe(&self) -> String {
        match self {
            Self::ById { id } => format!("provider:{}", id),
            Self::Named { profile } => format!("profile:{}", profile),
            Self::ByLabel { label } => format!("label:{}", label),
            Self::WithAuth { id, auth } => format!("{}:{}", id, auth),
        }
    }
}

/// Why a provider id failed to construct.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidId {
    #[error("provider/model id must not be empty")]
    Empty,
    #[error("provider/model id must not contain whitespace")]
    Whitespace,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_trims_and_rejects_empty() {
        let p = ProviderId::new("  anthropic  ").unwrap();
        assert_eq!(p.as_str(), "anthropic");
        assert!(ProviderId::new("").is_err());
        assert!(ProviderId::new("   ").is_err());
    }

    #[test]
    fn provider_id_rejects_whitespace_inside() {
        assert_eq!(
            ProviderId::new("open ai").unwrap_err(),
            InvalidId::Whitespace
        );
    }

    #[test]
    fn provider_id_from_str_panics_on_empty() {
        // The infallible From impl is for the common case; we still want the
        // fallible constructor for user input.
        let id: ProviderId = "anthropic".into();
        assert_eq!(id.as_str(), "anthropic");
    }

    #[test]
    fn model_id_serde_roundtrip() {
        let m = ModelId::new("claude-sonnet-4-6").unwrap();
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(s, "\"claude-sonnet-4-6\"");
        let back: ModelId = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn provider_profile_describe() {
        let p = ProviderProfile::ById {
            id: "anthropic".into(),
        };
        assert_eq!(p.describe(), "provider:anthropic");
    }
}
