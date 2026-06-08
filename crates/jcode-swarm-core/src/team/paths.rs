//! Filesystem layout for team runtime state — port of
//! `src/features/team-mode/team-registry/paths.ts`.

use std::path::{Path, PathBuf};

use crate::team::spec::{TeamError, TeamResult};

/// Env var that overrides the team base directory (used by tests/benchmarks).
pub const TEAMS_BASE_OVERRIDE_ENV: &str = "JCODE_TEAMS_BASE_OVERRIDE";

/// `~/.jcode/teams` — the team base directory (or the override dir in tests).
pub fn teams_base_dir() -> PathBuf {
    if let Some(over) = std::env::var_os(TEAMS_BASE_OVERRIDE_ENV) {
        return PathBuf::from(over);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".jcode")
        .join("teams")
}

pub fn runtime_dir(run_id: &str) -> PathBuf {
    teams_base_dir().join("runtime").join(run_id)
}

pub fn runtime_state_path(run_id: &str) -> PathBuf {
    runtime_dir(run_id).join("state.json")
}

pub fn inbox_dir(run_id: &str, member: &str) -> PathBuf {
    runtime_dir(run_id).join("inboxes").join(member)
}

pub fn tasks_dir(run_id: &str) -> PathBuf {
    runtime_dir(run_id).join("tasks")
}

pub fn worktree_dir(run_id: &str, member: &str) -> PathBuf {
    teams_base_dir()
        .join("worktrees")
        .join(run_id)
        .join(member)
}

/// Create the base directory tree for a run with 0o700 perms (port of `ensureBaseDirs`).
pub fn ensure_base_dirs(run_id: &str, members: &[String]) -> TeamResult<()> {
    use std::fs;
    for d in [
        teams_base_dir(),
        runtime_dir(run_id),
        tasks_dir(run_id),
        tasks_dir(run_id).join("claims"),
    ] {
        fs::create_dir_all(&d)?;
        set_private(&d);
    }
    for m in members {
        let d = inbox_dir(run_id, m);
        fs::create_dir_all(&d)?;
        set_private(&d);
    }
    Ok(())
}

#[cfg(unix)]
fn set_private(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn set_private(_p: &Path) {}

/// Reject empty names, traversal, and any character outside `[a-z0-9_-]`.
pub fn validate_team_name(name: &str) -> TeamResult<()> {
    if name.is_empty() {
        return Err(TeamError::InvalidTeamName(name.into(), "empty".into()));
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(TeamError::InvalidTeamName(
            name.into(),
            "must not contain '..', '/', or '\\'".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(TeamError::InvalidTeamName(
            name.into(),
            "only alphanumeric, hyphen, and underscore allowed".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_traversal_and_bad_chars() {
        assert!(validate_team_name("").is_err());
        assert!(validate_team_name("../etc").is_err());
        assert!(validate_team_name("a/b").is_err());
        assert!(validate_team_name("bad name").is_err());
        assert!(validate_team_name("Good-Team_1").is_ok());
    }

    #[test]
    fn ensure_base_dirs_creates_full_tree() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        ensure_base_dirs(&run, &["alpha".into(), "beta".into()]).unwrap();
        assert!(tasks_dir(&run).join("claims").is_dir());
        assert!(inbox_dir(&run, "alpha").is_dir());
        assert!(inbox_dir(&run, "beta").is_dir());
    }
}
