use crate::types::{FileDiff, RunId};
use dashmap::DashMap;
use std::sync::Arc;

/// In-memory overlay store for proposed file content.
///
/// Modeled after codebuff's `proposed-content-store.ts`:
/// a Map<runId, Record<path, content>> that holds draft file content
/// before any candidate is selected. The orchestrator writes proposed
/// content here, the selector reads from here, and the winner's content
/// is applied to real files.
///
/// This store uses DashMap for concurrent access since multiple
/// candidate agents may write proposals in parallel.
#[derive(Debug, Clone)]
pub struct ProposedContentStore {
    /// Nested map: RunId -> (file_path -> proposed_content).
    /// The inner DashMap allows concurrent writes to different files
    /// within the same run.
    inner: Arc<DashMap<String, DashMap<String, ProposedEntry>>>,
}

/// A proposed file entry with tracking metadata.
#[derive(Debug, Clone)]
pub struct ProposedEntry {
    /// Proposed content of the file.
    pub content: String,
    /// The candidate ID that proposed this content.
    pub candidate_id: String,
    /// Whether this is a new file (write) vs an edit.
    pub is_new_file: bool,
}

impl ProposedContentStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Set proposed content for a file within a run.
    pub fn set_proposed(
        &self,
        run_id: &RunId,
        file_path: impl Into<String>,
        content: impl Into<String>,
        candidate_id: impl Into<String>,
        is_new_file: bool,
    ) {
        let run_map = self.inner.entry(run_id.to_string()).or_default();
        run_map.insert(
            file_path.into(),
            ProposedEntry {
                content: content.into(),
                candidate_id: candidate_id.into(),
                is_new_file,
            },
        );
    }

    /// Get proposed content for a specific file within a run.
    pub fn get_proposed(&self, run_id: &RunId, file_path: &str) -> Option<ProposedEntry> {
        self.inner
            .get(run_id.to_string().as_str())
            .and_then(|run_map| run_map.get(file_path).map(|entry| entry.clone()))
    }

    /// Check if a file has proposed content in a run.
    pub fn has_proposed(&self, run_id: &RunId, file_path: &str) -> bool {
        self.inner
            .get(run_id.to_string().as_str())
            .is_some_and(|run_map| run_map.contains_key(file_path))
    }

    /// Get all proposed file paths for a run.
    pub fn get_proposed_paths(&self, run_id: &RunId) -> Vec<String> {
        self.inner
            .get(run_id.to_string().as_str())
            .map(|run_map| run_map.iter().map(|entry| entry.key().clone()).collect())
            .unwrap_or_default()
    }

    /// Get all proposed entries for a run.
    pub fn get_all_proposed(&self, run_id: &RunId) -> Vec<(String, ProposedEntry)> {
        self.inner
            .get(run_id.to_string().as_str())
            .map(|run_map| {
                run_map
                    .iter()
                    .map(|entry| (entry.key().clone(), entry.value().clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Build FileDiff entries from proposed content for a run, comparing
    /// against the original file contents provided.
    pub fn build_diffs(
        &self,
        run_id: &RunId,
        candidate_id: impl Into<String>,
        original_contents: &[OriginalFileEntry],
    ) -> Vec<FileDiff> {
        let candidate_id = candidate_id.into();
        let mut diffs = Vec::new();

        let Some(run_map) = self.inner.get(run_id.to_string().as_str()) else {
            return diffs;
        };

        for entry in run_map.iter() {
            if entry.value().candidate_id != candidate_id {
                continue;
            }

            let file_path = entry.key().clone();
            let proposed = entry.value();
            let original = original_contents.iter().find(|o| o.file_path == file_path);

            let old_content = original.map(|o| o.content.clone()).unwrap_or_default();
            let new_content = proposed.content.clone();

            let unified_diff = make_unified_diff(&file_path, &old_content, &new_content);

            if old_content == new_content {
                continue;
            }

            diffs.push(FileDiff {
                file_path,
                unified_diff,
                old_content,
                new_content,
                is_new_file: proposed.is_new_file,
            });
        }

        diffs
    }

    /// Clear all proposed content for a run.
    pub fn clear_run(&self, run_id: &RunId) {
        self.inner.remove(run_id.to_string().as_str());
    }

    /// Clear all runs from the store.
    pub fn clear_all(&self) {
        self.inner.clear();
    }

    /// Number of active runs in the store.
    pub fn run_count(&self) -> usize {
        self.inner.len()
    }

    /// Check if a run exists.
    pub fn has_run(&self, run_id: &RunId) -> bool {
        self.inner.contains_key(run_id.to_string().as_str())
    }
}

impl Default for ProposedContentStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Original file content used for diff computation.
#[derive(Debug, Clone)]
pub struct OriginalFileEntry {
    pub file_path: String,
    pub content: String,
}

/// Generate a unified diff between old and new content.
/// Uses the `similar` crate's diffing algorithm.
fn make_unified_diff(file_path: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }

    let diff = similar::TextDiff::from_lines(old, new);
    let mut result = format!("--- a/{}\n+++ b/{}\n", file_path, file_path);

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            similar::ChangeTag::Delete => "-",
            similar::ChangeTag::Insert => "+",
            similar::ChangeTag::Equal => " ",
        };
        result.push_str(&format!("{}{}", sign, change.value()));
    }

    // Remove trailing newline if content doesn't end with one
    if !result.ends_with('\n') {
        result.push('\n');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_get_proposed() {
        let store = ProposedContentStore::new();
        let run_id = RunId::new();

        store.set_proposed(&run_id, "src/main.rs", "content v1", "candidate-0", false);
        let entry = store.get_proposed(&run_id, "src/main.rs");

        assert!(entry.is_some());
        assert_eq!(entry.unwrap().content, "content v1");
    }

    #[test]
    fn test_has_proposed() {
        let store = ProposedContentStore::new();
        let run_id = RunId::new();

        assert!(!store.has_proposed(&run_id, "src/main.rs"));
        store.set_proposed(&run_id, "src/main.rs", "content", "candidate-0", false);
        assert!(store.has_proposed(&run_id, "src/main.rs"));
    }

    #[test]
    fn test_clear_run() {
        let store = ProposedContentStore::new();
        let run_id = RunId::new();

        store.set_proposed(&run_id, "src/main.rs", "content", "candidate-0", false);
        assert!(store.has_run(&run_id));

        store.clear_run(&run_id);
        assert!(!store.has_run(&run_id));
    }

    #[test]
    fn test_build_diffs() {
        let store = ProposedContentStore::new();
        let run_id = RunId::new();

        store.set_proposed(&run_id, "src/main.rs", "hello world", "candidate-0", false);

        let original = vec![OriginalFileEntry {
            file_path: "src/main.rs".to_string(),
            content: "hello".to_string(),
        }];

        let diffs = store.build_diffs(&run_id, "candidate-0", &original);

        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].has_changes());
        assert!(diffs[0].unified_diff.contains("hello world"));
    }

    #[test]
    fn test_no_changes_diff() {
        let store = ProposedContentStore::new();
        let run_id = RunId::new();

        store.set_proposed(&run_id, "src/main.rs", "same content", "candidate-0", false);

        let original = vec![OriginalFileEntry {
            file_path: "src/main.rs".to_string(),
            content: "same content".to_string(),
        }];

        let diffs = store.build_diffs(&run_id, "candidate-0", &original);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_get_proposed_paths() {
        let store = ProposedContentStore::new();
        let run_id = RunId::new();

        store.set_proposed(&run_id, "a.rs", "a", "candidate-0", false);
        store.set_proposed(&run_id, "b.rs", "b", "candidate-0", false);

        let mut paths = store.get_proposed_paths(&run_id);
        paths.sort();
        assert_eq!(paths, vec!["a.rs", "b.rs"]);
    }
}
