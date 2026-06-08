//! Runtime state store — port of `team-state-store/store.ts`.
//!
//! The on-disk `state.json` per run is the source of truth. Reads are plain;
//! writes go through a per-run lock + atomic rename.

use std::fs;

use crate::team::locks::{atomic_write, read_json, with_lock_stale};
use crate::team::paths::{runtime_dir, runtime_state_path, teams_base_dir};
use crate::team::spec::*;
use std::time::Duration;

/// Stale threshold for per-run state lock: brief read-modify-write operations
/// should recover quickly from a crash. 30s vs the default 300s.
const STATE_LOCK_STALE: Duration = Duration::from_secs(30);

/// Create the initial runtime state file (status = `Creating`).
pub fn create_runtime(
    spec: &TeamSpec,
    lead_session_id: &str,
    source: SpecSource,
) -> TeamResult<TeamRuntimeState> {
    let run_id = uuid::Uuid::new_v4().to_string();
    let now = now_millis();
    let members = spec
        .members
        .iter()
        .map(|m| MemberRuntime {
            name: m.name().to_string(),
            session_id: None,
            tmux_pane_id: None,
            agent_type: if Some(m.name()) == spec.lead_agent_id.as_deref() {
                MemberAgentType::Leader
            } else {
                MemberAgentType::GeneralPurpose
            },
            subagent_type: match m {
                TeamMemberSpec::SubagentType { subagent_type, .. } => Some(subagent_type.clone()),
                TeamMemberSpec::Category { .. } => None,
            },
            category: match m {
                TeamMemberSpec::Category { category, .. } => Some(category.clone()),
                TeamMemberSpec::SubagentType { .. } => None,
            },
            status: MemberStatus::Pending,
            color: m.common().color.clone(),
            worktree_path: m.common().worktree_path.clone(),
            last_injected_turn_marker: None,
            pending_injected_message_ids: Vec::new(),
        })
        .collect();

    let state = TeamRuntimeState {
        version: 1,
        team_run_id: run_id.clone(),
        team_name: spec.name.clone(),
        spec_source: source,
        created_at: now,
        status: RuntimeStatus::Creating,
        lead_session_id: Some(lead_session_id.to_string()),
        tmux_layout: None,
        members,
        shutdown_requests: Vec::new(),
        bounds: RuntimeBounds::default(),
        capability_token: uuid::Uuid::new_v4().simple().to_string(),
    };
    fs::create_dir_all(runtime_dir(&run_id))?;
    persist(&state)?;
    Ok(state)
}

/// Load the runtime state for a run, or `NotFound` if its file is absent.
///
/// Validates the on-disk `version` field. A `state.json` written by an older
/// or newer schema (`version != 1`) is rejected with a clear error so callers
/// can run a migrator instead of silently deserializing with default-valued
/// missing fields (which would look loaded but be silently corrupt).
pub fn load_runtime(run_id: &str) -> TeamResult<TeamRuntimeState> {
    let path = runtime_state_path(run_id);
    if !path.exists() {
        return Err(TeamError::NotFound(run_id.to_string()));
    }
    let state: TeamRuntimeState = read_json(&path)?;
    if state.version != 1 {
        return Err(TeamError::UnsupportedSchemaVersion {
            run_id: run_id.to_string(),
            found: state.version,
            expected: 1,
        });
    }
    Ok(state)
}

fn persist(state: &TeamRuntimeState) -> TeamResult<()> {
    atomic_write(
        &runtime_state_path(&state.team_run_id),
        &format!("{}\n", serde_json::to_string_pretty(state)?),
    )
}

/// Read-modify-write the runtime state under a per-run lock
/// (port of `transitionRuntimeState`).
pub fn transition<F>(run_id: &str, mutate: F) -> TeamResult<TeamRuntimeState>
where
    F: FnOnce(&mut TeamRuntimeState),
{
    let lock = runtime_dir(run_id).join(".state.lock");
    with_lock_stale(
        &lock,
        &format!("transition:{run_id}"),
        STATE_LOCK_STALE,
        Duration::from_secs(15),
        || {
            let mut state = load_runtime(run_id)?;
            mutate(&mut state);
            persist(&state)?;
            Ok(state)
        },
    )
}

/// Enumerate runtime states whose status is `Creating` or `Active`.
pub fn list_active_runs() -> TeamResult<Vec<TeamRuntimeState>> {
    let runtime_root = teams_base_dir().join("runtime");
    let mut out = Vec::new();
    let rd = match fs::read_dir(&runtime_root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let run_id = entry.file_name().to_string_lossy().into_owned();
        // Runs with corrupt or unsupported-version state.json are silently
        // skipped. An orphan run leaves files on disk but is unreachable
        // via tools and invisible in the TUI; the stale-sweep callback
        // should eventually clean it up.
        let state = match load_runtime(&run_id) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if matches!(
            state.status,
            RuntimeStatus::Creating | RuntimeStatus::Active
        ) {
            out.push(state);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> TeamSpec {
        TeamSpec {
            version: 1,
            name: name.into(),
            description: None,
            created_at: 0,
            lead_agent_id: Some("lead".into()),
            team_allowed_paths: None,
            members: vec![
                TeamMemberSpec::SubagentType {
                    name: "lead".into(),
                    subagent_type: "sisyphus".into(),
                    prompt: None,
                    common: MemberCommon::default(),
                },
                TeamMemberSpec::SubagentType {
                    name: "worker".into(),
                    subagent_type: "sisyphus-junior".into(),
                    prompt: None,
                    common: MemberCommon::default(),
                },
            ],
        }
    }

    #[test]
    fn create_load_transition_roundtrip() {
        let _base = crate::team::test_support::guarded_base();
        let state = create_runtime(&spec("alpha"), "sess-lead", SpecSource::Project).unwrap();
        assert_eq!(state.status, RuntimeStatus::Creating);
        assert_eq!(state.members.len(), 2);
        assert_eq!(state.members[0].agent_type, MemberAgentType::Leader);
        assert_eq!(state.members[1].agent_type, MemberAgentType::GeneralPurpose);

        let run = state.team_run_id.clone();
        let loaded = load_runtime(&run).unwrap();
        assert_eq!(loaded.team_name, "alpha");

        let updated = transition(&run, |s| s.status = RuntimeStatus::Active).unwrap();
        assert_eq!(updated.status, RuntimeStatus::Active);
        assert_eq!(load_runtime(&run).unwrap().status, RuntimeStatus::Active);
    }

    #[test]
    fn list_active_runs_filters_by_status() {
        let _base = crate::team::test_support::guarded_base();
        let a = create_runtime(&spec("a"), "s", SpecSource::Project).unwrap();
        let _b = create_runtime(&spec("b"), "s", SpecSource::Project).unwrap();
        transition(&a.team_run_id, |s| s.status = RuntimeStatus::Deleted).unwrap();
        let active = list_active_runs().unwrap();
        // `a` is Deleted, `b` is still Creating → only `b` is active.
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].team_name, "b");
    }

    #[test]
    fn load_missing_run_is_not_found() {
        let _base = crate::team::test_support::guarded_base();
        assert!(matches!(
            load_runtime("nope").unwrap_err(),
            TeamError::NotFound(_)
        ));
    }
}
