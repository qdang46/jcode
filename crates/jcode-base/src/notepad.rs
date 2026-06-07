//! 3-tier file-based notepad for compaction-resistant context notes.
//!
//! The notepad stores short text notes across three tiers, each backed by
//! a plain markdown file in `<working_dir>/.jcode/notepad/`:
//!
//! - **Priority** – critical context that is injected into the system prompt
//!   every turn, surviving compaction. The model uses it as always-present
//!   context (current goal, key constraints, pinned decisions).
//! - **Working** – scratchpad for the current session. Cleared between
//!   sessions. Not injected automatically.
//! - **Manual** – user-authored notes that persist across sessions. Not
//!   injected automatically.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Tiers
// ---------------------------------------------------------------------------

/// The three notepad tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotepadTier {
    /// Injected into the system prompt every turn.
    Priority,
    /// Session-scoped scratchpad.
    Working,
    /// Persistent user notes.
    Manual,
}

impl NotepadTier {
    /// Human-readable label for the tier.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Priority => "priority",
            Self::Working => "working",
            Self::Manual => "manual",
        }
    }

    /// The file name used on disk (e.g. `priority.md`).
    pub fn filename(&self) -> &'static str {
        match self {
            Self::Priority => "priority.md",
            Self::Working => "working.md",
            Self::Manual => "manual.md",
        }
    }

    /// All tiers for iteration.
    pub fn all() -> &'static [NotepadTier] {
        &[Self::Priority, Self::Working, Self::Manual]
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Notepad subsection of the main [`Config`](crate::config::Config).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotepadConfig {
    /// Whether the notepad feature is enabled (default: `true`).
    pub enabled: bool,

    /// Directory for notepad files, relative to the working directory
    /// (default: `.jcode/notepad`).
    pub dir: String,

    /// Maximum characters for a single tier's content (default: 4096).
    pub max_chars_per_tier: usize,
}

impl Default for NotepadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: ".jcode/notepad".to_string(),
            max_chars_per_tier: 4096,
        }
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// The notepad engine — reads/writes/clears tiered note files on disk.
pub struct Notepad {
    base_dir: PathBuf,
    enabled: bool,
    max_chars_per_tier: usize,
}

impl Notepad {
    /// Create a new `Notepad` if enabled, or returns `None`.
    ///
    /// `working_dir` is the session's working directory; the note files are
    /// placed under `<working_dir>/<config.dir>`.
    pub fn new(working_dir: Option<&Path>, config: &NotepadConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }
        let base = working_dir
            .map(|wd| wd.join(&config.dir))
            .unwrap_or_else(|| PathBuf::from(&config.dir));
        // Ensure the directory exists — best-effort.
        let _ = std::fs::create_dir_all(&base);
        Some(Self {
            base_dir: base,
            enabled: true,
            max_chars_per_tier: config.max_chars_per_tier,
        })
    }

    // -- helpers -----------------------------------------------------------

    fn tier_path(&self, tier: NotepadTier) -> PathBuf {
        self.base_dir.join(tier.filename())
    }

    // -- public API --------------------------------------------------------

    /// Read the content of a tier. Returns an empty string if the file does
    /// not exist or cannot be read.
    pub fn read(&self, tier: NotepadTier) -> String {
        if !self.enabled {
            return String::new();
        }
        std::fs::read_to_string(self.tier_path(tier)).unwrap_or_default()
    }

    /// Write `content` to a tier, truncating if `max_chars_per_tier` is
    /// exceeded. Returns an error if the write fails.
    pub fn write(&self, tier: NotepadTier, content: &str) -> std::io::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let path = self.tier_path(tier);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let truncated = if content.len() > self.max_chars_per_tier {
            &content[..content.floor_char_boundary(self.max_chars_per_tier)]
        } else {
            content
        };
        std::fs::write(&path, truncated)
    }

    /// Clear a tier's content (write an empty string).
    pub fn clear(&self, tier: NotepadTier) -> std::io::Result<()> {
        self.write(tier, "")
    }

    /// Read the priority tier and format it as a prompt block suitable for
    /// system-prompt injection. Returns `None` when the tier is empty or
    /// the notepad is disabled.
    pub fn priority_prompt_block(&self) -> Option<String> {
        let content = self.read(NotepadTier::Priority);
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(format!("# Priority Notes\n\n{}", trimmed))
    }

    /// The resolved base directory for notepad files.
    pub fn dir(&self) -> &Path {
        &self.base_dir
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_notepad() -> (tempfile::TempDir, Notepad) {
        let dir = tempfile::tempdir().unwrap();
        let config = NotepadConfig {
            enabled: true,
            dir: ".notepad".to_string(),
            max_chars_per_tier: 4096,
        };
        let notepad = Notepad::new(Some(dir.path()), &config).unwrap();
        (dir, notepad)
    }

    #[test]
    fn test_read_write_roundtrip() {
        let (_dir, np) = temp_notepad();
        np.write(NotepadTier::Priority, "hello world").unwrap();
        assert_eq!(np.read(NotepadTier::Priority), "hello world");
    }

    #[test]
    fn test_clear() {
        let (_dir, np) = temp_notepad();
        np.write(NotepadTier::Working, "data").unwrap();
        np.clear(NotepadTier::Working).unwrap();
        assert_eq!(np.read(NotepadTier::Working), "");
    }

    #[test]
    fn test_read_nonexistent_returns_empty() {
        let (_dir, np) = temp_notepad();
        assert_eq!(np.read(NotepadTier::Manual), "");
    }

    #[test]
    fn test_priority_prompt_block_returns_none_when_empty() {
        let (_dir, np) = temp_notepad();
        assert!(np.priority_prompt_block().is_none());
    }

    #[test]
    fn test_priority_prompt_block_formats_content() {
        let (_dir, np) = temp_notepad();
        np.write(NotepadTier::Priority, "Keep this in mind").unwrap();
        let block = np.priority_prompt_block().unwrap();
        assert!(block.contains("Priority Notes"));
        assert!(block.contains("Keep this in mind"));
    }

    #[test]
    fn test_disabled_notepad_returns_none() {
        let config = NotepadConfig {
            enabled: false,
            ..Default::default()
        };
        let np = Notepad::new(None, &config);
        assert!(np.is_none());
    }

    #[test]
    fn test_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let config = NotepadConfig {
            enabled: true,
            dir: ".notepad".to_string(),
            max_chars_per_tier: 10,
        };
        let np = Notepad::new(Some(dir.path()), &config).unwrap();
        np.write(NotepadTier::Priority, "this is way too long for the limit")
            .unwrap();
        let content = np.read(NotepadTier::Priority);
        assert!(content.len() <= 10);
    }

    #[test]
    fn test_tier_as_str() {
        assert_eq!(NotepadTier::Priority.as_str(), "priority");
        assert_eq!(NotepadTier::Working.as_str(), "working");
        assert_eq!(NotepadTier::Manual.as_str(), "manual");
    }

    #[test]
    fn test_tier_filename() {
        assert_eq!(NotepadTier::Priority.filename(), "priority.md");
        assert_eq!(NotepadTier::Working.filename(), "working.md");
        assert_eq!(NotepadTier::Manual.filename(), "manual.md");
    }

    #[test]
    fn test_all_tiers() {
        let tiers = NotepadTier::all();
        assert_eq!(tiers.len(), 3);
        assert!(tiers.contains(&NotepadTier::Priority));
        assert!(tiers.contains(&NotepadTier::Working));
        assert!(tiers.contains(&NotepadTier::Manual));
    }
}
