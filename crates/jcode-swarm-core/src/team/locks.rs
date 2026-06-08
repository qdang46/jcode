//! Atomic file primitives — port of
//! `src/features/team-mode/team-state-store/locks.ts`.
//!
//! - `with_lock`: exclusive-create lockfile with stale-owner reaping.
//! - `atomic_write`: temp file + fsync + rename (atomic on the same volume).
//! - `read_json`: typed read.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::team::spec::{TeamError, TeamResult};

const LOCK_RETRY: Duration = Duration::from_millis(50);
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_STALE: Duration = Duration::from_secs(300);

fn epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Is `pid` alive? `kill(pid, 0)` performs error checking without sending a signal.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks for process existence/permission; it sends nothing.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}
#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    // Conservative on non-unix: never treat a lock as stale via pid liveness.
    true
}

fn detect_stale(lock_path: &Path, stale: Duration) -> bool {
    let Ok(content) = fs::read_to_string(lock_path) else {
        return false;
    };
    let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    if lines.len() != 3 {
        return false;
    }
    let (Ok(pid), Ok(acquired)) = (lines[1].parse::<u32>(), lines[2].parse::<u128>()) else {
        return false;
    };
    if pid_alive(pid) {
        return false;
    }
    epoch_ms().saturating_sub(acquired) > stale.as_millis()
}

/// Acquire an exclusive lockfile, run `body`, then release. Mirrors `withLock`.
pub fn with_lock<T>(
    lock_path: &Path,
    owner_tag: &str,
    body: impl FnOnce() -> TeamResult<T>,
) -> TeamResult<T> {
    let start = Instant::now();
    loop {
        if start.elapsed() > LOCK_WAIT_TIMEOUT {
            return Err(TeamError::LockTimeout(lock_path.display().to_string()));
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(mut f) => {
                let _ = write!(f, "{owner_tag}\n{}\n{}\n", std::process::id(), epoch_ms());
                let _ = f.sync_all();
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if detect_stale(lock_path, DEFAULT_STALE) {
                    let _ = fs::remove_file(lock_path);
                    continue;
                }
                std::thread::sleep(LOCK_RETRY);
            }
            Err(e) => return Err(TeamError::Io(e)),
        }
    }
    let result = body();
    let _ = fs::remove_file(lock_path); // release (best-effort, like reapStaleLock)
    result
}

/// Write `content` to a temp file, fsync, then atomically rename into place.
pub fn atomic_write(path: &Path, content: &str) -> TeamResult<()> {
    let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(TeamError::Io(e))
        }
    }
}

/// Read and deserialize a JSON file into `T`.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> TeamResult<T> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_lock_runs_body_and_releases() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("x.lock");
        let out = with_lock(&lock, "test", || Ok(42)).unwrap();
        assert_eq!(out, 42);
        assert!(!lock.exists(), "lock must be released after body runs");
    }

    #[test]
    fn with_lock_reaps_stale_dead_pid_lock() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("s.lock");
        // Write a lock owned by a definitely-dead pid, acquired long ago.
        std::fs::write(&lock, "owner\n999999999\n1\n").unwrap();
        // Should reap and acquire without timing out.
        let out = with_lock(&lock, "test", || Ok(7)).unwrap();
        assert_eq!(out, 7);
    }

    #[test]
    fn atomic_write_then_read_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("data.json");
        atomic_write(&p, "{\"a\":1}\n").unwrap();
        let v: serde_json::Value = read_json(&p).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn atomic_write_leaves_no_temp_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d.json");
        atomic_write(&p, "{}").unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no .tmp files should remain");
    }
}
