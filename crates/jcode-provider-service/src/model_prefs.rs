//! Persistent model preferences (favorites, recents).
//!
//! Plan §3 Phase 5:
//!   > 4. \`f\` toggles favorite (persisted to \`model_prefs.json\`)
//!   > 5. Enter selects model (and optionally sets default)
//!
//! The TUI picker stores favorites and recent selections
//! in-memory via \`tui_picker::PickerState\`. This module adds
//! the persistence layer: a JSON file at
//! \`~/.jcode/model_prefs.json\` that survives process restarts.
//!
//! Format:
//! \`\`\`json
//! {
//!   "favorites": [{"provider": "anthropic", "model": "claude-haiku-4-5"}, ...],
//!   "recents":   [{"provider": "openai", "model": "gpt-5-mini"}, ...]
//! }
//! \`\`\`

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{ModelId, ProviderId};

/// On-disk shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelPrefs {
    /// The user's default model. (Per the plan: "Enter selects
    /// model (and optionally sets default)".)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<FavoriteEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub favorites: Vec<FavoriteEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recents: Vec<FavoriteEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FavoriteEntry {
    pub provider: ProviderId,
    pub model: ModelId,
}

impl ModelPrefs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from a JSON file. Returns an empty `ModelPrefs` if the
    /// file is missing; returns `Invalid` if the file is present
    /// but malformed.
    pub fn load(path: &Path) -> Result<Self, PrefsError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| PrefsError::Io(e.to_string()))?;
        serde_json::from_str(&raw).map_err(|e| PrefsError::Invalid(e.to_string()))
    }

    /// Save to a JSON file. Creates the parent directory if needed.
    pub fn save(&self, path: &Path) -> Result<(), PrefsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PrefsError::Io(e.to_string()))?;
        }
        let raw =
            serde_json::to_string_pretty(self).map_err(|e| PrefsError::Invalid(e.to_string()))?;
        std::fs::write(path, raw).map_err(|e| PrefsError::Io(e.to_string()))?;
        Ok(())
    }

    /// Add a favorite (no-op if already present).
    pub fn add_favorite(&mut self, provider: ProviderId, model: ModelId) {
        let entry = FavoriteEntry { provider, model };
        if !self.favorites.contains(&entry) {
            self.favorites.push(entry);
        }
    }

    /// Remove a favorite.
    pub fn remove_favorite(&mut self, provider: &ProviderId, model: &ModelId) {
        self.favorites
            .retain(|e| &e.provider != provider || &e.model != model);
    }

    /// True if the (provider, model) is a favorite.
    pub fn is_favorite(&self, provider: &ProviderId, model: &ModelId) -> bool {
        self.favorites
            .iter()
            .any(|e| &e.provider == provider && &e.model == model)
    }

    /// Set the user's default model.
    pub fn set_default(&mut self, provider: ProviderId, model: ModelId) {
        self.default = Some(FavoriteEntry { provider, model });
    }

    /// Clear the user's default model.
    pub fn clear_default(&mut self) {
        self.default = None;
    }

    /// The user's default model, if set.
    pub fn default_model(&self) -> Option<&FavoriteEntry> {
        self.default.as_ref()
    }

    /// Push a recent selection. De-duplicates and caps at 10.
    pub fn push_recent(&mut self, provider: ProviderId, model: ModelId) {
        self.recents
            .retain(|e| e.provider != provider || e.model != model);
        self.recents.insert(0, FavoriteEntry { provider, model });
        if self.recents.len() > 10 {
            self.recents.truncate(10);
        }
    }

    /// Return a HashSet view of the favorites for compatibility
    /// with `tui_picker::PickerState`.
    pub fn favorites_set(&self) -> HashSet<(ProviderId, ModelId)> {
        self.favorites
            .iter()
            .map(|e| (e.provider.clone(), e.model.clone()))
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum PrefsError {
    #[error("io error: {0}")]
    Io(String),
    #[error("invalid prefs file: {0}")]
    Invalid(String),
}

/// Default path: `~/.jcode/model_prefs.json`.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".jcode").join("model_prefs.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_when_file_missing() {
        let p = std::env::temp_dir().join("jcode-prefs-missing.json");
        let _ = std::fs::remove_file(&p);
        let prefs = ModelPrefs::load(&p).unwrap();
        assert!(prefs.favorites.is_empty());
        assert!(prefs.recents.is_empty());
    }

    #[test]
    fn round_trip() {
        let p = std::env::temp_dir().join("jcode-prefs-rt.json");
        let _ = std::fs::remove_file(&p);
        let mut prefs = ModelPrefs::new();
        prefs.add_favorite("anthropic".into(), "claude-haiku-4-5".into());
        prefs.add_favorite("openai".into(), "gpt-5-mini".into());
        prefs.push_recent("openai".into(), "gpt-5-mini".into());
        prefs.save(&p).unwrap();
        let loaded = ModelPrefs::load(&p).unwrap();
        assert_eq!(loaded, prefs);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn add_favorite_dedupes() {
        let mut prefs = ModelPrefs::new();
        prefs.add_favorite("anthropic".into(), "claude-haiku-4-5".into());
        prefs.add_favorite("anthropic".into(), "claude-haiku-4-5".into());
        assert_eq!(prefs.favorites.len(), 1);
    }

    #[test]
    fn remove_favorite_works() {
        let mut prefs = ModelPrefs::new();
        prefs.add_favorite("anthropic".into(), "claude-haiku-4-5".into());
        prefs.remove_favorite(&"anthropic".into(), &"claude-haiku-4-5".into());
        assert!(prefs.favorites.is_empty());
    }

    #[test]
    fn is_favorite() {
        let mut prefs = ModelPrefs::new();
        prefs.add_favorite("anthropic".into(), "claude-haiku-4-5".into());
        assert!(prefs.is_favorite(&"anthropic".into(), &"claude-haiku-4-5".into()));
        assert!(!prefs.is_favorite(&"openai".into(), &"gpt-5-mini".into()));
    }

    #[test]
    fn push_recent_dedupes_and_caps() {
        let mut prefs = ModelPrefs::new();
        for _ in 0..15 {
            prefs.push_recent("anthropic".into(), "claude-haiku-4-5".into());
        }
        assert_eq!(prefs.recents.len(), 1, "deduped");
        for i in 0..15 {
            prefs.push_recent("a".into(), format!("m{i}").as_str().into());
        }
        assert!(prefs.recents.len() <= 10, "capped");
    }

    #[test]
    fn favorites_set_returns_keys() {
        let mut prefs = ModelPrefs::new();
        prefs.add_favorite("anthropic".into(), "claude-haiku-4-5".into());
        let set = prefs.favorites_set();
        assert_eq!(set.len(), 1);
        assert!(set.contains(&("anthropic".into(), "claude-haiku-4-5".into())));
    }

    #[test]
    fn round_trip_with_default() {
        let p = std::env::temp_dir().join("jcode-prefs-default-rt.json");
        let _ = std::fs::remove_file(&p);
        let mut prefs = ModelPrefs::new();
        prefs.set_default("anthropic".into(), "claude-haiku-4-5".into());
        prefs.save(&p).unwrap();
        let loaded = ModelPrefs::load(&p).unwrap();
        assert_eq!(
            loaded.default_model().unwrap().provider.as_str(),
            "anthropic"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn clear_default_removes_field() {
        let mut prefs = ModelPrefs::new();
        prefs.set_default("anthropic".into(), "claude-haiku-4-5".into());
        assert!(prefs.default_model().is_some());
        prefs.clear_default();
        assert!(prefs.default_model().is_none());
    }

    #[test]
    fn invalid_file_surfaces_error() {
        let p = std::env::temp_dir().join("jcode-prefs-bad.json");
        std::fs::write(&p, "not json").unwrap();
        let err = ModelPrefs::load(&p).unwrap_err();
        assert!(matches!(err, PrefsError::Invalid(_)));
        let _ = std::fs::remove_file(&p);
    }
}
