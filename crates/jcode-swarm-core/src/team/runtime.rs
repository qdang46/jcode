//! Team lifecycle — port of `team-runtime/{create,delete-team,shutdown}.ts`.
//!
//! Design note (deviation from the TS reference, intentional): `create_team`
//! here is tmux-free and synchronous so it is fully unit-testable. Bounded
//! parallel member spawn uses `std::thread::scope` (no async runtime in this
//! crate). Tmux layout activation and stale-session sweeping are separate
//! functions the app-core layer calls when a live tmux context exists.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::team::eligibility::assert_eligible;
use crate::team::layout::{self, LayoutMember};
use crate::team::{mailbox, paths, spec::*, state};

/// Spawns a headless member session and returns its session id. In jcode this
/// wraps `std::process::Command::new("jcode")` + server registration; in tests
/// it is a stub.
pub trait MemberSpawner: Send + Sync {
    fn spawn(&self, run_id: &str, member: &TeamMemberSpec, prompt: &str) -> TeamResult<String>;
}

/// Create a team: validate, persist runtime state, spawn members (bounded
/// parallelism), and mark `Active`. Idempotent for an existing active run with
/// the same (name, lead). Tmux-free; call [`activate_team_layout`] separately.
pub fn create_team(
    mut spec: TeamSpec,
    lead_session_id: &str,
    spawner: &dyn MemberSpawner,
) -> TeamResult<TeamRuntimeState> {
    normalize_spec(&mut spec)?;
    paths::validate_team_name(&spec.name)?;
    for m in &spec.members {
        if let Err(msg) = assert_eligible(m.agent_type()) {
            return Err(TeamError::IneligibleAgent(m.name().to_string(), msg));
        }
    }

    if let Some(existing) = find_existing_run(&spec.name, lead_session_id)? {
        return Ok(existing);
    }

    let member_names: Vec<String> = spec.members.iter().map(|m| m.name().to_string()).collect();
    let run = state::create_runtime(&spec, lead_session_id, SpecSource::Project)?;
    let run_id = run.team_run_id.clone();
    paths::ensure_base_dirs(&run_id, &member_names)?;

    let cursor = AtomicUsize::new(0);
    let failure: Mutex<Option<TeamError>> = Mutex::new(None);
    let deadline =
        Instant::now() + std::time::Duration::from_secs(run.bounds.max_wall_clock_minutes * 60);
    let worker_count = run
        .bounds
        .max_parallel_members
        .min(spec.members.len())
        .max(1);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    if lock(&failure).is_some() {
                        return;
                    }
                    if Instant::now() > deadline {
                        *lock(&failure) = Some(TeamError::Task(
                            "wall-clock deadline exceeded for team spawn".into(),
                        ));
                        return;
                    }
                    let i = cursor.fetch_add(1, Ordering::SeqCst);
                    let Some(member) = spec.members.get(i) else {
                        return;
                    };
                    let prompt = build_member_prompt(&spec, member, &run_id);
                    match spawner.spawn(&run_id, member, &prompt) {
                        Ok(session_id) => {
                            let _ = state::transition(&run_id, |st| {
                                if let Some(rm) =
                                    st.members.iter_mut().find(|m| m.name == member.name())
                                {
                                    rm.session_id = Some(session_id.clone());
                                    rm.status = MemberStatus::Running;
                                }
                            });
                        }
                        Err(e) => {
                            *lock(&failure) = Some(e);
                            return;
                        }
                    }
                }
            });
        }
    });

    if let Some(e) = failure.into_inner().unwrap_or(None) {
        let _ = delete_team(&run_id); // partial-create cleanup
        return Err(e);
    }

    state::transition(&run_id, |st| st.status = RuntimeStatus::Active)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Build the spawn prompt for a member (port of `buildMemberPrompt`).
pub fn build_member_prompt(spec: &TeamSpec, member: &TeamMemberSpec, run_id: &str) -> String {
    let mut lines = vec![
        format!("Team: {}", spec.name),
        format!("TeamRunId: {run_id}"),
        format!("Member: {}", member.name()),
    ];
    if let Some(wt) = &member.common().worktree_path {
        lines.push(format!("Worktree: {wt}"));
    }
    match member {
        TeamMemberSpec::Category { prompt, .. } => lines.push(prompt.clone()),
        TeamMemberSpec::SubagentType {
            prompt: Some(p), ..
        } => lines.push(p.clone()),
        _ => {}
    }
    lines.join("\n")
}

/// Promote the first member to lead when unset; enforce member-count bounds
/// (port of the `.transform`/`.superRefine` in `TeamSpecSchema`).
pub fn normalize_spec(spec: &mut TeamSpec) -> TeamResult<()> {
    if spec.members.is_empty() {
        return Err(TeamError::InvalidTeamName(
            spec.name.clone(),
            "team needs at least one member".into(),
        ));
    }
    if spec.members.len() > TEAM_MAX_MEMBERS {
        return Err(TeamError::InvalidTeamName(
            spec.name.clone(),
            format!("max {TEAM_MAX_MEMBERS} members"),
        ));
    }
    // Validate every member name for path traversal, control chars, and tmux flag-injection risk.
    for m in &spec.members {
        paths::validate_member_name(m.name())?;
    }
    if spec.lead_agent_id.is_none() {
        spec.lead_agent_id = Some(spec.members[0].name().to_string());
    }
    Ok(())
}

fn find_existing_run(name: &str, lead: &str) -> TeamResult<Option<TeamRuntimeState>> {
    for st in state::list_active_runs()? {
        if st.team_name == name && st.lead_session_id.as_deref() == Some(lead) {
            return Ok(Some(st));
        }
    }
    Ok(None)
}

/// Activate the tmux layout for an existing run. `attach_cmd` builds the pane
/// command for each member (e.g. `jcode attach --team ... --member ...`).
/// No-op outside tmux. Records pane ids + layout into the runtime state.
pub fn activate_team_layout(
    run_id: &str,
    window_target: &str,
    caller_pane: &str,
    attach_cmd: impl Fn(&MemberRuntime) -> String,
) -> TeamResult<()> {
    if !layout::can_visualize() {
        return Ok(());
    }
    let run = state::load_runtime(run_id)?;
    let cmds: Vec<(String, String, String)> = run
        .members
        .iter()
        .map(|m| {
            (
                m.name.clone(),
                attach_cmd(m),
                m.worktree_path.clone().unwrap_or_else(|| ".".to_string()),
            )
        })
        .collect();
    let members: Vec<LayoutMember> = cmds
        .iter()
        .map(|(name, cmd, cwd)| LayoutMember {
            name,
            attach_cmd: cmd,
            cwd,
        })
        .collect();
    let panes = layout::create_team_layout(window_target, caller_pane, &members)?;
    state::transition(run_id, |st| {
        st.tmux_layout = Some(TmuxLayout {
            owned_session: false,
            target_session_id: window_target.to_string(),
            focus_window_id: Some(window_target.to_string()),
            grid_window_id: None,
        });
        for m in st.members.iter_mut() {
            if let Some(p) = panes.get(&m.name) {
                m.tmux_pane_id = Some(p.clone());
            }
        }
    })?;
    Ok(())
}

/// Sweep orphaned `jcode-team-*` tmux sessions not in the current active set.
/// Call before creating a new team (kept separate from `create_team` so the
/// latter stays tmux-free and hermetic).
pub fn sweep_stale_sessions() -> TeamResult<Vec<String>> {
    let active: HashSet<String> = state::list_active_runs()?
        .into_iter()
        .map(|s| s.team_run_id)
        .collect();
    layout::sweep_stale_team_sessions(&active)
}

/// Request shutdown of all members: deliver a `shutdown_request` message to each
/// and mark the run `ShutdownRequested`.
pub fn shutdown_team(run_id: &str) -> TeamResult<()> {
    let run = state::load_runtime(run_id)?;
    let lead = run
        .members
        .iter()
        .find(|m| m.agent_type == MemberAgentType::Leader)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "lead".to_string());
    let active: Vec<String> = run.members.iter().map(|m| m.name.clone()).collect();
    let ctx = mailbox::SendContext::lead(&active, &run.capability_token);
    for m in &run.members {
        if m.name == lead {
            continue;
        }
        let msg = TeamMessage {
            version: 1,
            message_id: uuid::Uuid::new_v4().to_string(),
            from: lead.clone(),
            to: m.name.clone(),
            kind: MessageKind::ShutdownRequest,
            body: "Please wrap up and shut down.".to_string(),
            summary: None,
            references: vec![],
            timestamp: now_millis(),
            correlation_id: None,
            color: None,
        };
        // NOTE: send failure is silently dropped; the run still transitions to
        // ShutdownRequested. A future PR should add logging infrastructure to
        // swarm-core so this surfaces in diagnostics.
        let _ = mailbox::send_message(&msg, run_id, &ctx);
    }
    state::transition(run_id, |st| st.status = RuntimeStatus::ShutdownRequested)?;
    Ok(())
}

/// Tear down a run: remove tmux layout (if any), delete the runtime dir.
///
/// Transitions to `Deleting`, cleans up tmux, persists `Deleted`, then
/// removes the runtime directory. After this call, `load_runtime` returns
/// `NotFound`. The on-disk tombstone is a `deleted.marker` file at the team
/// base so sweeper tools can distinguish "deleted" from "never existed".
pub fn delete_team(run_id: &str) -> TeamResult<()> {
    let _ = state::transition(run_id, |st| st.status = RuntimeStatus::Deleting);
    if let Ok(st) = state::load_runtime(run_id)
        && let Some(layout) = &st.tmux_layout
    {
        let pane_ids: Vec<String> = st
            .members
            .iter()
            .filter_map(|m| m.tmux_pane_id.clone())
            .collect();
        let _ =
            layout::remove_team_layout(layout.owned_session, &layout.target_session_id, &pane_ids);
    }
    // Persist `Deleted` before removing files so the state machine is complete.
    // If the process crashes after this point, `Deleted` is visible on next startup.
    let _ = state::transition(run_id, |st| st.status = RuntimeStatus::Deleted);
    let _ = std::fs::remove_dir_all(paths::runtime_dir(run_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubSpawner;
    impl MemberSpawner for StubSpawner {
        fn spawn(&self, _run: &str, member: &TeamMemberSpec, _prompt: &str) -> TeamResult<String> {
            Ok(format!("sess-{}", member.name()))
        }
    }

    struct FailingSpawner;
    impl MemberSpawner for FailingSpawner {
        fn spawn(&self, _run: &str, _m: &TeamMemberSpec, _p: &str) -> TeamResult<String> {
            Err(TeamError::Task("boom".into()))
        }
    }

    fn spec(name: &str, members: usize) -> TeamSpec {
        let members = (0..members)
            .map(|i| TeamMemberSpec::SubagentType {
                name: format!("m{i}"),
                subagent_type: "sisyphus".into(),
                prompt: None,
                common: MemberCommon::default(),
            })
            .collect();
        TeamSpec {
            version: 1,
            name: name.into(),
            description: None,
            created_at: 0,
            lead_agent_id: None,
            team_allowed_paths: None,
            members,
        }
    }

    #[test]
    fn create_team_spawns_all_members_and_activates() {
        let _base = crate::team::test_support::guarded_base();
        let run = create_team(spec("alpha", 3), "lead-sess", &StubSpawner).unwrap();
        assert_eq!(run.status, RuntimeStatus::Active);
        let loaded = state::load_runtime(&run.team_run_id).unwrap();
        assert_eq!(loaded.members.len(), 3);
        assert!(loaded.members.iter().all(|m| m.session_id.is_some()));
        assert!(
            loaded
                .members
                .iter()
                .all(|m| m.status == MemberStatus::Running)
        );
        // First member promoted to lead.
        assert_eq!(loaded.members[0].agent_type, MemberAgentType::Leader);
    }

    #[test]
    fn create_team_is_idempotent_for_same_name_and_lead() {
        let _base = crate::team::test_support::guarded_base();
        let a = create_team(spec("dup", 2), "same-lead", &StubSpawner).unwrap();
        let b = create_team(spec("dup", 2), "same-lead", &StubSpawner).unwrap();
        assert_eq!(a.team_run_id, b.team_run_id);
    }

    #[test]
    fn create_team_rejects_ineligible_agent() {
        let _base = crate::team::test_support::guarded_base();
        let mut s = spec("ro", 1);
        s.members = vec![TeamMemberSpec::SubagentType {
            name: "reader".into(),
            subagent_type: "oracle".into(),
            prompt: None,
            common: MemberCommon::default(),
        }];
        let err = create_team(s, "lead", &StubSpawner).unwrap_err();
        assert!(matches!(err, TeamError::IneligibleAgent(_, _)));
    }

    #[test]
    fn create_team_over_max_members_rejected() {
        let _base = crate::team::test_support::guarded_base();
        let err = create_team(spec("big", TEAM_MAX_MEMBERS + 1), "lead", &StubSpawner).unwrap_err();
        assert!(matches!(err, TeamError::InvalidTeamName(_, _)));
    }

    #[test]
    fn create_team_cleans_up_on_spawn_failure() {
        let _base = crate::team::test_support::guarded_base();
        let err = create_team(spec("fail", 3), "lead", &FailingSpawner).unwrap_err();
        assert!(matches!(err, TeamError::Task(_)));
        // No active run should remain after cleanup.
        assert!(state::list_active_runs().unwrap().is_empty());
    }

    #[test]
    fn delete_team_marks_deleted_and_removes_dir() {
        let _base = crate::team::test_support::guarded_base();
        let run = create_team(spec("gone", 2), "lead", &StubSpawner).unwrap();
        delete_team(&run.team_run_id).unwrap();
        assert!(!paths::runtime_dir(&run.team_run_id).exists());
        assert!(state::list_active_runs().unwrap().is_empty());
    }

    #[test]
    fn shutdown_team_messages_members_and_sets_status() {
        let _base = crate::team::test_support::guarded_base();
        let run = create_team(spec("shut", 3), "lead", &StubSpawner).unwrap();
        shutdown_team(&run.team_run_id).unwrap();
        let loaded = state::load_runtime(&run.team_run_id).unwrap();
        assert_eq!(loaded.status, RuntimeStatus::ShutdownRequested);
        // Non-lead members each received a shutdown_request message.
        assert_eq!(
            mailbox::list_unread(&run.team_run_id, "m1").unwrap().len(),
            1
        );
        assert_eq!(
            mailbox::list_unread(&run.team_run_id, "m2").unwrap().len(),
            1
        );
    }
}
