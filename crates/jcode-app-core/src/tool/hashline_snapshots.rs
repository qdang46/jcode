//! Global SnapshotStore for hashline file version tracking.
//!
//! Wraps the `hashline::snapshot_store::InMemorySnapshotStore` in an `Arc<RwLock>`
//! so the store can be shared across all tool invocations within a process.
//!
//! The store is the foundation for drift detection and recovery. When `read`
//! records a snapshot, the resulting `#TAG` can be quoted in a `hashline_edit`
//! patch input; the patcher then verifies the file has not changed before
//! applying, and can recover via 3-way merge if drift is detected.
//!
//! ponytail: limits are hardcoded. Make configurable via ToolConfig when the
//! hashline integration stabilizes.

use hashline::snapshot_store::{InMemorySnapshotStore, Snapshot, SnapshotStore};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

/// Default limits, matching the reference hashline store.
const DEFAULT_MAX_PATHS: usize = 30;
const DEFAULT_MAX_VERSIONS_PER_PATH: usize = 4;
const DEFAULT_MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// Process-global snapshot store. Initialized lazily on first access.
static STORE: OnceLock<Arc<RwLock<InMemorySnapshotStore>>> = OnceLock::new();

/// Get the global store, initializing it on first call.
pub fn global() -> Arc<RwLock<InMemorySnapshotStore>> {
    STORE
        .get_or_init(|| {
            Arc::new(RwLock::new(InMemorySnapshotStore::with_options(
                DEFAULT_MAX_PATHS,
                DEFAULT_MAX_VERSIONS_PER_PATH,
                DEFAULT_MAX_TOTAL_BYTES,
            )))
        })
        .clone()
}

/// Record a snapshot for `path` with `text`. Returns the 4-hex content tag.
///
/// If `seen_lines` is provided, those line numbers (1-indexed) are merged into
/// the snapshot's seen-lines set so future edits can reject anchors on lines
/// the producer never displayed.
pub fn record(path: &Path, text: &str, seen_lines: Option<&[usize]>) -> String {
    let canonical = canonicalize(path);
    let store = global();
    let mut guard = store.blocking_write();
    guard.record(&canonical, text, seen_lines)
}

/// Look up the most recent snapshot for `path`.
pub fn head(path: &Path) -> Option<Snapshot> {
    let canonical = canonicalize(path);
    let store = global();
    let guard = store.blocking_read();
    guard.head(&canonical)
}

/// Look up a specific snapshot version by tag.
pub fn by_hash(path: &Path, hash: &str) -> Option<Snapshot> {
    let canonical = canonicalize(path);
    let store = global();
    let guard = store.blocking_read();
    guard.by_hash(&canonical, hash)
}

/// Merge `lines` into the seen-lines set of the snapshot identified by `hash`.
pub fn record_seen_lines(path: &Path, hash: &str, lines: &[usize]) {
    let canonical = canonicalize(path);
    let store = global();
    let mut guard = store.blocking_write();
    guard.record_seen_lines(&canonical, hash, lines);
}

/// Drop all version history for a single path.
pub fn invalidate(path: &Path) {
    let canonical = canonicalize(path);
    let store = global();
    let mut guard = store.blocking_write();
    guard.invalidate(&canonical);
}

/// Drop all snapshot history (used on session reset).
pub fn clear() {
    let store = global();
    let mut guard = store.blocking_write();
    guard.clear();
}

/// Best-effort canonical path for snapshot keying. Falls back to the input
/// path if canonicalization fails (e.g. file does not exist yet).
fn canonicalize(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Returns true if every line in `required` is contained in `seen`.
pub fn lines_were_seen(seen: Option<&HashSet<usize>>, required: &[usize]) -> bool {
    let Some(seen) = seen else {
        return true;
    };
    required.iter().all(|line| seen.contains(line))
}

/// Format a hashline file header: `[path#TAG]`.
pub fn format_header(path: &Path, tag: &str) -> String {
    format!(
        "[{}#{}]",
        path.file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default(),
        tag
    )
}

/// Compute the 4-hex file content tag using the hashline crate's standard
/// algorithm. This is the tag the model sees in the `[path#TAG]` header.
pub fn compute_file_tag(text: &str) -> String {
    hashline::hash::compute_file_hash(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_returns_consistent_tag() {
        let path = Path::new("/tmp/jcode-snap-test-a.txt");
        let tag1 = record(path, "hello\nworld\n", None);
        assert_eq!(tag1.len(), 4);
        let tag2 = record(path, "hello\nworld\n", None);
        assert_eq!(tag1, tag2);
    }

    #[test]
    fn head_returns_most_recent() {
        let path = Path::new("/tmp/jcode-snap-test-b.txt");
        let t1 = record(path, "v1", None);
        let t2 = record(path, "v2", None);
        assert_ne!(t1, t2);
        let snap = head(path).expect("head should exist");
        assert_eq!(snap.hash, t2);
    }

    #[test]
    fn by_hash_finds_historical_version() {
        let path = Path::new("/tmp/jcode-snap-test-c.txt");
        let t1 = record(path, "alpha", None);
        let _t2 = record(path, "beta", None);
        let snap = by_hash(path, &t1).expect("old version should be retained");
        assert_eq!(snap.text, "alpha");
    }

    #[test]
    fn seen_lines_propagate() {
        let path = Path::new("/tmp/jcode-snap-test-d.txt");
        let tag = record(path, "l1\nl2\nl3\n", Some(&[1, 2]));
        record_seen_lines(path, &tag, &[3]);
        let snap = head(path).expect("head");
        let seen = snap.seen_lines.expect("seen_lines should be set");
        assert!(seen.contains(&1));
        assert!(seen.contains(&2));
        assert!(seen.contains(&3));
    }

    #[test]
    fn lines_were_seen_handles_missing_provenance() {
        assert!(lines_were_seen(None, &[1, 2, 3]));
        let mut set = HashSet::new();
        set.insert(2);
        assert!(lines_were_seen(Some(&set), &[2]));
        assert!(!lines_were_seen(Some(&set), &[1]));
    }

    #[test]
    fn format_header_creates_tagged_path() {
        let h = format_header(Path::new("src/main.rs"), "A3B2");
        assert_eq!(h, "[src/main.rs#A3B2]");
    }

    #[test]
    fn compute_file_tag_is_4_hex() {
        let tag = compute_file_tag("anything");
        assert_eq!(tag.len(), 4);
        assert!(tag.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
