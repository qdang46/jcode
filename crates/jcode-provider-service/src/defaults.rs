//! Per-provider default model store.
//!
//! Persists the user's "default model" choice (per provider, plus a
//! global default) so the next session picks up where the user left
//! off. Backed by a simple JSON file under the user's data directory
//! (default: `~/.jcode/provider-defaults.json`). No migration story
//! is needed — if the file is missing or malformed, defaults fall
//! back to the catalog's heuristics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{ModelId, ProviderId};

/// Persisted default-model state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderDefaults {
    /// Global default (provider, model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<(ProviderId, ModelId)>,
    /// Per-provider default model.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub per_provider: HashMap<ProviderId, ModelId>,
}

impl ProviderDefaults {
    /// Empty defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from a JSON file. Returns an empty `ProviderDefaults` if
    /// the file is missing; returns `Invalid` if the file is present
    /// but malformed.
    pub fn load(path: &Path) -> Result<Self, DefaultsError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| DefaultsError::Io(e.to_string()))?;
        serde_json::from_str(&raw).map_err(|e| DefaultsError::Invalid(e.to_string()))
    }

    /// Save to a JSON file. Creates the parent directory if needed.
    pub fn save(&self, path: &Path) -> Result<(), DefaultsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| DefaultsError::Io(e.to_string()))?;
        }
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| DefaultsError::Invalid(e.to_string()))?;
        std::fs::write(path, raw).map_err(|e| DefaultsError::Io(e.to_string()))?;
        Ok(())
    }

    /// Set the global default.
    pub fn set_global(&mut self, provider: ProviderId, model: ModelId) {
        self.global = Some((provider, model));
    }

    /// Set the default model for a specific provider.
    pub fn set_for_provider(&mut self, provider: ProviderId, model: ModelId) {
        self.per_provider.insert(provider, model);
    }

    /// Resolve the model for a given provider:
    /// 1. Per-provider override.
    /// 2. Global default (if its provider matches).
    /// 3. The provided fallback (from Catalog::default()).
    pub fn resolve(&self, provider: &ProviderId, fallback: Option<ModelId>) -> Option<ModelId> {
        if let Some(m) = self.per_provider.get(provider) {
            return Some(m.clone());
        }
        if let Some((p, m)) = &self.global
            && p == provider
        {
            return Some(m.clone());
        }
        fallback
    }
}

#[derive(Debug, Error)]
pub enum DefaultsError {
    #[error("io error: {0}")]
    Io(String),
    #[error("invalid defaults file: {0}")]
    Invalid(String),
}

/// Default path: `~/.jcode/provider-defaults.json`.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".jcode")
            .join("provider-defaults.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_when_file_missing() {
        let p = std::env::temp_dir().join("jcode-defaults-missing.json");
        let _ = std::fs::remove_file(&p);
        let d = ProviderDefaults::load(&p).unwrap();
        assert!(d.global.is_none());
        assert!(d.per_provider.is_empty());
    }

    #[test]
    fn round_trip() {
        let p = std::env::temp_dir().join("jcode-defaults-rt.json");
        let _ = std::fs::remove_file(&p);
        let mut d = ProviderDefaults::new();
        d.set_global("anthropic".into(), "claude-sonnet-4-6".into());
        d.set_for_provider("openai".into(), "gpt-5-mini".into());
        d.save(&p).unwrap();
        let loaded = ProviderDefaults::load(&p).unwrap();
        assert_eq!(loaded, d);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn per_provider_takes_priority_over_global() {
        let mut d = ProviderDefaults::new();
        d.set_global("openai".into(), "gpt-5.1".into());
        d.set_for_provider("anthropic".into(), "claude-haiku-4-5".into());
        assert_eq!(
            d.resolve(&"anthropic".into(), None).unwrap().as_str(),
            "claude-haiku-4-5"
        );
        assert_eq!(
            d.resolve(&"openai".into(), None).unwrap().as_str(),
            "gpt-5.1"
        );
    }

    #[test]
    fn global_falls_through_to_fallback() {
        let d = ProviderDefaults::new();
        assert_eq!(
            d.resolve(&"anthropic".into(), Some("claude-sonnet-4-6".into()))
                .unwrap()
                .as_str(),
            "claude-sonnet-4-6"
        );
    }

    #[test]
    fn invalid_file_surfaces_error() {
        let p = std::env::temp_dir().join("jcode-defaults-bad.json");
        std::fs::write(&p, "not json").unwrap();
        let err = ProviderDefaults::load(&p).unwrap_err();
        assert!(matches!(err, DefaultsError::Invalid(_)));
        let _ = std::fs::remove_file(&p);
    }
}
