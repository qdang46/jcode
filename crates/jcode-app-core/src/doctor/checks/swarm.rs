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

/// Count `git status --porcelain` entries in `cwd`. `None` if not a git repo or
/// git is unavailable.
fn git_status_porcelain(cwd: &std::path::Path) -> Option<usize> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
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
