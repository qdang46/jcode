//! Team-mode core types — a faithful Rust port of oh-my-openagent's
//! `src/features/team-mode/types.ts` (Zod schemas).
//!
//! See `.claude/skills/feature-planning/plans/issue-390-tmux-team-viz.md` §3.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants (RuntimeBoundsSchema defaults + message cap)
// ---------------------------------------------------------------------------

/// Hard ceiling on team size (`RuntimeBoundsSchema.maxMembers` default = 8).
pub const TEAM_MAX_MEMBERS: usize = 8;
/// Members allowed to run concurrently during spawn (`maxParallelMembers` = 4).
pub const TEAM_MAX_PARALLEL: usize = 4;
/// Mailbox message ceiling per run before pruning (`maxMessagesPerRun` = 10_000).
pub const TEAM_MAX_MESSAGES_PER_RUN: usize = 10_000;
/// Wall-clock budget for an entire team run (`maxWallClockMinutes` = 120).
pub const TEAM_MAX_WALL_CLOCK_MINUTES: u64 = 120;
/// Per-member turn ceiling (`maxMemberTurns` = 500).
pub const TEAM_MAX_TURNS_PER_MEMBER: usize = 500;
/// Message body hard cap — `body: z.string().max(32 * 1024)`.
pub const TEAM_MESSAGE_MAX_BYTES: usize = 32 * 1024;
/// Default per-recipient unread backpressure ceiling (configurable).
pub const TEAM_RECIPIENT_UNREAD_MAX_BYTES: usize = 10 * 1024 * 1024; // 10 MiB

fn default_version() -> u8 {
    1
}
fn default_true() -> bool {
    true
}

/// Current epoch milliseconds (UTC).
pub fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ---------------------------------------------------------------------------
// Team spec & members
// ---------------------------------------------------------------------------

/// A team definition (port of `TeamSpecSchema`). `version` pins the on-disk schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSpec {
    #[serde(default = "default_version")]
    pub version: u8,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "now_millis")]
    pub created_at: i64,
    /// Lead member name. If absent, the first member is promoted (see `normalize_spec`).
    #[serde(default)]
    pub lead_agent_id: Option<String>,
    #[serde(default)]
    pub team_allowed_paths: Option<Vec<String>>,
    pub members: Vec<TeamMemberSpec>,
}

/// One configured member. `kind` discriminates how the agent is resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamMemberSpec {
    /// User-defined category mapped to a prompt (`CategoryMemberSchema`).
    Category {
        name: String,
        category: String,
        prompt: String,
        #[serde(flatten)]
        common: MemberCommon,
    },
    /// Built-in subagent type (`SubagentMemberSchema`).
    SubagentType {
        name: String,
        subagent_type: String,
        #[serde(default)]
        prompt: Option<String>,
        #[serde(flatten)]
        common: MemberCommon,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemberCommon {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub subscriptions: Vec<String>,
    #[serde(default)]
    pub backend_type: BackendType,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BackendType {
    InProcess,
    /// Default for Issue #390 (split-pane visualization).
    #[default]
    Tmux,
}

impl TeamMemberSpec {
    pub fn name(&self) -> &str {
        match self {
            Self::Category { name, .. } | Self::SubagentType { name, .. } => name,
        }
    }

    pub fn common(&self) -> &MemberCommon {
        match self {
            Self::Category { common, .. } | Self::SubagentType { common, .. } => common,
        }
    }

    /// The agent type used for eligibility checks. Category members run on the
    /// default worker; subagent members declare their concrete agent type.
    pub fn agent_type(&self) -> &str {
        match self {
            Self::SubagentType { subagent_type, .. } => subagent_type,
            Self::Category { .. } => "sisyphus",
        }
    }
}

// ---------------------------------------------------------------------------
// Messages (port of MessageSchema)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessage {
    pub version: u8,
    pub message_id: String,
    pub from: String,
    /// Recipient name, or `"*"` for broadcast (lead only).
    pub to: String,
    pub kind: MessageKind,
    pub body: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub references: Vec<TeamReference>,
    pub timestamp: i64,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Message,
    ShutdownRequest,
    ShutdownApproved,
    ShutdownRejected,
    Announcement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamReference {
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Tasks (port of TaskSchema)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    pub version: u8,
    pub id: String,
    pub subject: String,
    pub description: String,
    #[serde(default)]
    pub active_form: Option<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub claimed_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Claimed,
    InProgress,
    Completed,
    Deleted,
}

// ---------------------------------------------------------------------------
// Runtime state (port of RuntimeStateSchema)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRuntimeState {
    pub version: u8,
    /// UUID v4 — also the tmux session suffix (`jcode-team-{team_run_id}`).
    pub team_run_id: String,
    pub team_name: String,
    pub spec_source: SpecSource,
    pub created_at: i64,
    pub status: RuntimeStatus,
    #[serde(default)]
    pub lead_session_id: Option<String>,
    #[serde(default)]
    pub tmux_layout: Option<TmuxLayout>,
    pub members: Vec<MemberRuntime>,
    #[serde(default)]
    pub shutdown_requests: Vec<ShutdownRequest>,
    pub bounds: RuntimeBounds,
    /// Per-run shared secret. Required to be presented on every mailbox
    /// API call (send/list/ack) to prevent cross-team access by local
    /// processes that happen to know the run_id UUID. Generated at create
    /// time, persisted in `state.json` with `0o600` file mode.
    #[serde(default)]
    pub capability_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecSource {
    Project,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Creating,
    Active,
    ShutdownRequested,
    Deleting,
    Deleted,
    Failed,
    Orphaned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBounds {
    #[serde(default = "RuntimeBounds::default_max_members")]
    pub max_members: usize,
    #[serde(default = "RuntimeBounds::default_max_parallel")]
    pub max_parallel_members: usize,
    #[serde(default = "RuntimeBounds::default_max_messages")]
    pub max_messages_per_run: usize,
    #[serde(default = "RuntimeBounds::default_wall_clock")]
    pub max_wall_clock_minutes: u64,
    #[serde(default = "RuntimeBounds::default_max_turns")]
    pub max_member_turns: usize,
}

impl RuntimeBounds {
    fn default_max_members() -> usize {
        TEAM_MAX_MEMBERS
    }
    fn default_max_parallel() -> usize {
        TEAM_MAX_PARALLEL
    }
    fn default_max_messages() -> usize {
        TEAM_MAX_MESSAGES_PER_RUN
    }
    fn default_wall_clock() -> u64 {
        TEAM_MAX_WALL_CLOCK_MINUTES
    }
    fn default_max_turns() -> usize {
        TEAM_MAX_TURNS_PER_MEMBER
    }
}

impl Default for RuntimeBounds {
    fn default() -> Self {
        Self {
            max_members: TEAM_MAX_MEMBERS,
            max_parallel_members: TEAM_MAX_PARALLEL,
            max_messages_per_run: TEAM_MAX_MESSAGES_PER_RUN,
            max_wall_clock_minutes: TEAM_MAX_WALL_CLOCK_MINUTES,
            max_member_turns: TEAM_MAX_TURNS_PER_MEMBER,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxLayout {
    /// Did we create the session (vs. split the caller's window)?
    pub owned_session: bool,
    /// `jcode-team-{team_run_id}` when owned, else the caller's session id.
    pub target_session_id: String,
    #[serde(default)]
    pub focus_window_id: Option<String>,
    #[serde(default)]
    pub grid_window_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberRuntime {
    pub name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub tmux_pane_id: Option<String>,
    pub agent_type: MemberAgentType,
    #[serde(default)]
    pub subagent_type: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    pub status: MemberStatus,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub last_injected_turn_marker: Option<String>,
    #[serde(default)]
    pub pending_injected_message_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemberAgentType {
    Leader,
    GeneralPurpose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Pending,
    Running,
    Idle,
    Errored,
    Completed,
    ShutdownApproved,
}

impl MemberStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Idle => "idle",
            Self::Errored => "errored",
            Self::Completed => "completed",
            Self::ShutdownApproved => "shutdown_approved",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownRequest {
    pub member_id: String,
    pub requester_name: String,
    pub requested_at: i64,
    #[serde(default)]
    pub approved_at: Option<i64>,
    #[serde(default)]
    pub rejected_reason: Option<String>,
    #[serde(default)]
    pub rejected_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TeamError {
    #[error("team '{0}' already has an active run")]
    AlreadyActive(String),
    #[error("team run '{0}' not found")]
    NotFound(String),
    #[error("team is deleting/deleted; messages rejected")]
    TeamDeleting,
    #[error("broadcast requires lead role")]
    BroadcastNotPermitted,
    #[error("payload exceeds the 32 KiB message size limit")]
    PayloadTooLarge,
    #[error("recipient inbox full (backpressure)")]
    RecipientBackpressure,
    #[error("duplicate message id {0}")]
    DuplicateMessageId(String),
    #[error("agent '{0}' is not eligible to be a team member: {1}")]
    IneligibleAgent(String, String),
    #[error("invalid team name '{0}': {1}")]
    InvalidTeamName(String, String),
    #[error("task error: {0}")]
    Task(String),
    #[error("lock timeout acquiring {0}")]
    LockTimeout(String),
    #[error(
        "schema version {found} for run '{run_id}' is not supported (expected {expected}); a migration is required"
    )]
    UnsupportedSchemaVersion {
        run_id: String,
        found: u8,
        expected: u8,
    },
    #[error("mailbox auth failed")]
    MailboxAuthFailed(String),
    #[error("invalid member name '{0}': {1}")]
    InvalidMemberName(String, String),
    #[error("tmux error: {0}")]
    Tmux(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type TeamResult<T> = Result<T, TeamError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_spec_roundtrips_through_json() {
        let json = r#"{
            "name": "refactor-auth",
            "members": [
                {"kind": "subagent_type", "name": "lead", "subagent_type": "sisyphus"},
                {"kind": "category", "name": "ui", "category": "visual", "prompt": "do ui"}
            ]
        }"#;
        let spec: TeamSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.name, "refactor-auth");
        assert_eq!(spec.version, 1);
        assert_eq!(spec.members.len(), 2);
        assert_eq!(spec.members[0].name(), "lead");
        assert_eq!(spec.members[0].agent_type(), "sisyphus");
        assert_eq!(spec.members[1].agent_type(), "sisyphus"); // category -> default worker
        // Re-serialize and parse again to confirm stability.
        let s = serde_json::to_string(&spec).unwrap();
        let again: TeamSpec = serde_json::from_str(&s).unwrap();
        assert_eq!(again.members.len(), 2);
    }

    #[test]
    fn message_kind_serializes_snake_case() {
        let v = serde_json::to_string(&MessageKind::ShutdownRequest).unwrap();
        assert_eq!(v, "\"shutdown_request\"");
    }

    #[test]
    fn runtime_bounds_default_matches_constants() {
        let b = RuntimeBounds::default();
        assert_eq!(b.max_members, TEAM_MAX_MEMBERS);
        assert_eq!(b.max_parallel_members, TEAM_MAX_PARALLEL);
        assert_eq!(b.max_member_turns, TEAM_MAX_TURNS_PER_MEMBER);
    }

    #[test]
    fn backend_type_defaults_to_tmux() {
        assert_eq!(BackendType::default(), BackendType::Tmux);
    }
}
