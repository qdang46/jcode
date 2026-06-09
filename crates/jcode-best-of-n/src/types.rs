use serde::{Deserialize, Serialize};

/// Unique identifier for a best-of-N run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub String);

impl RunId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for RunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a single candidate within a run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CandidateId(pub String);

impl CandidateId {
    pub fn new(index: usize) -> Self {
        Self(format!("candidate-{index}"))
    }
}

impl std::fmt::Display for CandidateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a candidate after execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateStatus {
    /// Candidate completed successfully with meaningful changes.
    Success,
    /// Candidate completed but produced no changes (ghost).
    NoChanges,
    /// Candidate failed during execution.
    Failed,
}

/// The strategy used for a candidate (how it was prompted/configured).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateStrategy {
    /// Human-readable label (e.g. "temperature-0.2", "precise").
    pub label: String,
    /// Temperature used for this candidate.
    pub temperature: f64,
    /// Optional model override (None = use orchestrator's model).
    pub model: Option<String>,
}

impl Default for CandidateStrategy {
    fn default() -> Self {
        Self {
            label: "default".to_string(),
            temperature: 0.5,
            model: None,
        }
    }
}

/// A diff for a single file produced by a candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// Path of the changed file.
    pub file_path: String,
    /// Unified diff text (the actual diff output).
    pub unified_diff: String,
    /// Old content (before edit).
    pub old_content: String,
    /// New content (after edit).
    pub new_content: String,
    /// Whether this is a new file (write) vs edit.
    pub is_new_file: bool,
}

impl FileDiff {
    /// Returns true if this diff changes the file content.
    pub fn has_changes(&self) -> bool {
        self.old_content != self.new_content
    }
}

/// The output produced by a single candidate implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateDiff {
    /// Unique ID for this candidate.
    pub candidate_id: CandidateId,
    /// Strategy used.
    pub strategy: CandidateStrategy,
    /// Status of execution.
    pub status: CandidateStatus,
    /// Per-file diffs produced by this candidate.
    pub file_diffs: Vec<FileDiff>,
    /// Total number of tokens consumed by this candidate (if available).
    pub total_tokens: Option<u64>,
    /// Error message if status is Failed.
    pub error: Option<String>,
}

impl CandidateDiff {
    /// Returns true if the candidate produced any actual content changes.
    pub fn has_meaningful_changes(&self) -> bool {
        self.status == CandidateStatus::Success && self.file_diffs.iter().any(|d| d.has_changes())
    }

    /// Number of files touched (with changes).
    pub fn changed_file_count(&self) -> usize {
        self.file_diffs.iter().filter(|d| d.has_changes()).count()
    }

    /// Total number of edit operations across all files.
    pub fn total_ops(&self) -> usize {
        self.file_diffs.len()
    }
}

/// Scoring metrics for a candidate used by the deterministic selector.
#[derive(Debug, Clone)]
pub struct SelectScore {
    /// Candidate this score belongs to.
    pub candidate_id: CandidateId,
    /// Primary: succeeded (true) or failed (false).
    pub success: bool,
    /// Secondary: has non-ghost changes.
    pub has_changes: bool,
    /// Tertiary: fewer files changed = better (focus).
    pub file_count: usize,
    /// Quaternary: fewer ops = better.
    pub op_count: usize,
    /// Quinary: lower token cost = better.
    pub tokens: u64,
    /// Final tiebreaker: deterministic.
    pub index: usize,
}

/// Final result of a best-of-N run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestOfNResult {
    /// The run ID.
    pub run_id: RunId,
    /// All candidates and their diffs.
    pub candidates: Vec<CandidateDiff>,
    /// Index (in candidates) of the selected winner.
    pub winner_index: Option<usize>,
    /// The winner's diff (shortcut).
    pub winner: Option<CandidateDiff>,
    /// Selection reason (for Display mode).
    pub selection_reason: Option<String>,
}

/// A proposed edit operation (before application).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeOp {
    /// The proposed op kind.
    pub kind: ProposeOpKind,
    /// File path.
    pub file_path: String,
    /// For Edit: old string to replace. For Write: empty/none.
    pub old_string: Option<String>,
    /// The new content / replacement.
    pub new_string: String,
    /// Anchor metadata for hashline edit (only used when kind is HashlineEdit).
    pub anchor_line: Option<usize>,
    pub anchor_hash_sha256: Option<String>,
    pub anchor_context_window: Option<usize>,
}

/// Kind of proposed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposeOpKind {
    /// str_replace style edit.
    Edit,
    /// Write entire file.
    Write,
    /// Hashline-anchored edit (old_string + new_string within verified window).
    HashlineEdit,
}

/// Anchor metadata for a hashline-anchored edit proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashlineAnchor {
    pub file_path: String,
    pub line: usize,
    pub hash_sha256: String,
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    pub old_string: String,
    pub new_string: String,
}

fn default_context_window() -> usize {
    0
}

impl ProposeOp {
    pub fn edit(
        file_path: impl Into<String>,
        old_string: impl Into<String>,
        new_string: impl Into<String>,
    ) -> Self {
        Self {
            kind: ProposeOpKind::Edit,
            file_path: file_path.into(),
            old_string: Some(old_string.into()),
            new_string: new_string.into(),
            anchor_line: None,
            anchor_hash_sha256: None,
            anchor_context_window: None,
        }
    }

    pub fn write(file_path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            kind: ProposeOpKind::Write,
            file_path: file_path.into(),
            old_string: None,
            new_string: content.into(),
            anchor_line: None,
            anchor_hash_sha256: None,
            anchor_context_window: None,
        }
    }

    pub fn hashline_edit(
        file_path: impl Into<String>,
        anchor: HashlineAnchor,
        old_string: impl Into<String>,
        new_string: impl Into<String>,
    ) -> Self {
        Self {
            kind: ProposeOpKind::HashlineEdit,
            file_path: file_path.into(),
            old_string: Some(old_string.into()),
            new_string: new_string.into(),
            anchor_line: Some(anchor.line),
            anchor_hash_sha256: Some(anchor.hash_sha256),
            anchor_context_window: Some(anchor.context_window),
        }
    }
}
