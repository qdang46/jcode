//! Team management tools — upgraded to drive `jcode-swarm-core::team` runtime.
//!
//! Replaces the old JSON-file CRUD with the full team-mode: member spawn,
//! file-based mailbox, dependency-aware task board, tmux layout.
//! Backward-compatible: old `~/.jcode/teams/<name>.json` configs are still
//! readable by the status tool.

use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::team::{mailbox, runtime, spec::*, state, tasklist};

// ---------------------------------------------------------------------------
// MemberSpawner: spawns headless jcode agent sessions
// ---------------------------------------------------------------------------

struct JcodeMemberSpawner;

impl runtime::MemberSpawner for JcodeMemberSpawner {
    fn spawn(
        &self,
        run_id: &str,
        member: &TeamMemberSpec,
        prompt: &str,
    ) -> crate::team::spec::TeamResult<String> {
        let member_name = member.name().to_string();
        let session_id = format!("jcode-team-{}-{}", &run_id[..8], member_name);
        crate::logging::info(&format!(
            "spawn team member session run={run_id} member={member_name} session={session_id}"
        ));
        // Spawn a headless jcode server for this team member.
        // The process inherits the parent's PATH and runtime dir access so it
        // can read/write the shared file-based mailbox and task board.
        // NOTE: env vars JCODE_TEAM_RUN_ID, JCODE_TEAM_MEMBER, and
        // JCODE_TEAM_PROMPT are inherited by all spawned subprocesses.
        // The capability token is deliberately NOT included. See security
        // review finding M5 for the accepted disclosure surface.
        let bin = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("jcode"));
        match std::process::Command::new(&bin)
            .arg("serve")
            .arg("--temporary-server")
            .arg("--owner-pid")
            .arg(std::process::id().to_string())
            .env("JCODE_TEAM_RUN_ID", run_id)
            .env("JCODE_TEAM_MEMBER", &member_name)
            .env("JCODE_TEAM_PROMPT", prompt)
            .spawn()
        {
            Ok(child) => {
                // Detach: we do not wait for the child. It runs until the team
                // is shut down or the child process exits on its own.
                let pid = child.id();
                // Detach: forget the child handle so it continues running
                // independently (Child::drop would block waiting).
                std::mem::forget(child);
                crate::logging::info(&format!(
                    "spawned team member pid={pid} run={run_id} member={member_name}",
                ));
                Ok(session_id)
            }
            Err(e) => {
                crate::logging::error(&format!(
                    "failed to spawn team member run={run_id} member={member_name} err={e}"
                ));
                Err(crate::team::spec::TeamError::Tmux(format!(
                    "failed to spawn team member '{member_name}': {e}"
                )))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TeamCreateTool
// ---------------------------------------------------------------------------

pub struct TeamCreateTool;

impl TeamCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamCreateInput {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    members: Vec<Value>,
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Create a multi-agent team. Spawns up to 8 members (max 4 parallel), each in a tmux \
         pane, with a file-based mailbox and a dependency-aware task board."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name", "members"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": {
                    "type": "string",
                    "description": "Team name (alphanumeric, hyphens, underscores)."
                },
                "description": {
                    "type": "string",
                    "description": "What this team is for."
                },
                "members": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 8,
                    "description": "Team members. Each must have name + kind (subagent_type or category).",
                    "items": {
                        "type": "object",
                        "required": ["name", "kind"]
                    }
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamCreateInput = serde_json::from_value(input)?;
        let members: Vec<TeamMemberSpec> = parsed
            .members
            .into_iter()
            .map(serde_json::from_value)
            .collect::<std::result::Result<_, _>>()?;

        let spec = TeamSpec {
            version: 1,
            name: parsed.name.clone(),
            description: parsed.description,
            created_at: chrono::Utc::now().timestamp_millis(),
            lead_agent_id: None,
            team_allowed_paths: None,
            members,
        };

        // Sweep stale tmux sessions from previous runs before creating a new team.
        let _ = runtime::sweep_stale_sessions();

        let session_id = ctx.session_id.clone();
        let spawner = JcodeMemberSpawner;
        let run = tokio::task::spawn_blocking(move || {
            let run = runtime::create_team(spec, &session_id, &spawner)?;
            // Activate tmux layout: read TMUX_PANE / TMUX environment for
            // the caller's window target and pane id. Graceful no-op outside tmux.
            let window_target = std::env::var("TMUX_PANE")
                .or_else(|_| std::env::var("TMUX"))
                .unwrap_or_default();
            if !window_target.is_empty() {
                let caller_pane = window_target.clone();
                runtime::activate_team_layout(
                    &run.team_run_id,
                    &window_target,
                    &caller_pane,
                    |m| {
                        format!(
                            "jcode serve --team-run-id {} --member-name {} 2>/dev/null",
                            run.team_run_id, m.name
                        )
                    },
                )?;
            }
            Ok::<_, crate::team::spec::TeamError>(run)
        })
        .await
        .map_err(|e| anyhow::anyhow!("team creation panicked: {e}"))??;

        Ok(
            ToolOutput::new(serde_json::to_string_pretty(&run)?).with_title(format!(
                "Team '{}' active ({} members)",
                parsed.name,
                run.members.len()
            )),
        )
    }
}

// ---------------------------------------------------------------------------
// TeamDeleteTool
// ---------------------------------------------------------------------------

pub struct TeamDeleteTool;

impl TeamDeleteTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamDeleteInput {
    team_run_id: String,
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        "team_delete"
    }

    fn description(&self) -> &str {
        "Delete a team run by its run ID. Removes tmux panes (if any) and the \
         runtime directory. Also accepts a legacy team name for backward compatibility."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_run_id"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_run_id": {
                    "type": "string",
                    "description": "Team run ID (UUID) or legacy team name."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamDeleteInput = serde_json::from_value(input)?;
        let run_id = parsed.team_run_id;

        // Try as run_id first, then fall back to legacy name lookup
        let target = if let Ok(state) = state::load_runtime(&run_id) {
            state.team_run_id
        } else {
            // Legacy: list all active runs and find one by team name
            let runs = state::list_active_runs()?;
            let found = runs
                .into_iter()
                .find(|r| r.team_name == run_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("no active team found with name or id '{run_id}'")
                })?;
            found.team_run_id
        };

        let target_clone = target.clone();
        tokio::task::spawn_blocking(move || runtime::delete_team(&target_clone))
            .await
            .map_err(|e| anyhow::anyhow!("team deletion panicked: {e}"))??;

        Ok(ToolOutput::new(format!("Team run '{target}' deleted.")).with_title("Team deleted"))
    }
}

// ---------------------------------------------------------------------------
// TeamStatusTool
// ---------------------------------------------------------------------------

pub struct TeamStatusTool;

impl TeamStatusTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamStatusInput {
    team_run_id: Option<String>,
}

#[async_trait]
impl Tool for TeamStatusTool {
    fn name(&self) -> &str {
        "team_status"
    }

    fn description(&self) -> &str {
        "Show status of an active team run, or list all active teams. Includes \
         member states, mailbox sizes, and task counts."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "team_run_id": {
                    "type": "string",
                    "description": "Optional: show status of a specific team run. Omitting lists all active teams."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamStatusInput = serde_json::from_value(input)?;

        let output = if let Some(run_id) = &parsed.team_run_id {
            // Try as run_id first, then fall back to legacy name lookup
            let state = if let Ok(st) = state::load_runtime(run_id) {
                st
            } else {
                let runs = state::list_active_runs()?;
                runs.into_iter()
                    .find(|r| r.team_name == *run_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("no active team found with name or id '{run_id}'")
                    })?
            };
            serde_json::to_string_pretty(&state)?
        } else {
            let runs = state::list_active_runs()?;
            if runs.is_empty() {
                "No active teams.".to_string()
            } else {
                let summaries: Vec<Value> = runs
                    .iter()
                    .map(|r| {
                        json!({
                            "team_run_id": r.team_run_id,
                            "team_name": r.team_name,
                            "status": r.status,
                            "member_count": r.members.len(),
                            "created_at": r.created_at,
                        })
                    })
                    .collect();
                serde_json::to_string_pretty(&summaries)?
            }
        };

        Ok(ToolOutput::new(output).with_title("Team status"))
    }
}

// ---------------------------------------------------------------------------
// TeamSendMessageTool
// ---------------------------------------------------------------------------

pub struct TeamSendMessageTool;

impl TeamSendMessageTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamSendMessageInput {
    team_run_id: String,
    to: String,
    body: String,
    #[serde(default)]
    kind: Option<String>,
}

#[async_trait]
impl Tool for TeamSendMessageTool {
    fn name(&self) -> &str {
        "team_send_message"
    }

    fn description(&self) -> &str {
        "Send a message to a team member via the file-based mailbox."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_run_id", "to", "body"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_run_id": { "type": "string" },
                "to": { "type": "string", "description": "Recipient name, or '*' for broadcast." },
                "body": { "type": "string", "maxLength": 32768 },
                "kind": { "type": "string", "default": "message" }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamSendMessageInput = serde_json::from_value(input)?;
        let run = state::load_runtime(&parsed.team_run_id)?;
        let lead = run
            .members
            .iter()
            .find(|m| m.agent_type == MemberAgentType::Leader)
            .map(|m| m.name.clone())
            .unwrap_or_default();

        let msg = TeamMessage {
            version: 1,
            message_id: uuid::Uuid::new_v4().to_string(),
            from: lead.clone(),
            to: parsed.to,
            kind: MessageKind::Message,
            body: parsed.body,
            summary: None,
            references: vec![],
            timestamp: now_millis(),
            correlation_id: None,
            color: None,
        };

        let active: Vec<String> = run.members.iter().map(|m| m.name.clone()).collect();
        let ctx = mailbox::SendContext::lead(&active, &run.capability_token);
        let result = mailbox::send_message(&msg, &parsed.team_run_id, &ctx)?;

        Ok(ToolOutput::new(format!(
            "Message {} delivered to {} recipient(s): {:?}",
            result.message_id,
            result.delivered_to.len(),
            result.delivered_to,
        )))
    }
}

// ---------------------------------------------------------------------------
// TeamTaskCreateTool
// ---------------------------------------------------------------------------

pub struct TeamTaskCreateTool;

impl TeamTaskCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamTaskCreateInput {
    team_run_id: String,
    subject: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    blocked_by: Vec<String>,
}

#[async_trait]
impl Tool for TeamTaskCreateTool {
    fn name(&self) -> &str {
        "team_task_create"
    }

    fn description(&self) -> &str {
        "Create a task in a team's dependency-aware task board."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_run_id", "subject"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_run_id": { "type": "string" },
                "subject": { "type": "string" },
                "description": { "type": "string" },
                "blocked_by": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs this task depends on."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamTaskCreateInput = serde_json::from_value(input)?;
        let task = tasklist::create_task(
            &parsed.team_run_id,
            tasklist::NewTask {
                subject: parsed.subject,
                description: parsed.description,
                blocks: vec![],
                blocked_by: parsed.blocked_by,
            },
        )?;

        Ok(ToolOutput::new(serde_json::to_string_pretty(&task)?)
            .with_title(format!("Task #{} created", task.id)))
    }
}

// ---------------------------------------------------------------------------
// TeamTaskClaimTool
// ---------------------------------------------------------------------------

pub struct TeamTaskClaimTool;

impl TeamTaskClaimTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamTaskClaimInput {
    team_run_id: String,
    task_id: String,
    member: String,
}

#[async_trait]
impl Tool for TeamTaskClaimTool {
    fn name(&self) -> &str {
        "team_task_claim"
    }

    fn description(&self) -> &str {
        "Claim a pending task. Fails if blocked by incomplete dependencies."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_run_id", "task_id", "member"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_run_id": { "type": "string" },
                "task_id": { "type": "string" },
                "member": { "type": "string" }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamTaskClaimInput = serde_json::from_value(input)?;
        let task = tasklist::claim_task(&parsed.team_run_id, &parsed.task_id, &parsed.member)?;
        Ok(ToolOutput::new(serde_json::to_string_pretty(&task)?)
            .with_title(format!("Task #{} claimed by {}", task.id, parsed.member)))
    }
}

// ---------------------------------------------------------------------------
// TeamTaskListTool
// ---------------------------------------------------------------------------

pub struct TeamTaskListTool;

impl TeamTaskListTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamTaskListInput {
    team_run_id: String,
    #[serde(default)]
    status: Option<String>,
}

#[async_trait]
impl Tool for TeamTaskListTool {
    fn name(&self) -> &str {
        "team_task_list"
    }

    fn description(&self) -> &str {
        "List tasks in a team's task board, optionally filtered by status."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_run_id"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_run_id": { "type": "string" },
                "status": {
                    "type": "string",
                    "description": "Filter: pending | claimed | in_progress | completed"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamTaskListInput = serde_json::from_value(input)?;
        let status_filter = parsed.status.as_deref().and_then(|s| {
            Some(match s {
                "pending" => TaskStatus::Pending,
                "claimed" => TaskStatus::Claimed,
                "in_progress" => TaskStatus::InProgress,
                "completed" => TaskStatus::Completed,
                _ => return None,
            })
        });
        let tasks = tasklist::list_tasks(&parsed.team_run_id, status_filter, None)?;
        Ok(ToolOutput::new(serde_json::to_string_pretty(&tasks)?)
            .with_title(format!("{} tasks", tasks.len())))
    }
}

// ---------------------------------------------------------------------------
// TeamShutdownTool
// ---------------------------------------------------------------------------

pub struct TeamShutdownTool;

impl TeamShutdownTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamShutdownInput {
    team_run_id: String,
}

#[async_trait]
impl Tool for TeamShutdownTool {
    fn name(&self) -> &str {
        "team_shutdown"
    }

    fn description(&self) -> &str {
        "Request orderly shutdown of all team members. Delivers a shutdown_request \
         message to each non-lead member and marks the run ShutdownRequested."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_run_id"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_run_id": { "type": "string" }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamShutdownInput = serde_json::from_value(input)?;
        runtime::shutdown_team(&parsed.team_run_id)?;
        Ok(ToolOutput::new(format!(
            "Team run '{}' shutdown requested.",
            parsed.team_run_id
        )))
    }
}
