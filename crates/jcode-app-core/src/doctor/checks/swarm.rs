//! Swarm preconditions (re-interpretation of pi's "admission control"):
//! READ-ONLY checks of the conditions a healthy swarm launch needs. This never
//! spawns, kills, or admits agents, and never mutates git state — even with
//! `--fix`.

use super::super::types::{CheckCategory, DoctorOptions, Finding};

pub fn check_swarm(opts: &DoctorOptions, out: &mut Vec<Finding>) {
    // Swarm agents coordinate on shared files, so a clean worktree avoids
    // "code shifting under your feet" conflicts. Report, never mutate.
    match git_status_porcelain(&opts.cwd) {
        None => out.push(Finding::ok(
            CheckCategory::Swarm,
            "not a git repository (swarm git checks skipped)",
        )),
        Some(0) => out.push(Finding::ok(CheckCategory::Swarm, "git worktree clean")),
        Some(n) => out.push(
            Finding::warn(
                CheckCategory::Swarm,
                format!("git worktree has {n} uncommitted change(s)"),
            )
            .with_remediation("commit or stash before spawning a swarm to avoid edit conflicts"),
        ),
    }
}

/// Count `git status --porcelain` entries in `cwd`. `None` if not a git repo,
/// git is unavailable, or git does not finish within the timeout.
///
/// Bounded by a 5s timeout (a hung/slow FS or index.lock must not hang an
/// offline health check) and runs with `core.fsmonitor=` disabled so a
/// status-time fsmonitor command from an untrusted repo cannot execute.
fn git_status_porcelain(cwd: &std::path::Path) -> Option<usize> {
    use std::sync::mpsc;
    use std::time::Duration;

    let cwd = cwd.to_path_buf();
    let (tx, rx) = mpsc::channel();
    // Clone the sender so we can drop the original before recv_timeout (if the
    // spawned thread panics, the clone is dropped during unwind and the channel
    // errors immediately instead of hanging for the full timeout).
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        let result = std::process::Command::new("git")
            .args([
                "-c",
                "core.fsmonitor=",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "filter.lfs.smudge=",
                "-c",
                "protocol.version=2",
                "-c",
                "core.optionalLocks=true",
            ])
            .arg("-C")
            .arg(&cwd)
            .args(["status", "--porcelain"])
            .output();
        let _ = tx.send(result);
    });
    drop(tx2);
    let output = rx.recv_timeout(Duration::from_secs(5)).ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
    )
}
