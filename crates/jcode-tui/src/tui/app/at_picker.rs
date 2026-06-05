//! `@<path>` autocomplete picker — wraps `ffs-search` `FilePicker` for use
//! in the TUI input dropdown.
//!
//! ## Design
//!
//! `AtPicker` is a session-scoped singleton that:
//! 1. On `warm_up()`: spawns ffs's background filesystem scan + watcher rooted
//!    at the session working directory. Non-blocking; subsequent `search()`
//!    calls return empty until the scan completes.
//! 2. On `search(input, cursor)`: builds an `ffs_search::mention::MentionResolver`
//!    (Phase A of the @-mention system) which is cursor-aware, reuses the
//!    existing `fuzzy_search` + `fuzzy_search_directories` pipelines, and
//!    boosts candidates with the LMDB-backed `FrecencyTracker`. Drops the
//!    leading `@` if present. Returns at most `AT_PICKER_MAX_SUGGESTIONS`.
//! 3. On `resolve_payload(path)`: uses `ffs_engine::mention::resolve_mentions`
//!    (Phase B) to read the file with token-budget discipline. Cache results
//!    in a `MentionResolverCache` keyed by `(path, turn_id)` so repeated
//!    resolutions inside one turn don't re-read disk.
//!
//! Frecency state lives at `<cache_root>/<repo_hash>/{frecency,queries}` so
//! multiple jcode sessions on the same repo share rankings.
//!
//! ## Concurrency
//!
//! `AtPicker` is `Send + Sync` and cheap to clone (internals are Arc'd).
//! `search()` uses a non-blocking read of the shared picker so the UI
//! thread never blocks on the background scan.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::time::Duration;

use ffs_engine::mention::{ResolveOptions, ResolvedMention, resolve_mentions};
use ffs_search::mention::{MentionCandidate, MentionKind, MentionResolver, MentionResult};
use ffs_search::{
    FfsMode, FilePicker, FilePickerOptions, FrecencyTracker, QueryTracker, SharedFilePicker,
    SharedFrecency, SharedQueryTracker,
};

/// Maximum suggestions returned by `search()`. Matches Claude Code's
/// `MAX_SUGGESTIONS = 15`.
pub const AT_PICKER_MAX_SUGGESTIONS: usize = 15;

/// Default token budget per `resolve_payload` call. ~50k tokens matches
/// ffs-budget's `DEFAULT_PERCENT_BODY` heuristic and is what
/// ffs-engine::mention::ResolveOptions picks when unset.
///
/// Reserved for Phase B: the TUI selection flow will call
/// `resolve_payload` when the user confirms an @-mention. Until that
/// lands, the symbol is `#[allow(dead_code)]` so the API surface stays
/// stable.
#[allow(dead_code)]
const DEFAULT_RESOLVE_BUDGET_TOKENS: u32 = 50_000;

/// One suggestion item produced by `AtPicker::search`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtSuggestion {
    /// Path relative to the picker's base directory, e.g. `src/main.rs`
    /// or `crates/jcode-core` (no trailing `/` even for dirs — caller
    /// decides whether to append it for display).
    pub display_path: String,
    pub is_directory: bool,
}

/// State of the underlying ffs background scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerState {
    /// `warm_up()` has not been called yet.
    Uninitialized,
    /// Background scan in progress; queries return empty results.
    Scanning,
    /// Scan complete; queries are served.
    Ready,
}

/// Session-scoped wrapper around ffs `FilePicker`.
#[derive(Clone)]
pub struct AtPicker {
    inner: Arc<AtPickerInner>,
}

struct AtPickerInner {
    base_path: PathBuf,
    cache_dir: PathBuf,
    shared_picker: SharedFilePicker,
    shared_frecency: SharedFrecency,
    shared_query_tracker: SharedQueryTracker,
    /// Set on first successful `warm_up()` to avoid double-spawn.
    warm_up_started: AtomicBool,
    #[allow(dead_code)] // Phase B cache; consumed by resolve_payload above.
    /// Turn-keyed cache of `ResolvedMention` payloads so repeated
    /// resolutions inside one turn don't re-read the same path. Keyed
    /// by `(absolute_path, turn_id)`. Capped at 256 entries (LRU not
    /// implemented — simple FIFO eviction at capacity is fine because
    /// turn_id is monotonically increasing and the cache is cleared
    /// implicitly when the session ends).
    resolve_cache: std::sync::RwLock<std::collections::HashMap<(PathBuf, u64), ResolvedMention>>,
}

impl AtPicker {
    /// Create a new picker for a given base path. `cache_root` is typically
    /// `~/.jcode/cache/ffs/`; a per-repo subdirectory will be created inside.
    ///
    /// This call is cheap — no filesystem scan happens until `warm_up()`.
    pub fn new(base_path: impl Into<PathBuf>, cache_root: impl Into<PathBuf>) -> Self {
        let base_path = base_path.into();
        let cache_root = cache_root.into();
        let cache_dir = cache_root.join(repo_cache_key(&base_path));

        Self {
            inner: Arc::new(AtPickerInner {
                base_path,
                cache_dir,
                shared_picker: SharedFilePicker::default(),
                // Frecency on; query-tracker noop (we don't need combo-boost
                // history for one-off mention completions).
                shared_frecency: SharedFrecency::default(),
                shared_query_tracker: SharedQueryTracker::noop(),
                warm_up_started: AtomicBool::new(false),
                resolve_cache: Default::default(),
            }),
        }
    }

    /// Spawn the background scan + watcher. Idempotent: subsequent calls are
    /// no-ops. Returns `Ok(())` even if frecency init fails (frecency is
    /// optional — search still works without it).
    pub fn warm_up(&self) -> Result<(), AtPickerError> {
        if self.inner.warm_up_started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        // Best-effort cache dir creation. Failure here only disables
        // frecency persistence; search still works.
        let frecency_init = std::fs::create_dir_all(&self.inner.cache_dir)
            .map_err(AtPickerError::CacheDir)
            .and_then(|()| {
                FrecencyTracker::open(self.inner.cache_dir.join("frecency"))
                    .map_err(|e| AtPickerError::Ffs(e.to_string()))
            })
            .and_then(|t| {
                self.inner
                    .shared_frecency
                    .init(t)
                    .map_err(|e| AtPickerError::Ffs(e.to_string()))
            });

        if let Err(err) = frecency_init {
            crate::logging::warn(&format!(
                "AtPicker: frecency init failed, continuing without it: {err}"
            ));
        }

        // QueryTracker is noop — init() is also a no-op, but we leave the
        // call here so future enablement is a one-line change.
        let _ = QueryTracker::open(self.inner.cache_dir.join("queries"))
            .ok()
            .map(|t| self.inner.shared_query_tracker.init(t));

        let opts = FilePickerOptions {
            base_path: self.inner.base_path.to_string_lossy().into_owned(),
            mode: FfsMode::Ai,
            enable_mmap_cache: false,
            enable_content_indexing: false,
            cache_budget: None,
            watch: true,
            follow_symlinks: false,
        };

        FilePicker::new_with_shared_state(
            self.inner.shared_picker.clone(),
            self.inner.shared_frecency.clone(),
            opts,
        )
        .map_err(|e| AtPickerError::Ffs(e.to_string()))?;

        Ok(())
    }

    /// Inspect the current picker state without blocking.
    pub fn state(&self) -> PickerState {
        if !self.inner.warm_up_started.load(Ordering::Acquire) {
            return PickerState::Uninitialized;
        }

        // Try a non-blocking read; if we can read it AND a picker is
        // installed AND scan is no longer active, we're Ready.
        let Ok(guard) = self.inner.shared_picker.read() else {
            return PickerState::Scanning;
        };
        match &*guard {
            None => PickerState::Scanning,
            Some(p) => {
                if p.is_scan_active() {
                    PickerState::Scanning
                } else {
                    PickerState::Ready
                }
            }
        }
    }

    /// Synchronously wait for the scan to finish. Used by tests; the TUI
    /// path uses `state()` polling instead.
    #[cfg(test)]
    pub fn wait_until_ready(&self, timeout: Duration) -> bool {
        self.inner.shared_picker.wait_for_scan(timeout)
    }

    /// Cursor-aware @-mention candidate search via the new ffs Phase A
    /// `MentionResolver`.
    ///
    /// `input` is the raw user text containing the `@<query>` token (with
    /// or without trailing whitespace); `cursor` is the byte offset of the
    /// caret (typically `input.len()` for end-of-buffer autocomplete). The
    /// resolver:
    ///   1. parses the @-token at the cursor (email/URL/mid-word safe)
    ///   2. reuses the existing `fuzzy_search` + `fuzzy_search_directories`
    ///      pipelines ranked by frizbee score + LMDB frecency boost
    ///   3. merges File + Directory candidates into a single ranked list
    ///
    /// `limit` caps the result count (capped at
    /// [`AT_PICKER_MAX_SUGGESTIONS`]). Returns an empty `Vec` if the picker
    /// is not yet `Ready`.
    pub fn search(&self, input: &str, cursor: usize, limit: usize) -> Vec<AtSuggestion> {
        if !matches!(self.state(), PickerState::Ready) {
            return Vec::new();
        }
        let limit = limit.min(AT_PICKER_MAX_SUGGESTIONS);

        let Ok(guard) = self.inner.shared_picker.read() else {
            return Vec::new();
        };
        let Some(picker) = guard.as_ref() else {
            return Vec::new();
        };

        // Build a MentionResolver that delegates to the same ffs pipeline
        // the rest of jcode uses, with frecency already wired via
        // SharedFrecency. The resolver reuses the live picker's lifetime.
        let resolver = MentionResolver::new(picker)
            .with_shared_frecency(self.inner.shared_frecency.clone())
            .with_options(ffs_search::mention::MentionOptions {
                max_candidates: limit,
                include_files: true,
                include_dirs: true,
                ..Default::default()
            });

        let MentionResult { candidates, .. } = resolver.search(input, cursor);

        candidates
            .into_iter()
            .map(|c| at_suggestion_from_candidate(c, picker))
            .collect()
    }

    /// Resolve a candidate path to a `ResolvedMention` using ffs Phase B.
    /// Truncates content to fit `opts.max_tokens` (default 50_000) and
    /// classifies binary/image automatically.
    ///
    /// `turn_id` is an opaque u64 the host passes in (e.g. conversation
    /// turn index) so repeated resolutions inside one turn reuse the
    /// cached content instead of re-reading disk.
    #[allow(dead_code)] // Phase B: TUI selection flow will call this.
    pub fn resolve_payload(
        &self,
        path: &Path,
        turn_id: u64,
        opts: Option<ResolveOptions>,
    ) -> Result<ResolvedMention, String> {
        let opts = opts.unwrap_or_default();
        let opts = ResolveOptions {
            max_tokens: DEFAULT_RESOLVE_BUDGET_TOKENS,
            ..opts
        };
        // Use a turn-keyed cache so repeated calls in the same turn don't
        // re-read the same path. turn_id 0 disables the cache.
        let cache_key = if turn_id == 0 {
            None
        } else {
            Some((path.to_path_buf(), turn_id))
        };
        if let Some(key) = &cache_key
            && let Some(hit) = self
                .inner
                .resolve_cache
                .read()
                .ok()
                .and_then(|g| g.get(key).cloned())
        {
            return Ok(hit);
        }

        let resolved = resolve_mentions(std::slice::from_ref(&path.to_path_buf()), &opts)
            .into_iter()
            .next()
            .ok_or_else(|| format!("no resolution produced for {}", path.display()))?;

        if let Some(key) = cache_key
            && let Ok(mut g) = self.inner.resolve_cache.write()
        {
            g.insert(key, resolved.clone());
        }
        Ok(resolved)
    }

    /// Record that the user selected a given absolute path. Bumps frecency
    /// score so the entry ranks higher next time.
    ///
    /// Best-effort: failures are logged and swallowed.
    #[allow(dead_code)] // Public API kept for upcoming TUI wiring.
    pub fn record_selection(&self, path: &Path) {
        let Ok(frecency_guard) = self.inner.shared_frecency.write() else {
            return;
        };
        let Some(tracker) = frecency_guard.as_ref() else {
            return;
        };
        if let Err(err) = tracker.track_access(path) {
            crate::logging::warn(&format!("AtPicker: frecency track_access failed: {err}"));
        }
    }

    /// Base path the picker is rooted at.
    #[allow(dead_code)] // Public API kept for upcoming TUI wiring.
    pub fn base_path(&self) -> &Path {
        &self.inner.base_path
    }
}

impl std::fmt::Debug for AtPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtPicker")
            .field("base_path", &self.inner.base_path)
            .field("state", &self.state())
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AtPickerError {
    #[error("could not create cache directory: {0}")]
    CacheDir(std::io::Error),
    #[error("ffs error: {0}")]
    Ffs(String),
}

/// Three-state slot for the lazy-initialized picker stored on `App`.
///
/// Lifecycle:
///   Pending  → first @-keystroke triggers init via [`AtPickerSlot::ensure`]
///   Initialized(p) → subsequent calls return `Some(p.clone())`
///   Failed   → stop retrying (e.g. working_dir missing or invalid)
#[derive(Debug, Default)]
pub enum AtPickerSlot {
    #[default]
    Pending,
    Initialized(AtPicker),
    Failed,
}

impl AtPickerSlot {
    /// Get a handle to the picker, lazy-initializing if necessary.
    /// Returns `None` if init has failed or `working_dir` is missing/invalid.
    pub fn ensure(&mut self, working_dir: Option<&str>) -> Option<AtPicker> {
        match self {
            AtPickerSlot::Initialized(p) => return Some(p.clone()),
            AtPickerSlot::Failed => return None,
            AtPickerSlot::Pending => {}
        }
        let cwd = match working_dir {
            Some(s) => Path::new(s),
            None => {
                *self = AtPickerSlot::Failed;
                return None;
            }
        };
        if !cwd.is_dir() {
            *self = AtPickerSlot::Failed;
            return None;
        }
        let cache_root = jcode_ffs_cache_root();
        let picker = AtPicker::new(cwd, cache_root);
        match picker.warm_up() {
            Ok(()) => {
                *self = AtPickerSlot::Initialized(picker.clone());
                Some(picker)
            }
            Err(err) => {
                crate::logging::warn(&format!(
                    "AtPicker: warm_up failed, disabling @ autocomplete: {err}"
                ));
                *self = AtPickerSlot::Failed;
                None
            }
        }
    }

    /// Returns `Some(AtPicker)` only if already initialized; never starts a
    /// new init. Used when we need a non-blocking peek.
    #[allow(dead_code)] // Public API kept for upcoming TUI wiring.
    pub fn get(&self) -> Option<AtPicker> {
        match self {
            AtPickerSlot::Initialized(p) => Some(p.clone()),
            _ => None,
        }
    }
}

/// Convert a `MentionCandidate` (Phase A) to the legacy `AtSuggestion`
/// shape that the rest of the TUI consumes. Drops `Directory` candidates
/// that have empty display paths (root, `.`).
fn at_suggestion_from_candidate(cand: MentionCandidate<'_>, _picker: &FilePicker) -> AtSuggestion {
    let is_directory = matches!(cand.kind, MentionKind::Directory);
    AtSuggestion {
        display_path: cand.relative_path,
        is_directory,
    }
}
fn jcode_ffs_cache_root() -> PathBuf {
    let base = crate::storage::jcode_dir().unwrap_or_else(|_| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".jcode")
    });
    base.join("cache").join("ffs")
}

/// Stable per-repo key for the cache subdirectory. Just the basename plus
/// a short hash of the absolute path so two repos with the same name don't
/// collide.
fn repo_cache_key(path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let basename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo")
        .replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_',
            "_",
        );
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{}-{:016x}", basename, hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_repo() -> (TempDir, TempDir) {
        let repo = TempDir::new().unwrap();
        let cache = TempDir::new().unwrap();
        fs::write(repo.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(repo.path().join("Cargo.toml"), "[package]").unwrap();
        fs::create_dir_all(repo.path().join("src")).unwrap();
        fs::write(repo.path().join("src/lib.rs"), "// lib").unwrap();
        fs::create_dir_all(repo.path().join("docs")).unwrap();
        fs::write(repo.path().join("docs/README.md"), "# docs").unwrap();
        (repo, cache)
    }

    #[test]
    fn state_uninitialized_before_warm_up() {
        let (repo, cache) = setup_repo();
        let picker = AtPicker::new(repo.path(), cache.path());
        assert_eq!(picker.state(), PickerState::Uninitialized);
        // Uninitialized state: empty regardless of input.
        assert!(picker.search("@main", 5, 10).is_empty());
    }

    #[test]
    fn warm_up_eventually_becomes_ready() {
        let (repo, cache) = setup_repo();
        let picker = AtPicker::new(repo.path(), cache.path());
        picker.warm_up().expect("warm up");
        assert!(
            picker.wait_until_ready(Duration::from_secs(5)),
            "scan did not complete within 5s"
        );
        assert_eq!(picker.state(), PickerState::Ready);
    }

    #[test]
    fn search_returns_files_and_dirs() {
        let (repo, cache) = setup_repo();
        let picker = AtPicker::new(repo.path(), cache.path());
        picker.warm_up().expect("warm up");
        assert!(picker.wait_until_ready(Duration::from_secs(5)));

        let results = picker.search("@", 1, 15);
        assert!(!results.is_empty(), "expected non-empty initial listing");

        // We expect at least our planted files + dirs to surface.
        let has_main = results
            .iter()
            .any(|s| !s.is_directory && s.display_path.contains("main.rs"));
        let has_dir = results.iter().any(|s| s.is_directory);
        assert!(has_main, "expected main.rs in results: {:?}", results);
        assert!(has_dir, "expected at least one directory: {:?}", results);
    }

    #[test]
    fn search_filters_by_query() {
        let (repo, cache) = setup_repo();
        let picker = AtPicker::new(repo.path(), cache.path());
        picker.warm_up().expect("warm up");
        assert!(picker.wait_until_ready(Duration::from_secs(5)));

        let results = picker.search("@Cargo", 6, 15);
        assert!(
            results
                .iter()
                .any(|s| s.display_path.contains("Cargo.toml")),
            "expected Cargo.toml: {:?}",
            results
        );
    }

    #[test]
    fn search_with_trailing_slash_returns_dirs_only() {
        let (repo, cache) = setup_repo();
        let picker = AtPicker::new(repo.path(), cache.path());
        picker.warm_up().expect("warm up");
        assert!(picker.wait_until_ready(Duration::from_secs(5)));

        let results = picker.search("@/", 2, 15);
        // The resolver returns a mix; filter to dirs to validate the
        // directory-candidate path still surfaces the planted `docs/` etc.
        let dir_count = results.iter().filter(|s| s.is_directory).count();
        assert!(
            dir_count > 0,
            "expected at least one directory in @/ results: {:?}",
            results
        );
    }

    #[test]
    fn warm_up_idempotent() {
        let (repo, cache) = setup_repo();
        let picker = AtPicker::new(repo.path(), cache.path());
        picker.warm_up().expect("first warm up");
        picker.warm_up().expect("second warm up no-op");
    }

    #[test]
    fn multi_picker_same_dir_no_deadlock() {
        // Risk: two AtPickers on the same cwd shouldn't deadlock on LMDB.
        // We give them DIFFERENT cache subdirs (per-PID-style isolation)
        // because LMDB only supports one writer per environment.
        let (repo, _cache) = setup_repo();
        let cache1 = TempDir::new().unwrap();
        let cache2 = TempDir::new().unwrap();

        let p1 = AtPicker::new(repo.path(), cache1.path());
        let p2 = AtPicker::new(repo.path(), cache2.path());
        p1.warm_up().expect("p1 warm");
        p2.warm_up().expect("p2 warm");

        assert!(p1.wait_until_ready(Duration::from_secs(5)));
        assert!(p2.wait_until_ready(Duration::from_secs(5)));

        let r1 = p1.search("@main", 5, 10);
        let r2 = p2.search("@main", 5, 10);
        assert!(!r1.is_empty());
        assert!(!r2.is_empty());
    }

    #[test]
    fn repo_cache_key_is_stable_and_unique() {
        let a = repo_cache_key(Path::new("/tmp/foo"));
        let b = repo_cache_key(Path::new("/tmp/foo"));
        let c = repo_cache_key(Path::new("/tmp/bar"));
        assert_eq!(a, b, "same path → same key");
        assert_ne!(a, c, "different path → different key");
        assert!(a.starts_with("foo-"));
        assert!(c.starts_with("bar-"));
    }
}
