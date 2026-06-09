//! Auto-Dream — background consolidation using the forked agent pattern.
//!
//! The auto-dream system runs a background forked agent periodically to
//! consolidate session context, extract insights, and update project-level
//! knowledge. Unlike memory extraction (which saves specific facts), dreaming
//! is about synthesis — connecting patterns across turns, identifying trends,
//! and building higher-level models.
//!
//! ## Schedule
//! Runs every `turn_interval` turns. Configurable in `config.toml`.
//!
//! ## Tool Restrictions
//! Same read-only restrictions as memory extraction, but write access is
//! broader (multiple allowed directories).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the auto-dream background consolidation.
///
/// Dream output is stored as files in the specified dream directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutoDreamConfig {
    /// Enable auto-dream.
    pub enabled: bool,

    /// Run dream every N turns.
    pub turn_interval: usize,

    /// Max turns for the dream agent.
    pub max_turns: u32,

    /// Max output tokens per turn.
    pub max_output_tokens: u32,

    /// Directories where the dream agent is allowed to write.
    pub allowed_dirs: Vec<PathBuf>,

    /// Dream output directory.
    pub dream_dir: PathBuf,
}

impl Default for AutoDreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            turn_interval: 10,
            max_turns: 2,
            max_output_tokens: 2048,
            allowed_dirs: vec![PathBuf::from(".jcode/dreams")],
            dream_dir: PathBuf::from(".jcode/dreams"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_dream_defaults() {
        let config = AutoDreamConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.turn_interval, 10);
        assert_eq!(config.max_turns, 2);
        assert_eq!(config.max_output_tokens, 2048);
        assert_eq!(config.allowed_dirs, vec![PathBuf::from(".jcode/dreams")]);
    }

    #[test]
    fn test_auto_dream_custom_values() {
        let config = AutoDreamConfig {
            enabled: true,
            turn_interval: 20,
            max_turns: 3,
            max_output_tokens: 4096,
            allowed_dirs: vec![
                PathBuf::from(".jcode/dreams"),
                PathBuf::from(".jcode/memory"),
            ],
            dream_dir: PathBuf::from(".jcode/dreams"),
        };
        assert!(config.enabled);
        assert_eq!(config.turn_interval, 20);
        assert_eq!(config.allowed_dirs.len(), 2);
    }
}
