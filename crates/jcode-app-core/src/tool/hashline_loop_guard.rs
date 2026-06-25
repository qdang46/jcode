//! No-op loop guard for hashline edit retry protection.
//!
//! Tracks consecutive byte-identical edit results per path. After 3 identical
//! no-ops the guard escalates to a hard error, preventing the model from
//! burning tokens on identical retries.

use std::path::PathBuf;
use std::sync::Mutex;

/// Upper bound on identical no-ops before the guard fires (mirrors oh-my-pi).
const NOOP_LIMIT: u64 = 3;

/// Error raised when the no-op limit is exceeded.
#[derive(Debug)]
pub struct NoopLimitExceeded {
    pub path: PathBuf,
    pub count: u64,
}

impl std::fmt::Display for NoopLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "hashline edit produced no change {} times in a row for {} — \
             the replacement does not match current file content. Re-read the file first.",
            self.count,
            self.path.display()
        )
    }
}

/// Guard structure that tracks repetition per path.
pub struct NoopGuard {
    inner: Mutex<std::collections::HashMap<PathBuf, (u64, u64)>>,
}

impl Default for NoopGuard {
    fn default() -> Self {
        Self {
            inner: Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl NoopGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an edit outcome. If `contents_differ` is `false` (no change)
    /// and the same content hash was produced last time, increment the counter.
    /// Returns `Ok(())` or `Err(NoopLimitExceeded)`.
    pub fn record(
        &self,
        path: PathBuf,
        contents_differ: bool,
        content_hash: u64,
    ) -> Result<(), NoopLimitExceeded> {
        let mut map = self.inner.lock().expect("noop guard poisoned");
        if contents_differ {
            map.remove(&path);
            return Ok(());
        }
        let entry = map.entry(path.clone()).or_insert((0, 0));
        if entry.1 == content_hash {
            entry.0 += 1;
        } else {
            entry.0 = 1;
            entry.1 = content_hash;
        }
        if entry.0 >= NOOP_LIMIT {
            let count = entry.0;
            map.remove(&path);
            return Err(NoopLimitExceeded { path, count });
        }
        Ok(())
    }

    /// Reset the counter for a path (called on successful edit or re-read).
    pub fn reset(&self, path: &PathBuf) {
        let mut map = self.inner.lock().expect("noop guard poisoned");
        map.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_first_noop() {
        let guard = NoopGuard::new();
        let path = PathBuf::from("/tmp/test.rs");
        assert!(guard.record(path, false, 42).is_ok());
    }

    #[test]
    fn errors_after_three_identical_noops() {
        let guard = NoopGuard::new();
        let path = PathBuf::from("/tmp/test.rs");
        assert!(guard.record(path.clone(), false, 42).is_ok());
        assert!(guard.record(path.clone(), false, 42).is_ok());
        let err = guard.record(path.clone(), false, 42).unwrap_err();
        assert!(err.to_string().contains("3 times"));
    }

    #[test]
    fn different_hash_resets_counter() {
        let guard = NoopGuard::new();
        let path = PathBuf::from("/tmp/test.rs");
        assert!(guard.record(path.clone(), false, 42).is_ok());
        assert!(guard.record(path.clone(), false, 99).is_ok());
        assert!(guard.record(path.clone(), false, 99).is_ok());
        assert!(guard.record(path.clone(), false, 99).is_ok());
        let err = guard.record(path.clone(), false, 99).unwrap_err();
        assert!(err.to_string().contains("3 times"));
    }

    #[test]
    fn successful_edit_resets_counter() {
        let guard = NoopGuard::new();
        let path = PathBuf::from("/tmp/test.rs");
        assert!(guard.record(path.clone(), false, 42).is_ok());
        guard.reset(&path);
        assert!(guard.record(path.clone(), false, 42).is_ok());
        assert!(guard.record(path.clone(), false, 42).is_ok());
        assert!(guard.record(path.clone(), false, 42).is_ok());
        let err = guard.record(path.clone(), false, 42).unwrap_err();
        assert!(err.to_string().contains("3 times"));
    }

    #[test]
    fn differ_content_removes_path() {
        let guard = NoopGuard::new();
        let path = PathBuf::from("/tmp/test.rs");
        assert!(guard.record(path.clone(), false, 42).is_ok());
        assert!(guard.record(path.clone(), true, 99).is_ok());
        assert!(guard.record(path.clone(), false, 42).is_ok());
        assert!(guard.record(path.clone(), false, 42).is_ok());
        assert!(guard.record(path.clone(), false, 42).is_ok());
        let err = guard.record(path.clone(), false, 42).unwrap_err();
        assert!(err.to_string().contains("3 times"));
    }
}
