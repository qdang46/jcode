//! Atomic file primitives — port of
//! `src/features/team-mode/team-state-store/locks.ts`.
//!
//! - `with_lock`: exclusive-create lockfile with stale-owner reaping.
//! - `atomic_write`: temp file + fsync + parent-dir fsync + rename
//!   (atomic on the same volume, durable across power loss).
//! - `read_json`: typed read.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::team::spec::{TeamError, TeamResult};

const LOCK_RETRY_BASE_MS: u64 = 50;
/// Upper bound for the random jitter added to each retry sleep.
const LOCK_RETRY_JITTER_MS: u64 = 25;
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_STALE: Duration = Duration::from_secs(300);
const STALE_VERDICT_RECHECK: Duration = Duration::from_millis(250);

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

/// RAII guard that unlinks a temp file on early return (e.g. panic, write error).
struct TempFileGuard {
    path: Option<PathBuf>,
}
impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
    /// Mark the rename as successful; the temp file no longer exists at `path`.
    fn disarm(mut self) {
        self.path = None;
    }
}
impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(p) = self.path.take() {
            let _ = fs::remove_file(&p);
        }
    }
}

/// Random jitter in `[0, LOCK_RETRY_JITTER_MS]` ms. Avoids thundering herd under
/// lock contention: callers retrying the same lock wake at slightly different times.
fn retry_jitter() -> Duration {
    use std::cell::Cell;
    thread_local! {
        static RNG: Cell<Option<u64>> = const { Cell::new(None) };
    }
    RNG.with(|r| {
        let mut s = r.get().unwrap_or(0);
        // xorshift64 — small state, fast, no external dep.
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        r.set(Some(s));
        if s == 0 {
            s = 1;
        }
        Duration::from_millis((s % (LOCK_RETRY_JITTER_MS + 1)).max(1))
    })
}

/// Set file mode to owner-only. Best-effort: failures are silently ignored
/// (a non-unix path or a filesystem that doesn't support chmod just keeps the
/// umask default; the parent directory's 0o700 is the primary protection).
#[cfg(unix)]
fn set_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = fs::set_permissions(path, perms);
    }
}
#[cfg(not(unix))]
fn set_private(_path: &Path) {}

/// Read the lock body and decide whether it is stale. The verdict can be cached
/// for a short window (`STALE_VERDICT_RECHECK`) to avoid re-stat'ing the lock
/// on every retry under contention.
struct StaleVerdict {
    is_stale: bool,
    last_checked: Instant,
}

impl StaleVerdict {
    fn fresh(&self) -> bool {
        self.last_checked.elapsed() < STALE_VERDICT_RECHECK
    }
    fn stale_check(lock_path: &Path, stale: Duration) -> bool {
        let Ok(content) = fs::read_to_string(lock_path) else {
            return false;
        };
        let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
        if lines.len() != 4 {
            return false;
        }
        let (Ok(pid), Ok(acquired), Ok(_hostname)) = (
            lines[1].parse::<u32>(),
            lines[2].parse::<u128>(),
            lines[3].parse::<String>(),
        ) else {
            return false;
        };
        // PID-reuse defense: only treat as stale if the recorded PID is dead
        // AND enough time has passed. A recycled PID that happens to be a
        // different process will report alive; the lock is then NOT reaped,
        // which is the correct safe behavior (better to wait than to free
        // someone else's lock).
        if pid_alive(pid) {
            return false;
        }
        epoch_ms().saturating_sub(acquired) > stale.as_millis()
    }
}

/// Acquire an exclusive lockfile, run `body`, then release. Mirrors `withLock`.
/// Uses `DEFAULT_STALE` (300s) as the stale-lock reaping threshold.
pub fn with_lock<T>(
    lock_path: &Path,
    owner_tag: &str,
    body: impl FnOnce() -> TeamResult<T>,
) -> TeamResult<T> {
    with_lock_stale(lock_path, owner_tag, DEFAULT_STALE, LOCK_WAIT_TIMEOUT, body)
}

/// Like `with_lock` but with explicit stale threshold and acquire timeout.
/// Use a shorter `stale` for brief operations (e.g. state transitions) so
/// crashed owners don't block recovery for the default 5 minutes.
pub fn with_lock_stale<T>(
    lock_path: &Path,
    owner_tag: &str,
    stale: Duration,
    acquire_timeout: Duration,
    body: impl FnOnce() -> TeamResult<T>,
) -> TeamResult<T> {
    let start = Instant::now();
    let hostname = hostname();
    let mut verdict = StaleVerdict {
        is_stale: false,
        last_checked: Instant::now() - STALE_VERDICT_RECHECK,
    };
    loop {
        if start.elapsed() > acquire_timeout {
            return Err(TeamError::LockTimeout(lock_path.display().to_string()));
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(mut f) => {
                // Surface write errors (lock body is critical for stale detection).
                write!(
                    f,
                    "{}\n{}\n{}\n{}\n",
                    owner_tag,
                    std::process::id(),
                    epoch_ms(),
                    hostname,
                )?;
                f.sync_all()?;
                set_private(lock_path);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if !verdict.fresh() {
                    verdict = StaleVerdict {
                        is_stale: StaleVerdict::stale_check(lock_path, stale),
                        last_checked: Instant::now(),
                    };
                }
                if verdict.is_stale {
                    let _ = fs::remove_file(lock_path);
                    // Force a re-check on the next loop iteration.
                    verdict.last_checked = Instant::now() - STALE_VERDICT_RECHECK;
                    continue;
                }
                let sleep = LOCK_RETRY_BASE_MS + retry_jitter().as_millis() as u64;
                std::thread::sleep(Duration::from_millis(sleep));
            }
            Err(e) => return Err(TeamError::Io(e)),
        }
    }
    let result = body();
    let _ = fs::remove_file(lock_path); // release (best-effort, like reapStaleLock)
    result
}

/// Best-effort hostname lookup. Returns "unknown" if it cannot be determined.
/// Tries `/etc/hostname` on Linux first (single-read), then falls back to
/// `libc::gethostname` which works on macOS (no `/etc/hostname` on Darwin).
fn hostname() -> String {
    #[cfg(unix)]
    {
        if let Ok(s) = fs::read_to_string("/etc/hostname") {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        // On macOS (and some Linux) `/etc/hostname` doesn't exist; use
        // libc::gethostname instead. SAFETY: gethostname writes up to 256
        // bytes into a stack buffer; we zero it first and check for NUL.
        let mut buf = [0u8; 256];
        if unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) == 0 } {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            if end > 0 {
                return String::from_utf8_lossy(&buf[..end]).to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Write `content` to a temp file, fsync it, fsync the parent directory,
/// then atomically rename into place. The parent-dir fsync is the difference
/// between "durable on the inode" and "durable in the directory entry";
/// without it, a power loss can leave the file present but missing from `ls`.
pub fn atomic_write(path: &Path, content: &str) -> TeamResult<()> {
    let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    let guard = TempFileGuard::new(tmp.clone());
    {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        // The temp is written; leave the guard armed in case `rename` fails
        // so the temp is cleaned up by Drop.
        f.flush()?;
    }
    // Sync the parent dir BEFORE rename so the new file's directory entry is
    // durable. Skip when the parent is a file (e.g. tests use paths inside
    // a tempdir whose parent is a regular file only in pathological cases).
    if let Some(parent) = path.parent().and_then(|p| File::open(p).ok()) {
        let _ = parent.sync_all();
    }
    match fs::rename(&tmp, path) {
        Ok(()) => {
            guard.disarm();
            set_private(path);
            Ok(())
        }
        Err(e) => Err(TeamError::Io(e)),
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
        // Write a 4-line lock owned by a definitely-dead pid, acquired long ago.
        // Format: owner_tag\npid\nacquired_ms\nhostname
        std::fs::write(&lock, "owner\n999999999\n1\nunknown\n").unwrap();
        // Should reap and acquire without timing out.
        let out = with_lock(&lock, "test", || Ok(7)).unwrap();
        assert_eq!(out, 7);
    }

    #[test]
    fn with_lock_keeps_live_pid_lock_for_full_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("l.lock");
        let our_pid = std::process::id();
        // Lock owned by us, with a fresh timestamp: must NOT be reaped.
        std::fs::write(
            &lock,
            format!("self\n{}\n{}\nunknown\n", our_pid, epoch_ms()),
        )
        .unwrap();
        // Use a tiny body and verify we time out because the live lock is held.
        let res = with_lock(&lock, "test", || Ok(1));
        assert!(matches!(res, Err(TeamError::LockTimeout(_))));
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

    #[test]
    fn atomic_write_cleans_temp_on_rename_target_locked() {
        // Simulate a write failure by attempting atomic_write to a path inside
        // a read-only directory. The Drop guard must remove the temp file.
        let dir = tempfile::tempdir().unwrap();
        // Create the target file as read-only by first writing then chmodding
        // the parent. We just attempt to write to a path that will fail because
        // the target is a directory (rename(2) will fail with EISDIR).
        let blocker = dir.path().join("blocker");
        std::fs::create_dir(&blocker).unwrap();
        let res = atomic_write(&blocker, "x");
        // On unix this fails because you can't rename a temp onto a directory.
        assert!(res.is_err());
        // No `.tmp.*` should remain in the parent.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                n.starts_with("blocker") && n.contains("tmp")
            })
            .collect();
        assert!(leftovers.is_empty(), "Drop guard must clean up temp");
    }
}
