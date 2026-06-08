//! Test-only helpers: a process-wide serial guard that isolates the
//! filesystem base dir (`JCODE_TEAMS_BASE_OVERRIDE`) per test so concurrent
//! tests never race on the env var.

use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::team::paths::TEAMS_BASE_OVERRIDE_ENV;

static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Holds the global test lock and a private temp base dir for the duration of a
/// test. While alive, `teams_base_dir()` resolves under `dir`.
pub struct TestBase {
    _guard: MutexGuard<'static, ()>,
    /// Held to keep the temp base dir alive for the test's duration (RAII).
    #[allow(dead_code)]
    pub dir: tempfile::TempDir,
}

impl TestBase {
    /// A fresh unique team run id.
    pub fn run_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// Acquire the serial guard and point the team base dir at a fresh temp dir.
pub fn guarded_base() -> TestBase {
    let guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: the serial guard ensures no other team test reads/writes this env
    // var concurrently while we mutate it.
    unsafe {
        std::env::set_var(TEAMS_BASE_OVERRIDE_ENV, dir.path());
    }
    TestBase { _guard: guard, dir }
}
