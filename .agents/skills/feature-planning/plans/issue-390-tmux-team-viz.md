# Implementation Plan: Tmux Team Visualization (Issue #390)

> Generated from research across 9 reference repos + full jcode codebase analysis.
> **Goal:** Port oh-my-openagent's `team-mode` to jcode's Rust/TUI stack — multi-agent
> coordination with per-member tmux panes, a file-based mailbox, a dependency-aware task
> board, automatic rebalancing, stale-session sweeping, and a live TUI team widget.

> **Stack:** Rust 2024 edition. New code lands in `crates/jcode-swarm-core` (logic),
> `crates/jcode-app-core` (tools + server state), `crates/jcode-tui` (widget).
> **Backend (Phase 1):** tmux split-panes. In-process backend deferred to v2.

---

## 1. Executive Summary

Issue #390 (Gap #28, Medium) asks for a tmux-based visualization layer for multi-agent
team orchestration. The lead agent spawns up to **8 members** (max **4 running in
parallel**), each in its own tmux pane. Members coordinate through a **file-based mailbox**
(`send`/`inbox`/`poll`/`ack`), claim work from a **task board** with dependency tracking
(`pending → claimed → in_progress → completed`), and the system **rebalances** panes when
membership changes and **sweeps stale** tmux sessions left behind by crashes.

The proven reference is **oh-my-openagent `src/features/team-mode/`** (TypeScript): it has
every component we need — tmux layout, mailbox, task list, runtime lifecycle, state store
with atomic file locks. **Claude Code's Agent Teams** contributes UI conventions
(per-teammate color assignment, split-pane vs in-process modes, keyboard navigation).

jcode already ships the scaffolding we build on: `SwarmState` / `SwarmMember` /
`SwarmRuntime` (server), `SwarmRole` / `SwarmLifecycleStatus` / `SwarmMemberRecord` /
`ChannelIndex` (swarm-core), a minimal `team.rs` CRUD tool, and a text-only `SwarmStatus`
info widget. We **port the file-based design to Rust** (not a literal translation), wire it
into the existing `SwarmState`, and add a graph-style TUI widget.

**Outcome:** an agent can say "create a team to refactor the auth module with 3 workers" and
jcode spins up isolated tmux panes, distributes tasks, routes inter-agent messages durably,
and renders a live roster + task DAG — all crash-resilient and bounded.

---

## 2. Architecture Decision

### Chosen Approach

**Port oh-my-openagent's file-based team-mode to Rust**, reusing jcode's `SwarmState` for
live session tracking while persisting team-specific state (mailbox, tasks, runtime, tmux
layout) as files under `~/.jcode/teams/`.

Why:
- **Completeness** — oh-my-openagent already implements *all five* Issue #390 requirements
  (panes, mailbox, task board, rebalancing, stale sweep) and ships tests for each.
- **Crash resilience** — file-based state survives process death; a hung member never
  corrupts shared state. This matches jcode's existing daemon-snapshot philosophy
  (see `docs/SWARM_ARCHITECTURE.md`: "Swarm runtime state survives reloads and crash
  recovery via daemon snapshots").
- **Portability** — the design relies only on the filesystem + tmux CLI, both of which Rust
  handles with `std::fs` and `std::process::Command`. No new heavy runtime dependency.
- **Language-agnostic IPC** — file-based mailbox lets headless members (spawned via
  `Command`) and TUI-attached members interoperate without a shared in-memory bus.

### Alternatives Considered

| Approach | Source | Pros | Cons | Decision |
|----------|--------|------|------|----------|
| File-based team-mode | oh-my-openagent | Complete, tested, crash-resilient, tmux-native | TS→Rust port effort | **Chosen** |
| In-process spawn + IPC channels | Claude Code (`in-process` mode) | No tmux dependency; simplest happy path | Loses the *visualization* that #390 is about; no durable mailbox | Rejected (revisit as v2 backend) |
| Reuse jcode `ChannelIndex` for messaging | jcode swarm-core | Reuses existing code | In-memory only — no persistence, no backpressure, no dedup; not built for 32 KB payloads | Rejected for mailbox; keep for live channel subs |
| Electron/Bubble Tea dashboard | hivemind / agent-dashboard | Rich GUI | Wrong stack; jcode is a Rust TUI; defeats "lives in the terminal" goal | Rejected |
| Pure-bash tmux orchestrator | twaldin/tmux-orchestrator | Zero deps | No type safety, no task DAG, no TUI integration | Rejected (informs tmux command shapes only) |

### Key divergences from the TS reference

1. **State store** — oh-my-openagent uses `withLock` (exclusive-create lockfiles) + atomic
   temp-write+rename. We reproduce this exactly in Rust with `OpenOptions::create_new(true)`
   and `fs::rename` (atomic on the same volume).
2. **Member spawn** — oh-my-openagent calls its `BackgroundManager`. jcode spawns headless
   members via `std::process::Command::new("jcode")` with `serve`/`attach` semantics, and
   registers them in `SwarmState` exactly like existing swarm members.
3. **Visualization** — oh-my-openagent *only* draws tmux panes. jcode additionally renders a
   `WidgetKind::TeamView` info widget (roster + task DAG) because jcode owns its TUI.

---

## 3. Data Structures & Types

All new types live in `crates/jcode-swarm-core/src/team/spec.rs`. They are faithful Rust
ports of `oh-my-openagent/src/features/team-mode/types.ts` (the Zod schemas).

### 3.1 Constants (port of `types.ts` bounds)

```rust
// crates/jcode-swarm-core/src/team/spec.rs

/// Hard ceiling on team size (RuntimeBoundsSchema.maxMembers default = 8).
pub const TEAM_MAX_MEMBERS: usize = 8;
/// Members allowed to run concurrently during spawn (maxParallelMembers = 4).
pub const TEAM_MAX_PARALLEL: usize = 4;
/// Mailbox message ceiling per run before pruning (maxMessagesPerRun = 10_000).
pub const TEAM_MAX_MESSAGES_PER_RUN: usize = 10_000;
/// Wall-clock budget for an entire team run (maxWallClockMinutes = 120).
pub const TEAM_MAX_WALL_CLOCK_MINUTES: u64 = 120;
/// Per-member turn ceiling (maxMemberTurns = 500).
pub const TEAM_MAX_TURNS_PER_MEMBER: usize = 500;
/// Message body hard cap — `body: z.string().max(32 * 1024)`.
pub const TEAM_MESSAGE_MAX_BYTES: usize = 32 * 1024;
/// Default per-recipient unread backpressure ceiling (configurable).
pub const TEAM_RECIPIENT_UNREAD_MAX_BYTES: usize = 10 * 1024 * 1024; // 10 MiB
/// Stale-reservation reclaim TTL for in-flight deliveries.
pub const TEAM_RESERVATION_STALE_MS: u64 = 60_000;
```

### 3.2 Team spec & members

```rust
use serde::{Deserialize, Serialize};

/// A team definition (port of TeamSpecSchema). `version` pins the on-disk schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSpec {
    #[serde(default = "default_version")]
    pub version: u8, // == 1
    pub name: String,            // ^[a-z0-9-]+$
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "now_millis")]
    pub created_at: i64,
    /// Lead member name. If absent, the first member is promoted (see `normalize`).
    #[serde(default)]
    pub lead_agent_id: Option<String>,
    #[serde(default)]
    pub team_allowed_paths: Option<Vec<String>>,
    pub members: Vec<TeamMemberSpec>, // 1..=TEAM_MAX_MEMBERS
}

fn default_version() -> u8 { 1 }
fn now_millis() -> i64 { chrono::Utc::now().timestamp_millis() }

/// One configured member. `kind` discriminates how the agent is resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamMemberSpec {
    /// User-defined category mapped to a prompt (CategoryMemberSchema).
    Category {
        name: String,                  // ^[a-z0-9-]+$
        category: String,
        prompt: String,
        #[serde(flatten)]
        common: MemberCommon,
    },
    /// Built-in subagent type (SubagentMemberSchema).
    SubagentType {
        name: String,
        subagent_type: String,         // validated against eligibility registry
        #[serde(default)]
        prompt: Option<String>,
        #[serde(flatten)]
        common: MemberCommon,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberCommon {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub subscriptions: Vec<String>,
    #[serde(default)]
    pub backend_type: BackendType,     // default in-process per schema; we default Tmux for #390
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BackendType {
    InProcess,
    #[default]
    Tmux,
}

impl TeamMemberSpec {
    pub fn name(&self) -> &str {
        match self { Self::Category { name, .. } | Self::SubagentType { name, .. } => name }
    }
    pub fn common(&self) -> &MemberCommon {
        match self { Self::Category { common, .. } | Self::SubagentType { common, .. } => common }
    }
}
```

### 3.3 Messages (port of `MessageSchema`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessage {
    pub version: u8,                   // == 1
    pub message_id: String,            // UUID v4
    pub from: String,                  // sender member name
    pub to: String,                    // recipient name, or "*" for broadcast (lead only)
    pub kind: MessageKind,
    pub body: String,                  // <= TEAM_MESSAGE_MAX_BYTES
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub references: Vec<TeamReference>,
    pub timestamp: i64,                // epoch millis
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
```

### 3.4 Tasks (port of `TaskSchema`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamTask {
    pub version: u8,                   // == 1
    pub id: String,                    // "1", "2", ... high-watermark
    pub subject: String,
    pub description: String,
    #[serde(default)]
    pub active_form: Option<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub owner: Option<String>,         // member name
    #[serde(default)]
    pub blocks: Vec<String>,           // task IDs this one blocks
    #[serde(default)]
    pub blocked_by: Vec<String>,       // task IDs that must finish first
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
```

### 3.5 Runtime state (port of `RuntimeStateSchema`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRuntimeState {
    pub version: u8,                   // == 1
    pub team_run_id: String,           // UUID v4 — also the tmux session suffix
    pub team_name: String,
    pub spec_source: SpecSource,       // Project | User
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecSource { Project, User }

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
    fn default_max_members() -> usize { TEAM_MAX_MEMBERS }
    fn default_max_parallel() -> usize { TEAM_MAX_PARALLEL }
    fn default_max_messages() -> usize { TEAM_MAX_MESSAGES_PER_RUN }
    fn default_wall_clock() -> u64 { TEAM_MAX_WALL_CLOCK_MINUTES }
    fn default_max_turns() -> usize { TEAM_MAX_TURNS_PER_MEMBER }
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
    pub owned_session: bool,           // did we create the session (vs. split caller window)?
    pub target_session_id: String,     // "jcode-team-{team_run_id}" when owned
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
    pub agent_type: MemberAgentType,   // Leader | GeneralPurpose
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
pub enum MemberAgentType { Leader, GeneralPurpose }

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
```

### 3.6 Error type

```rust
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
    #[error("payload exceeds {TEAM_MESSAGE_MAX_BYTES} bytes")]
    PayloadTooLarge,
    #[error("recipient inbox full (backpressure)")]
    RecipientBackpressure,
    #[error("duplicate message id {0}")]
    DuplicateMessageId(String),
    #[error("agent '{0}' is not eligible to be a team member: {1}")]
    IneligibleAgent(String, String),
    #[error("invalid team name '{0}': {1}")]
    InvalidTeamName(String, String),
    #[error("lock timeout acquiring {0}")]
    LockTimeout(String),
    #[error("tmux error: {0}")]
    Tmux(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type TeamResult<T> = Result<T, TeamError>;
```

---

## 4. Pseudocode — Core Algorithms

### 4.1 Team creation (port of `team-runtime/create.ts`)

```
FUNCTION create_team(spec, lead_session_id):
    normalize(spec)                       # promote first member to lead if none set
    validate_team_name(spec.name)         # ^[a-z0-9-]+$, no traversal
    FOR member IN spec.members:
        verdict = eligibility(member.agent_type)
        IF verdict == HardReject: RETURN Err(IneligibleAgent)

    existing = find_existing_runtime(spec.name, lead_session_id)
    IF existing AND existing.status IN {Creating, Active}: RETURN existing   # idempotent

    active_ids = list_active_team_run_ids()
    sweep_stale_team_sessions(active_ids)  # best-effort; ignore failures

    ensure_base_dirs()                     # ~/.jcode/teams/{run}/{inbox,tasks,tasks/claims}
    run = create_runtime_state(spec, lead_session_id, source)  # status=Creating
    register_team_run_for_cleanup(run.team_run_id)

    deadline = now + bounds.max_wall_clock_minutes * 60_000
    resources = empty per-member resource slots
    next = 0
    FAILURE = none

    # Bounded parallel spawn: at most max_parallel workers pull from a shared index.
    SPAWN max_parallel WORKERS, each loops:
        WHILE FAILURE is none:
            i = atomic_fetch_add(next)
            IF i >= members.len: BREAK
            member = members[i]
            IF now > deadline: FAILURE = WallClockExceeded; BREAK
            TRY:
                IF member.worktree_path: resources[i].worktree = mkdir(member.worktree_path)
                resolved = resolve_member(member)             # agent, model, prompt
                child = spawn_headless_session(prompt=build_member_prompt(...),
                                               agent=resolved.agent,
                                               parent=lead_session_id,
                                               team_run_id=run.id)
                session_id = wait_for_session_id(child, deadline)
                transition(run, set members[i].session_id, status=Running)
            CATCH e:
                FAILURE = e

    IF FAILURE is not none:
        cleanup_team_run_resources(run, resources)            # kill children, rm layout/worktrees
        RETURN Err(FAILURE)

    assert_no_unresolved_members(run)
    layout = activate_team_layout(run)                        # see 4.2
    transition(run, status=Active)
    RETURN run
```

### 4.2 Tmux layout creation (port of `team-layout-tmux/layout.ts`)

```
FUNCTION create_team_layout(run):
    IF env var TMUX not set: RETURN None              # canVisualize() == false
    IF run.members empty: RETURN None
    tmux = resolve_tmux_path()  ; IF none: RETURN None

    caller = resolve_caller_tmux_session(tmux)        # {pane_id, window_target, session_id}
    IF none: RETURN None

    panes = {}
    existing = list_panes_in_window(caller.window_target)
    teammates = existing \ {caller.pane_id}

    FOR member IN run.members:
        IF teammates empty:
            args = split-window -t caller.pane_id -h -d -l 70% -P -F '#{pane_id}' -c <cwd>
        ELSE:
            anchor = teammates[len/2]                 # split the middle pane
            direction = (len(teammates) odd) ? '-v' : '-h'   # alternate
            args = split-window -t anchor direction -d -P -F '#{pane_id}' -c <cwd>
        pane_id = run_tmux(args).trim()
        teammates.push(pane_id) ; panes[member.name] = pane_id
        run_tmux(select-pane -t pane_id -T member.name)              # set title
        run_tmux(send-keys  -t pane_id  attach_command(member)  Enter)

    run_tmux(select-layout -t caller.window_target main-vertical)
    run_tmux(resize-pane   -t caller.pane_id -x 30%)
    RETURN TmuxLayout{ owned_session:false, target_session_id:caller.session_id,
                       focus_window_id:caller.window_target, panes }
```

### 4.3 Send message with backpressure + dedup (port of `team-mailbox/send.ts`)

```
FUNCTION send_message(msg, run_id, ctx):
    serialized = pretty_json(msg) + "\n"
    IF byte_len(msg.body) > MESSAGE_MAX_BYTES: RETURN Err(PayloadTooLarge)
    state = load_runtime(run_id)
    IF state.status IN {Deleting, Deleted}: RETURN Err(TeamDeleting)
    IF msg.to == "*" AND NOT ctx.is_lead: RETURN Err(BroadcastNotPermitted)

    recipients = (msg.to == "*") ? unique(ctx.active_members) : [msg.to]
    delivered = []
    FOR r IN recipients:
        inbox = inbox_dir(run_id, r) ; mkdir(inbox, 0o700)
        WITH_LOCK(inbox + ".lock"):
            unread_bytes = sum file sizes of unread (*.json and .delivering-*.json)
            IF unread_bytes + byte_len(serialized) > recipient_unread_max:
                RETURN Err(RecipientBackpressure)
            IF exists(inbox/{id}.json) OR exists(inbox/.delivering-{id}.json):
                RETURN Err(DuplicateMessageId)
            target = ctx.reserved.contains(r) ? ".delivering-{id}.json" : "{id}.json"
            atomic_write(inbox/target, serialized)
            delivered.push(r)
    RETURN { message_id: msg.id, delivered_to: delivered }
```

### 4.4 Atomic file lock (port of `team-state-store/locks.ts`)

```
FUNCTION with_lock(lock_path, owner_tag, stale_ms, body):
    start = now
    LOOP:
        IF now - start > LOCK_WAIT_TIMEOUT (15s): RETURN Err(LockTimeout)
        TRY open(lock_path, create_new=true):       # O_EXCL — atomic
            write "{owner_tag}\n{pid}\n{now}\n" ; fsync ; close
            BREAK
        CATCH AlreadyExists:
            IF detect_stale(lock_path, stale_ms):    # owner pid dead AND age>stale
                remove(lock_path) ; CONTINUE
            sleep(50ms)
    result = body()                                  # run critical section
    remove(lock_path)                                # release (best-effort)
    RETURN result
```

### 4.5 Stale session sweep (port of `sweep-stale-team-sessions.ts`)

```
FUNCTION sweep_stale_team_sessions(active_run_ids):
    IF env TMUX not set: RETURN []
    sessions = tmux list-sessions -F '#{session_name}'
    killed = []
    FOR s IN sessions:
        m = regex_match(s, "^jcode-team-(<uuid>)$")
        IF m AND m.group(1) NOT IN active_run_ids:
            tmux kill-session -t s ; killed.push(s)
    RETURN killed
```

### 4.6 Task claim (port of `team-tasklist/claim.ts` + `store.ts`)

```
FUNCTION create_task(run_id, input):
    dir = tasks_dir(run_id) ; mkdir(dir, dir/claims)
    WITH_LOCK(dir + "/.lock"):
        n = read_high_watermark(dir + "/.highwatermark") + 1   # 0 on missing/corrupt
        atomic_write(dir + "/.highwatermark", str(n))
        task = Task{ id:str(n), status:Pending, created_at:now, updated_at:now, ...input }
        atomic_write(dir + "/{n}.json", pretty_json(task))
    RETURN task

FUNCTION claim_task(run_id, task_id, member):
    WITH_LOCK(tasks_dir/claims/{task_id}.lock):
        task = read_task(task_id)
        IF task.status != Pending: RETURN Err(AlreadyClaimed)
        IF any(blocked_by where dep.status != Completed): RETURN Err(Blocked)
        task.status = Claimed ; task.owner = member ; task.claimed_at = now
        atomic_write(task_file, pretty_json(task))
    RETURN task
```

---

## 5. Implementation Code

> New module tree (under `crates/jcode-swarm-core/src/`):
> ```
> team/
>   mod.rs          // re-exports
>   spec.rs         // §3 types + constants + TeamError
>   paths.rs        // base dirs, inbox/tasks paths, validate_team_name
>   locks.rs        // with_lock, atomic_write, read_json
>   eligibility.rs  // AGENT_ELIGIBILITY_REGISTRY
>   mailbox.rs      // send / inbox / poll / ack / reservation
>   tasklist.rs     // create / claim / update / list / dependencies
>   layout.rs       // tmux create / remove / rebalance / sweep
>   state.rs        // load/create/transition runtime state
>   runtime.rs      // create_team / delete_team / shutdown_team / status
> ```

### 5.1 `team/locks.rs` — atomic primitives (port of `locks.ts`)

```rust
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use crate::team::spec::{TeamError, TeamResult};

const LOCK_RETRY: Duration = Duration::from_millis(50);
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_STALE: Duration = Duration::from_secs(300);

fn epoch_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

/// Is `pid` alive? `kill(pid, 0)` returns Ok if the process exists.
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // SAFETY: signal 0 performs error checking without sending a signal.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}
#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool { true } // conservative on non-unix

fn detect_stale(lock_path: &Path, stale: Duration) -> bool {
    let Ok(content) = fs::read_to_string(lock_path) else { return false };
    let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    if lines.len() != 3 { return false; }
    let (Ok(pid), Ok(acquired)) = (lines[1].parse::<u32>(), lines[2].parse::<u128>())
        else { return false };
    if pid_alive(pid) { return false; }
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
        match OpenOptions::new().write(true).create_new(true).open(lock_path) {
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
        Err(e) => { let _ = fs::remove_file(&tmp); Err(TeamError::Io(e)) }
    }
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> TeamResult<T> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}
```

### 5.2 `team/paths.rs` — layout + name validation (port of `paths.ts`)

```rust
use std::path::{Path, PathBuf};
use crate::team::spec::{TeamError, TeamResult};

/// `~/.jcode/teams` — the team base directory.
pub fn teams_base_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        .join(".jcode").join("teams")
}
pub fn runtime_dir(run_id: &str) -> PathBuf { teams_base_dir().join("runtime").join(run_id) }
pub fn runtime_state_path(run_id: &str) -> PathBuf { runtime_dir(run_id).join("state.json") }
pub fn inbox_dir(run_id: &str, member: &str) -> PathBuf {
    runtime_dir(run_id).join("inboxes").join(member)
}
pub fn tasks_dir(run_id: &str) -> PathBuf { runtime_dir(run_id).join("tasks") }
pub fn worktree_dir(run_id: &str, member: &str) -> PathBuf {
    teams_base_dir().join("worktrees").join(run_id).join(member)
}

/// Create base dirs with 0o700 (mirrors ensureBaseDirs).
pub fn ensure_base_dirs(run_id: &str, members: &[String]) -> TeamResult<()> {
    use std::fs;
    for d in [teams_base_dir(), runtime_dir(run_id), tasks_dir(run_id),
              tasks_dir(run_id).join("claims")] {
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

/// Reject empty names, traversal, and non-`[a-z0-9_-]` characters.
pub fn validate_team_name(name: &str) -> TeamResult<()> {
    if name.is_empty() {
        return Err(TeamError::InvalidTeamName(name.into(), "empty".into()));
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(TeamError::InvalidTeamName(name.into(), "path traversal".into()));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(TeamError::InvalidTeamName(
            name.into(), "only [a-z0-9_-] allowed".into()));
    }
    Ok(())
}
```

### 5.3 `team/eligibility.rs` — agent registry (port of `AGENT_ELIGIBILITY_REGISTRY`)

```rust
/// Verdict for using an agent type as a *team member* (must be able to write the mailbox).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility { Eligible, Conditional, HardReject }

/// Mirrors oh-my-openagent's registry. Read-only agents are hard-rejected because team
/// members must write JSON files to peer inboxes.
pub fn eligibility(agent_type: &str) -> (Eligibility, &'static str) {
    match agent_type {
        "sisyphus" | "sisyphus-junior" | "atlas" => (Eligibility::Eligible, ""),
        "hephaestus" => (Eligibility::Conditional,
            "grant teammate permission or use sisyphus instead"),
        "oracle" | "librarian" | "explore" | "multimodal-looker"
        | "metis" | "momus" | "prometheus" => (Eligibility::HardReject,
            "agent is read-only; cannot write to mailbox. Use delegate-task instead."),
        // jcode-native default worker — eligible.
        _ => (Eligibility::Eligible, ""),
    }
}

pub fn assert_eligible(agent_type: &str) -> Result<(), String> {
    match eligibility(agent_type) {
        (Eligibility::HardReject, msg) => Err(msg.to_string()),
        _ => Ok(()),
    }
}
```

### 5.4 `team/mailbox.rs` — file-based messaging (port of `send/inbox/poll/ack/reservation`)

```rust
use std::fs;
use std::path::Path;
use crate::team::{locks::{atomic_write, with_lock, read_json},
                  paths::inbox_dir,
                  spec::*,
                  state::load_runtime};

pub struct SendContext<'a> {
    pub is_lead: bool,
    pub active_members: &'a [String],
    pub reserved_recipients: &'a [String],
    pub recipient_unread_max_bytes: usize,
}

pub struct SendResult { pub message_id: String, pub delivered_to: Vec<String> }

/// Port of send.ts. Validation order matches the reference exactly.
pub fn send_message(msg: &TeamMessage, run_id: &str, ctx: &SendContext) -> TeamResult<SendResult> {
    let serialized = format!("{}\n", serde_json::to_string_pretty(msg)?);
    let serialized_bytes = serialized.len();
    if msg.body.len() > TEAM_MESSAGE_MAX_BYTES {
        return Err(TeamError::PayloadTooLarge);
    }
    // assertTeamAcceptsMessages: a missing state file is tolerated (team not yet persisted).
    if let Ok(state) = load_runtime(run_id) {
        if matches!(state.status, RuntimeStatus::Deleting | RuntimeStatus::Deleted) {
            return Err(TeamError::TeamDeleting);
        }
    }
    if msg.to == "*" && !ctx.is_lead {
        return Err(TeamError::BroadcastNotPermitted);
    }

    let recipients: Vec<String> = if msg.to == "*" {
        let mut v = ctx.active_members.to_vec();
        v.sort(); v.dedup(); v
    } else {
        vec![msg.to.clone()]
    };

    let mut delivered = Vec::new();
    for recipient in recipients {
        let inbox = inbox_dir(run_id, &recipient);
        fs::create_dir_all(&inbox)?;
        let lock = inbox.with_extension("lock");
        with_lock(&lock, &format!("team-mailbox:{recipient}"), || {
            let unread = unread_size_bytes(&inbox)?;
            if unread + serialized_bytes > ctx.recipient_unread_max_bytes {
                return Err(TeamError::RecipientBackpressure);
            }
            let unreserved = inbox.join(format!("{}.json", msg.message_id));
            let reserved   = inbox.join(format!(".delivering-{}.json", msg.message_id));
            if unreserved.exists() || reserved.exists() {
                return Err(TeamError::DuplicateMessageId(msg.message_id.clone()));
            }
            let target = if ctx.reserved_recipients.contains(&recipient)
                { reserved } else { unreserved };
            atomic_write(&target, &serialized)?;
            Ok(())
        })?;
        delivered.push(recipient);
    }
    Ok(SendResult { message_id: msg.message_id.clone(), delivered_to: delivered })
}

/// Sum sizes of unread message files (`*.json` and `.delivering-*.json`, skip `processed/`).
fn unread_size_bytes(inbox: &Path) -> TeamResult<usize> {
    let mut total = 0usize;
    let rd = match fs::read_dir(inbox) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        if !name.ends_with(".json") { continue; }
        let is_delivering = name.starts_with(".delivering-");
        if name.starts_with('.') && !is_delivering { continue; }
        total += entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
    }
    Ok(total)
}

/// Port of inbox.ts: list unread, parse, skip malformed, sort ascending by timestamp.
pub fn list_unread(run_id: &str, member: &str) -> TeamResult<Vec<TeamMessage>> {
    let inbox = inbox_dir(run_id, member);
    let mut out = Vec::new();
    let rd = match fs::read_dir(&inbox) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.ends_with(".json") { continue; }
        match read_json::<TeamMessage>(&entry.path()) {
            Ok(m) => out.push(m),
            Err(_) => continue, // skip malformed/unreadable, like the reference
        }
    }
    out.sort_by_key(|m| m.timestamp);
    Ok(out)
}

/// Port of poll.ts: return messages not yet injected, tracking pending ids per turn.
pub fn poll_messages(run_id: &str, member: &str, already_pending: &[String])
    -> TeamResult<Vec<TeamMessage>>
{
    let unread = list_unread(run_id, member)?;
    Ok(unread.into_iter()
        .filter(|m| !already_pending.contains(&m.message_id))
        .collect())
}

/// Port of ack.ts: move acked messages into `processed/`.
pub fn acknowledge(run_id: &str, member: &str, message_ids: &[String]) -> TeamResult<()> {
    let inbox = inbox_dir(run_id, member);
    let processed = inbox.join("processed");
    fs::create_dir_all(&processed)?;
    for id in message_ids {
        let target = processed.join(format!("{id}.json"));
        for src in [inbox.join(format!("{id}.json")),
                    inbox.join(format!(".delivering-{id}.json"))] {
            match fs::rename(&src, &target) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(TeamError::Io(e)),
            }
        }
    }
    Ok(())
}
```

### 5.5 `team/tasklist.rs` — dependency-aware task board (port of `team-tasklist/*`)

```rust
use std::fs;
use crate::team::{locks::{atomic_write, with_lock, read_json},
                  paths::tasks_dir, spec::*};

pub struct NewTask {
    pub subject: String,
    pub description: String,
    pub blocks: Vec<String>,
    pub blocked_by: Vec<String>,
}

/// Port of store.ts: high-watermark counter under a tasks-dir lock.
pub fn create_task(run_id: &str, input: NewTask) -> TeamResult<TeamTask> {
    let dir = tasks_dir(run_id);
    fs::create_dir_all(dir.join("claims"))?;
    let lock = dir.join(".lock");
    with_lock(&lock, &format!("create-task:{run_id}"), || {
        let wm_path = dir.join(".highwatermark");
        let next = read_high_watermark(&wm_path) + 1;
        atomic_write(&wm_path, &next.to_string())?;
        let now = chrono::Utc::now().timestamp_millis();
        let task = TeamTask {
            version: 1, id: next.to_string(),
            subject: input.subject, description: input.description, active_form: None,
            status: TaskStatus::Pending, owner: None,
            blocks: input.blocks, blocked_by: input.blocked_by,
            created_at: now, updated_at: now, claimed_at: None,
        };
        atomic_write(&dir.join(format!("{}.json", task.id)),
                     &format!("{}\n", serde_json::to_string_pretty(&task)?))?;
        Ok(task)
    })
}

fn read_high_watermark(path: &std::path::Path) -> u64 {
    fs::read_to_string(path).ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|n| *n < u64::MAX) // guard
        .unwrap_or(0)
}

/// Port of claim.ts: atomic per-task claim with dependency gate.
pub fn claim_task(run_id: &str, task_id: &str, member: &str) -> TeamResult<TeamTask> {
    let dir = tasks_dir(run_id);
    let claim_lock = dir.join("claims").join(format!("{task_id}.lock"));
    with_lock(&claim_lock, &format!("claim:{task_id}"), || {
        let path = dir.join(format!("{task_id}.json"));
        let mut task: TeamTask = read_json(&path)?;
        if task.status != TaskStatus::Pending {
            return Err(TeamError::Tmux(format!("task {task_id} not claimable")));
        }
        // All blockers must be Completed.
        for dep in &task.blocked_by {
            let dep_task: TeamTask = read_json(&dir.join(format!("{dep}.json")))?;
            if dep_task.status != TaskStatus::Completed {
                return Err(TeamError::Tmux(format!("task {task_id} blocked by {dep}")));
            }
        }
        task.status = TaskStatus::Claimed;
        task.owner = Some(member.to_string());
        task.claimed_at = Some(chrono::Utc::now().timestamp_millis());
        task.updated_at = task.claimed_at.unwrap();
        atomic_write(&path, &format!("{}\n", serde_json::to_string_pretty(&task)?))?;
        Ok(task)
    })
}

/// Port of update.ts: validated status transitions.
pub fn update_status(run_id: &str, task_id: &str, next: TaskStatus) -> TeamResult<TeamTask> {
    let path = tasks_dir(run_id).join(format!("{task_id}.json"));
    let mut task: TeamTask = read_json(&path)?;
    if !valid_transition(task.status, next) {
        return Err(TeamError::Tmux(
            format!("invalid transition {:?} -> {:?}", task.status, next)));
    }
    task.status = next;
    task.updated_at = chrono::Utc::now().timestamp_millis();
    atomic_write(&path, &format!("{}\n", serde_json::to_string_pretty(&task)?))?;
    Ok(task)
}

fn valid_transition(from: TaskStatus, to: TaskStatus) -> bool {
    use TaskStatus::*;
    matches!((from, to),
        (Pending, Claimed) | (Claimed, InProgress) | (InProgress, Completed)
        | (Claimed, Pending)          // release
        | (Completed, Deleted) | (Pending, Deleted))
}

/// Port of list.ts: read all task files, optionally filtered.
pub fn list_tasks(run_id: &str, status: Option<TaskStatus>, owner: Option<&str>)
    -> TeamResult<Vec<TeamTask>>
{
    let dir = tasks_dir(run_id);
    let mut out = Vec::new();
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.ends_with(".json") { continue; }
        if let Ok(task) = read_json::<TeamTask>(&entry.path()) {
            if status.map(|s| s == task.status).unwrap_or(true)
               && owner.map(|o| task.owner.as_deref() == Some(o)).unwrap_or(true) {
                out.push(task);
            }
        }
    }
    out.sort_by(|a, b| a.id.parse::<u64>().unwrap_or(0).cmp(&b.id.parse::<u64>().unwrap_or(0)));
    Ok(out)
}

/// Port of dependencies.ts: transitive blockers; also detects cycles.
pub fn transitive_blockers(run_id: &str, task_id: &str) -> TeamResult<Vec<String>> {
    let dir = tasks_dir(run_id);
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![task_id.to_string()];
    let mut order = Vec::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) { continue; } // cycle guard
        let task: TeamTask = read_json(&dir.join(format!("{id}.json")))?;
        for dep in task.blocked_by {
            if dep != task_id { order.push(dep.clone()); }
            stack.push(dep);
        }
    }
    Ok(order)
}
```

### 5.6 `team/layout.rs` — tmux panes (port of `layout.ts` + `rebalance` + `sweep`)

```rust
use std::process::Command;
use crate::team::spec::{TeamError, TeamResult};

/// `canVisualize()` — only attempt layout work inside a tmux client.
pub fn can_visualize() -> bool { std::env::var_os("TMUX").is_some() }

fn tmux_path() -> Option<String> {
    // jcode resolves external binaries via PATH; tmux is invoked by name.
    which::which("tmux").ok().map(|p| p.display().to_string())
        .or_else(|| Some("tmux".to_string()))
}

fn run_tmux(args: &[&str]) -> TeamResult<String> {
    let tmux = tmux_path().ok_or_else(|| TeamError::Tmux("tmux not found".into()))?;
    let out = Command::new(tmux).args(args).output()
        .map_err(|e| TeamError::Tmux(format!("spawn failed: {e}")))?;
    if !out.status.success() {
        return Err(TeamError::Tmux(String::from_utf8_lossy(&out.stderr).into_owned()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub struct LayoutMember<'a> { pub name: &'a str, pub attach_cmd: &'a str, pub cwd: &'a str }

/// Split the caller's window into one pane per member. Returns pane id by member name.
/// Faithful port of createTeamLayoutInCallerWindow (horizontal first, then alternating).
pub fn create_team_layout(window_target: &str, caller_pane: &str, members: &[LayoutMember])
    -> TeamResult<std::collections::HashMap<String, String>>
{
    if !can_visualize() || members.is_empty() {
        return Ok(Default::default());
    }
    let mut panes = std::collections::HashMap::new();
    let existing = list_panes(window_target)?;
    let mut teammates: Vec<String> = existing.into_iter().filter(|p| p != caller_pane).collect();

    for m in members {
        let pane_id = if teammates.is_empty() {
            run_tmux(&["split-window", "-t", caller_pane, "-h", "-d", "-l", "70%",
                       "-P", "-F", "#{pane_id}", "-c", m.cwd])?
        } else {
            let anchor = &teammates[teammates.len() / 2];
            let dir = if teammates.len() % 2 == 1 { "-v" } else { "-h" };
            run_tmux(&["split-window", "-t", anchor, dir, "-d",
                       "-P", "-F", "#{pane_id}", "-c", m.cwd])?
        };
        teammates.push(pane_id.clone());
        panes.insert(m.name.to_string(), pane_id.clone());
        let _ = run_tmux(&["select-pane", "-t", &pane_id, "-T", m.name]);
        let _ = run_tmux(&["send-keys", "-t", &pane_id, m.attach_cmd, "Enter"]);
    }
    run_tmux(&["select-layout", "-t", window_target, "main-vertical"])?;
    run_tmux(&["resize-pane", "-t", caller_pane, "-x", "30%"])?;
    Ok(panes)
}

fn list_panes(window_target: &str) -> TeamResult<Vec<String>> {
    let out = run_tmux(&["list-panes", "-t", window_target, "-F", "#{pane_id}"])?;
    Ok(out.lines().filter(|l| !l.is_empty()).map(|l| l.to_string()).collect())
}

/// Port of removeTeamLayout: prefer killing owned session, else kill panes/windows.
pub fn remove_team_layout(owned_session: bool, target_session: &str, pane_ids: &[String])
    -> TeamResult<()>
{
    if !can_visualize() { return Ok(()); }
    if owned_session {
        let _ = run_tmux(&["kill-session", "-t", target_session]);
        return Ok(());
    }
    for pane in pane_ids {
        let _ = run_tmux(&["kill-pane", "-t", pane]); // best-effort
    }
    Ok(())
}

/// Port of rebalance-team-window.ts.
pub fn rebalance(window_id: &str, tiled: bool) -> TeamResult<()> {
    if window_id.is_empty() { return Ok(()); }
    let layout = if tiled { "tiled" } else { "main-vertical" };
    run_tmux(&["select-layout", "-t", window_id, layout])?;
    if !tiled {
        run_tmux(&["set-window-option", "-t", window_id, "main-pane-width", "60%"])?;
        run_tmux(&["select-layout", "-t", window_id, layout])?; // reapply after resize
    }
    Ok(())
}

/// Port of sweep-stale-team-sessions.ts: kill `jcode-team-{uuid}` sessions not in the set.
pub fn sweep_stale_team_sessions(active_run_ids: &std::collections::HashSet<String>)
    -> TeamResult<Vec<String>>
{
    if !can_visualize() { return Ok(vec![]); }
    let listing = match run_tmux(&["list-sessions", "-F", "#{session_name}"]) {
        Ok(s) => s,
        Err(_) => return Ok(vec![]),
    };
    let re = regex::Regex::new(
        r"^jcode-team-([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$"
    ).expect("valid regex");
    let mut killed = Vec::new();
    for line in listing.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(c) = re.captures(line) {
            let run_id = &c[1];
            if !active_run_ids.contains(run_id) {
                if run_tmux(&["kill-session", "-t", line]).is_ok() {
                    killed.push(line.to_string());
                }
            }
        }
    }
    Ok(killed)
}
```

### 5.7 `team/state.rs` — runtime state store (port of `team-state-store/store.ts`)

```rust
use std::fs;
use crate::team::{locks::{atomic_write, with_lock, read_json},
                  paths::{runtime_dir, runtime_state_path, teams_base_dir},
                  spec::*};

/// Create the initial runtime state file (status = Creating).
pub fn create_runtime(spec: &TeamSpec, lead_session_id: &str, source: SpecSource)
    -> TeamResult<TeamRuntimeState>
{
    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let members = spec.members.iter().map(|m| MemberRuntime {
        name: m.name().to_string(),
        session_id: None,
        tmux_pane_id: None,
        agent_type: if Some(m.name()) == spec.lead_agent_id.as_deref() {
            MemberAgentType::Leader
        } else { MemberAgentType::GeneralPurpose },
        subagent_type: match m { TeamMemberSpec::SubagentType { subagent_type, .. }
            => Some(subagent_type.clone()), _ => None },
        category: match m { TeamMemberSpec::Category { category, .. }
            => Some(category.clone()), _ => None },
        status: MemberStatus::Pending,
        color: m.common().color.clone(),
        worktree_path: m.common().worktree_path.clone(),
        last_injected_turn_marker: None,
        pending_injected_message_ids: Vec::new(),
    }).collect();

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
    };
    fs::create_dir_all(runtime_dir(&run_id))?;
    persist(&state)?;
    Ok(state)
}

pub fn load_runtime(run_id: &str) -> TeamResult<TeamRuntimeState> {
    let path = runtime_state_path(run_id);
    if !path.exists() {
        return Err(TeamError::NotFound(run_id.to_string()));
    }
    read_json(&path)
}

fn persist(state: &TeamRuntimeState) -> TeamResult<()> {
    atomic_write(&runtime_state_path(&state.team_run_id),
                 &format!("{}\n", serde_json::to_string_pretty(state)?))
}

/// Read-modify-write under a per-run lock (port of transitionRuntimeState).
pub fn transition<F>(run_id: &str, mutate: F) -> TeamResult<TeamRuntimeState>
where F: FnOnce(&mut TeamRuntimeState)
{
    let lock = runtime_dir(run_id).join(".state.lock");
    with_lock(&lock, &format!("transition:{run_id}"), || {
        let mut state = load_runtime(run_id)?;
        mutate(&mut state);
        persist(&state)?;
        Ok(state)
    })
}

/// Enumerate runtime states for active teams (status in {Creating, Active}).
pub fn list_active_runs() -> TeamResult<Vec<TeamRuntimeState>> {
    let runtime_root = teams_base_dir().join("runtime");
    let mut out = Vec::new();
    let rd = match fs::read_dir(&runtime_root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
        let run_id = entry.file_name().to_string_lossy().into_owned();
        if let Ok(state) = load_runtime(&run_id) {
            if matches!(state.status, RuntimeStatus::Creating | RuntimeStatus::Active) {
                out.push(state);
            }
        }
    }
    Ok(out)
}
```

### 5.8 `team/runtime.rs` — lifecycle (port of `team-runtime/create.ts`, `delete-team.ts`, `shutdown.ts`)

```rust
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use crate::team::{eligibility::assert_eligible, layout, paths, spec::*, state};

/// Callback that actually spawns a headless jcode member session and returns its id.
/// In jcode this wraps `std::process::Command::new("jcode")` + server registration.
pub trait MemberSpawner: Send + Sync {
    fn spawn(&self, run_id: &str, member: &TeamMemberSpec, prompt: &str)
        -> TeamResult<String>; // returns session_id
}

/// Idempotent: returns an existing Active/Creating run for (name, lead) if present.
pub async fn create_team(
    spec: TeamSpec,
    lead_session_id: &str,
    spawner: Arc<dyn MemberSpawner>,
) -> TeamResult<TeamRuntimeState> {
    let mut spec = spec;
    normalize_spec(&mut spec)?;
    paths::validate_team_name(&spec.name)?;
    for m in &spec.members {
        if let Err(msg) = assert_eligible(member_agent_type(m)) {
            return Err(TeamError::IneligibleAgent(m.name().to_string(), msg));
        }
    }

    if let Some(existing) = find_existing_run(&spec.name, lead_session_id)? {
        return Ok(existing);
    }

    // best-effort stale sweep before creating a new run
    let active: HashSet<String> = state::list_active_runs()?
        .into_iter().map(|s| s.team_run_id).collect();
    let _ = layout::sweep_stale_team_sessions(&active);

    let member_names: Vec<String> = spec.members.iter().map(|m| m.name().to_string()).collect();
    let mut run = state::create_runtime(&spec, lead_session_id, SpecSource::Project)?;
    paths::ensure_base_dirs(&run.team_run_id, &member_names)?;

    // Bounded parallel spawn (max_parallel) — shared atomic cursor like the TS Promise.all.
    let next = Arc::new(AtomicUsize::new(0));
    let worker_count = run.bounds.max_parallel_members.min(spec.members.len());
    let spec = Arc::new(spec);
    let run_id = run.team_run_id.clone();
    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let (next, spec, spawner, run_id) =
            (next.clone(), spec.clone(), spawner.clone(), run_id.clone());
        handles.push(tokio::task::spawn_blocking(move || -> TeamResult<()> {
            loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(member) = spec.members.get(i) else { return Ok(()) };
                let prompt = build_member_prompt(&spec, member, &run_id);
                let session_id = spawner.spawn(&run_id, member, &prompt)?;
                state::transition(&run_id, |st| {
                    if let Some(rm) = st.members.iter_mut().find(|m| m.name == member.name()) {
                        rm.session_id = Some(session_id.clone());
                        rm.status = MemberStatus::Running;
                    }
                })?;
                Ok::<(), TeamError>(())
            }
        }));
    }
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => { cleanup(&run_id); return Err(e); }
            Err(e) => { cleanup(&run_id); return Err(TeamError::Tmux(e.to_string())); }
        }
    }

    // Activate tmux layout (best effort — absence is non-fatal, like the reference).
    if layout::can_visualize() {
        run = state::load_runtime(&run_id)?;
        let members: Vec<layout::LayoutMember> = run.members.iter().map(|m| {
            layout::LayoutMember {
                name: &m.name,
                attach_cmd: "", // filled by caller-specific attach command builder
                cwd: ".",
            }
        }).collect();
        if let (Ok(window), Ok(pane)) = (caller_window(), caller_pane()) {
            if let Ok(panes) = layout::create_team_layout(&window, &pane, &members) {
                state::transition(&run_id, |st| {
                    st.tmux_layout = Some(TmuxLayout {
                        owned_session: false,
                        target_session_id: window.clone(),
                        focus_window_id: Some(window.clone()),
                        grid_window_id: None,
                    });
                    for m in st.members.iter_mut() {
                        if let Some(p) = panes.get(&m.name) { m.tmux_pane_id = Some(p.clone()); }
                    }
                })?;
            }
        }
    }

    state::transition(&run_id, |st| st.status = RuntimeStatus::Active)
}

/// Port of delete-team.ts: tear down layout + files, then mark Deleted.
pub fn delete_team(run_id: &str) -> TeamResult<()> {
    let _ = state::transition(run_id, |st| st.status = RuntimeStatus::Deleting);
    if let Ok(st) = state::load_runtime(run_id) {
        if let Some(layout) = &st.tmux_layout {
            let pane_ids: Vec<String> = st.members.iter()
                .filter_map(|m| m.tmux_pane_id.clone()).collect();
            let _ = layout::remove_team_layout(
                layout.owned_session, &layout.target_session_id, &pane_ids);
        }
    }
    let _ = std::fs::remove_dir_all(paths::runtime_dir(run_id));
    let _ = state::transition(run_id, |st| st.status = RuntimeStatus::Deleted);
    Ok(())
}

fn cleanup(run_id: &str) {
    let _ = delete_team(run_id);
}

fn find_existing_run(name: &str, lead: &str) -> TeamResult<Option<TeamRuntimeState>> {
    for st in state::list_active_runs()? {
        if st.team_name == name && st.lead_session_id.as_deref() == Some(lead) {
            return Ok(Some(st));
        }
    }
    Ok(None)
}

fn member_agent_type(m: &TeamMemberSpec) -> &str {
    match m {
        TeamMemberSpec::SubagentType { subagent_type, .. } => subagent_type,
        TeamMemberSpec::Category { .. } => "sisyphus", // category members use default worker
    }
}

fn build_member_prompt(spec: &TeamSpec, member: &TeamMemberSpec, run_id: &str) -> String {
    let mut lines = vec![
        format!("Team: {}", spec.name),
        format!("TeamRunId: {run_id}"),
        format!("Member: {}", member.name()),
    ];
    if let Some(wt) = &member.common().worktree_path { lines.push(format!("Worktree: {wt}")); }
    match member {
        TeamMemberSpec::Category { prompt, .. } => lines.push(prompt.clone()),
        TeamMemberSpec::SubagentType { prompt: Some(p), .. } => lines.push(p.clone()),
        _ => {}
    }
    lines.join("\n")
}

/// Promote the first member to lead when `lead_agent_id` is unset (port of `.transform`).
fn normalize_spec(spec: &mut TeamSpec) -> TeamResult<()> {
    if spec.members.is_empty() {
        return Err(TeamError::InvalidTeamName(spec.name.clone(), "no members".into()));
    }
    if spec.members.len() > TEAM_MAX_MEMBERS {
        return Err(TeamError::InvalidTeamName(
            spec.name.clone(), format!("max {TEAM_MAX_MEMBERS} members")));
    }
    if spec.lead_agent_id.is_none() {
        spec.lead_agent_id = Some(spec.members[0].name().to_string());
    }
    Ok(())
}

// These two are provided by the jcode caller (server has the live tmux context).
fn caller_window() -> TeamResult<String> { Ok(std::env::var("TMUX_PANE").unwrap_or_default()) }
fn caller_pane() -> TeamResult<String> { Ok(std::env::var("TMUX_PANE").unwrap_or_default()) }
```

### 5.9 `crates/jcode-app-core/src/tool/team.rs` — upgraded tools

The existing `TeamCreateTool` / `TeamDeleteTool` (basic JSON CRUD) are upgraded to drive the
runtime, and new sibling tools are added. All implement the existing `Tool` trait (see
`tool/mod.rs`). Only the new surface is shown:

```rust
use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use jcode_swarm_core::team::{spec::*, runtime, mailbox, tasklist, state};

pub struct TeamCreateTool { /* holds Arc<dyn MemberSpawner> + SwarmState handle */ }

#[derive(Deserialize)]
struct TeamCreateInput {
    name: String,
    #[serde(default)] description: Option<String>,
    members: Vec<Value>,        // parsed into TeamMemberSpec via serde
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str { "team_create" }
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
                "name": { "type": "string", "description": "Team name (^[a-z0-9-]+$)." },
                "description": { "type": "string" },
                "members": {
                    "type": "array", "minItems": 1, "maxItems": 8,
                    "items": { "type": "object", "required": ["name", "kind"] }
                }
            }
        })
    }
    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let parsed: TeamCreateInput = serde_json::from_value(input)?;
        let members: Vec<TeamMemberSpec> = parsed.members.into_iter()
            .map(serde_json::from_value).collect::<Result<_, _>>()?;
        let spec = TeamSpec {
            version: 1, name: parsed.name.clone(), description: parsed.description,
            created_at: chrono::Utc::now().timestamp_millis(),
            lead_agent_id: None, team_allowed_paths: None, members,
        };
        let run = runtime::create_team(spec, &_ctx.session_id, self.spawner.clone()).await?;
        Ok(ToolOutput::new(serde_json::to_string_pretty(&run)?)
            .with_title(format!("Team '{}' active ({} members)",
                                parsed.name, run.members.len())))
    }
}
// team_delete, team_status, team_send_message, team_task_create, team_task_claim,
// team_task_list, team_shutdown follow the same shape, each delegating to the
// jcode-swarm-core::team functions implemented above.
```

### 5.10 `crates/jcode-app-core/src/server/state.rs` — SwarmState wiring

```rust
// Add a live index of active team runs to SwarmState (file is source of truth;
// this is a hot cache for the TUI + tools, mirroring the existing swarms_by_id field).
pub struct SwarmState {
    pub members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    pub swarms_by_id: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    pub plans: Arc<RwLock<HashMap<String, VersionedPlan>>>,
    pub coordinators: Arc<RwLock<HashMap<String, String>>>,
    // NEW: team_run_id -> cached runtime snapshot for fast widget reads.
    pub team_runtimes: Arc<RwLock<HashMap<String, TeamRuntimeState>>>,
}
```

### 5.11 `crates/jcode-tui/src/tui/info_widget_team.rs` — TUI team widget (NEW)

Mirrors the rendering style of `info_widget_swarm_background.rs` (status icons + color per
member) but adds a task DAG section. Wired into the existing `WidgetKind` machinery.

```rust
use super::{InfoWidgetData, truncate_smart};
use crate::tui::color_support::rgb;
use ratatui::prelude::*;

/// Snapshot fed into InfoWidgetData.team_info (built from SwarmState.team_runtimes).
#[derive(Debug, Default, Clone)]
pub struct TeamInfo {
    pub team_name: String,
    pub member_total: usize,
    pub members: Vec<TeamMemberView>,
    pub tasks: Vec<TeamTaskView>,
}

#[derive(Debug, Clone)]
pub struct TeamMemberView {
    pub name: String,
    pub is_lead: bool,
    pub status: String,    // "pending" | "running" | "idle" | "errored" | "completed"
    pub task_count: usize,
    pub message_count: usize,
    pub color: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TeamTaskView {
    pub id: String,
    pub subject: String,
    pub status: String,    // "pending" | "claimed" | "in_progress" | "completed"
    pub owner: Option<String>,
    pub blocked_by: Vec<String>,
}

fn member_status_glyph(status: &str) -> (Color, &'static str) {
    match status {
        "pending"   => (rgb(140, 140, 150), "○"),
        "running"   => (rgb(255, 200, 100), "▶"),
        "idle"      => (rgb(120, 180, 120), "●"),
        "errored"   => (rgb(255, 100, 100), "✗"),
        "completed" => (rgb(100, 200, 100), "✓"),
        _           => (rgb(140, 140, 150), "·"),
    }
}

fn task_status_badge(status: &str) -> (Color, &'static str) {
    match status {
        "completed"   => (rgb(100, 200, 100), "[✓]"),
        "in_progress" => (rgb(255, 200, 100), "[▶]"),
        "claimed"     => (rgb(140, 180, 255), "[◑]"),
        _             => (rgb(140, 140, 150), "[○]"),
    }
}

pub(super) fn render_team_widget(data: &InfoWidgetData, inner: Rect) -> Vec<Line<'static>> {
    let Some(info) = &data.team_info else { return Vec::new() };
    let mut lines = Vec::new();

    // Header: team name + member/task counts.
    let active = info.members.iter().filter(|m| m.status == "running").count();
    lines.push(Line::from(vec![
        Span::styled("👥 ", Style::default().fg(rgb(255, 200, 100))),
        Span::styled(
            truncate_smart(&info.team_name, inner.width.saturating_sub(20) as usize),
            Style::default().fg(rgb(220, 220, 230)).bold()),
        Span::styled(
            format!(" {}/{} · {} active · {} tasks",
                    info.members.len(), info.member_total, active, info.tasks.len()),
            Style::default().fg(rgb(140, 140, 150))),
    ]));

    // Member rows (cap to fit height, reserve room for tasks).
    let max_members = ((inner.height as usize).saturating_sub(2)).min(info.members.len()).min(5);
    for m in info.members.iter().take(max_members) {
        let (color, glyph) = member_status_glyph(&m.status);
        let role = if m.is_lead { "★ " } else { "  " };
        let detail = format!("{} · {}t · {}m", m.status, m.task_count, m.message_count);
        lines.push(Line::from(vec![
            Span::styled(role.to_string(), Style::default().fg(rgb(255, 200, 100))),
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::styled(
                truncate_smart(&m.name, 14), Style::default().fg(rgb(200, 200, 210))),
            Span::styled(format!("  {detail}"), Style::default().fg(rgb(140, 140, 150))),
        ]));
    }

    // Task DAG (compact): show up to 3, with dependency arrows.
    let remaining = (inner.height as usize).saturating_sub(lines.len());
    if remaining > 1 && !info.tasks.is_empty() {
        lines.push(Line::from(Span::styled(
            "Tasks", Style::default().fg(rgb(140, 140, 150)).bold())));
        for t in info.tasks.iter().take(remaining.saturating_sub(1)).take(3) {
            let (color, badge) = task_status_badge(&t.status);
            let mut spans = vec![
                Span::styled(format!("{badge} "), Style::default().fg(color)),
                Span::styled(truncate_smart(&t.subject, 22),
                             Style::default().fg(rgb(190, 190, 200))),
            ];
            if let Some(owner) = &t.owner {
                spans.push(Span::styled(format!(" ({owner})"),
                    Style::default().fg(rgb(120, 120, 130))));
            }
            if !t.blocked_by.is_empty() {
                spans.push(Span::styled(format!(" ←{}", t.blocked_by.join(",")),
                    Style::default().fg(rgb(255, 170, 80))));
            }
            lines.push(Line::from(spans));
        }
    }
    lines
}
```

**Wiring into `info_widget.rs`** (mirrors the existing `SwarmStatus` machinery):

```rust
// 1. Add the variant:
pub enum WidgetKind { /* ... */ TeamView }

// 2. priority(): active teams are important — place near the top dynamically.
WidgetKind::TeamView => 6,           // base; bumped to 2 when a team is active (see effective_priority)

// 3. preferred_side(): Right (the roster/DAG benefits from width).
WidgetKind::TeamView => Side::Right,

// 4. min_height(): 5
WidgetKind::TeamView => 5,

// 5. all_by_priority(): insert TeamView after KvCache.

// 6. InfoWidgetData: add `pub team_info: Option<TeamInfo>,`

// 7. has_data_for(TeamView): self.team_info.as_ref().map(|t| !t.members.is_empty()).unwrap_or(false)

// 8. effective_priority(): if a team is active, return 2.

// 9. render_widget_content(): WidgetKind::TeamView => render_team_widget(data, inner),

// 10. calculate_widget_height(): size = 1 (header) + members(<=5) + 1 (Tasks) + tasks(<=3).
```

The data pipeline: the server periodically reads `SwarmState.team_runtimes` (refreshed from
the runtime state file + mailbox/task counts) and ships a `TeamInfo` into `InfoWidgetData`,
exactly like `SwarmInfo` is populated today in `tui_state.rs`.

---

## 6. Configuration & Wiring

### 6.1 `Cargo.toml` additions

`crates/jcode-swarm-core/Cargo.toml` currently depends only on `jcode-plan` and `serde`.
Add:

```toml
[dependencies]
jcode-plan = { path = "../jcode-plan" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", default-features = false, features = ["clock"] }
dirs = "5"
thiserror = "2"
regex = "1"
which = "6"
libc = "0.2"               # unix pid-liveness check for stale locks
tokio = { version = "1", features = ["rt", "macros"] } # spawn_blocking for parallel member spawn

[dev-dependencies]
tempfile = "3"             # isolated temp dirs in unit tests
criterion = "0.5"          # benchmarks

[[bench]]
name = "team_mailbox"
harness = false
```

> All versions should be pinned to the workspace's existing lockfile entries where present
> (uuid, chrono, serde_json, dirs, regex, libc, tokio are already used elsewhere in jcode, so
> reuse those exact versions to avoid duplicate-version bloat).

### 6.2 Tool registration

In `crates/jcode-app-core/src/tool/mod.rs`, the current `base_tools()` registers
`team_create`/`team_delete` indirectly. Register the upgraded + new tools (they need the
`MemberSpawner` + `SwarmState` handle, so they are session tools registered in
`Registry::new()` alongside `subagent`/`batch`, not in the cached `base_tools()`):

```rust
Self::insert_tool(&mut tools_map, "team_create",
    team::TeamCreateTool::new(spawner.clone(), swarm_state.clone()));
Self::insert_tool(&mut tools_map, "team_delete",   team::TeamDeleteTool::new(swarm_state.clone()));
Self::insert_tool(&mut tools_map, "team_status",   team::TeamStatusTool::new(swarm_state.clone()));
Self::insert_tool(&mut tools_map, "team_message",  team::TeamMessageTool::new(swarm_state.clone()));
Self::insert_tool(&mut tools_map, "team_task",     team::TeamTaskTool::new(swarm_state.clone()));
Self::insert_tool(&mut tools_map, "team_shutdown", team::TeamShutdownTool::new(swarm_state.clone()));
```

### 6.3 `jcode-swarm-core/src/lib.rs` export

```rust
pub mod team;   // re-exports spec, paths, locks, eligibility, mailbox, tasklist, layout, state, runtime
```

### 6.4 Config (`~/.jcode/config.toml`)

```toml
[team]
max_members = 8
max_parallel = 4
inbox_unread_max_bytes = 10485760   # 10 MiB backpressure ceiling per recipient
wall_clock_minutes = 120
stale_heartbeat_seconds = 300
teammate_mode = "tmux"              # "tmux" | "in-process" (v2) | "auto"
```

### 6.5 Member attach command

Headless members are launched by the `MemberSpawner` impl in `jcode-app-core`. The tmux pane
attach command (sent via `send-keys`) is:

```
jcode attach --team {team_run_id} --member {name} --session {session_id}
```

This reuses jcode's existing `serve`/`attach`/`connect` client model (README "persistent
background server, then attach more clients"), so a team member pane is just a normal jcode
client bound to the spawned headless session.

---

## 7. Repo References

Direct links to the source code each module is ported from. (oh-my-openagent default branch
is `dev`; Claude Code repo `claude-code-best/claude-code` default branch is `main`.)

| Feature aspect | Repo | File | Link |
|----------------|------|------|------|
| Types / schemas / bounds | oh-my-openagent | `src/features/team-mode/types.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/types.ts |
| Agent eligibility | oh-my-openagent | `types.ts` (`AGENT_ELIGIBILITY_REGISTRY`) | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/types.ts |
| Atomic locks / writes | oh-my-openagent | `team-state-store/locks.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-state-store/locks.ts |
| Path layout | oh-my-openagent | `team-registry/paths.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-registry/paths.ts |
| Mailbox send | oh-my-openagent | `team-mailbox/send.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-mailbox/send.ts |
| Mailbox inbox | oh-my-openagent | `team-mailbox/inbox.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-mailbox/inbox.ts |
| Mailbox poll | oh-my-openagent | `team-mailbox/poll.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-mailbox/poll.ts |
| Mailbox ack | oh-my-openagent | `team-mailbox/ack.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-mailbox/ack.ts |
| Delivery reservation | oh-my-openagent | `team-mailbox/reservation.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-mailbox/reservation.ts |
| Task store (high-watermark) | oh-my-openagent | `team-tasklist/store.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-tasklist/store.ts |
| Task claim | oh-my-openagent | `team-tasklist/claim.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-tasklist/claim.ts |
| Task update / list / deps | oh-my-openagent | `team-tasklist/{update,list,dependencies}.ts` | https://github.com/code-yeongyu/oh-my-openagent/tree/dev/src/features/team-mode/team-tasklist |
| Tmux layout | oh-my-openagent | `team-layout-tmux/layout.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-layout-tmux/layout.ts |
| Rebalance | oh-my-openagent | `team-layout-tmux/rebalance-team-window.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-layout-tmux/rebalance-team-window.ts |
| Stale sweep | oh-my-openagent | `team-layout-tmux/sweep-stale-team-sessions.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-layout-tmux/sweep-stale-team-sessions.ts |
| Runtime create | oh-my-openagent | `team-runtime/create.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-runtime/create.ts |
| Runtime delete / shutdown | oh-my-openagent | `team-runtime/{delete-team,shutdown}.ts` | https://github.com/code-yeongyu/oh-my-openagent/tree/dev/src/features/team-mode/team-runtime |
| State store | oh-my-openagent | `team-state-store/store.ts` | https://github.com/code-yeongyu/oh-my-openagent/blob/dev/src/features/team-mode/team-state-store/store.ts |
| Team create tool (UI) | claude-code | `packages/builtin-tools/src/tools/TeamCreateTool/TeamCreateTool.ts` | https://github.com/claude-code-best/claude-code/blob/main/packages/builtin-tools/src/tools/TeamCreateTool/TeamCreateTool.ts |
| Backend-agnostic spawn (UI) | claude-code | `packages/builtin-tools/src/tools/shared/spawnMultiAgent.ts` | https://github.com/claude-code-best/claude-code/blob/main/packages/builtin-tools/src/tools/shared/spawnMultiAgent.ts |
| Agent Teams display modes | claude-code (docs) | Agent Teams guide | https://code.claude.com/docs/en/agent-teams |

### jcode integration points (target files)

| Component | File | Action |
|-----------|------|--------|
| Swarm types | `crates/jcode-swarm-core/src/lib.rs` | add `pub mod team;` |
| Team logic | `crates/jcode-swarm-core/src/team/*.rs` | NEW (all §5 modules) |
| Tools | `crates/jcode-app-core/src/tool/team.rs` | UPGRADE + new tools |
| Tool registry | `crates/jcode-app-core/src/tool/mod.rs` | register session tools |
| Server state | `crates/jcode-app-core/src/server/state.rs` | add `team_runtimes` field |
| TUI widget | `crates/jcode-tui/src/tui/info_widget_team.rs` | NEW |
| Widget enum | `crates/jcode-tui/src/tui/info_widget.rs` | add `WidgetKind::TeamView` + `team_info` |
| Widget data feed | `crates/jcode-tui/src/tui/app/tui_state.rs` | populate `TeamInfo` (like `SwarmInfo`) |
| Design doc | `docs/SWARM_ARCHITECTURE.md` | already specifies the graph-view widget intent |

---

## 8. Test Cases

All unit tests use `tempfile::TempDir` and set `HOME`/base-dir override so the file layout is
isolated per test. (Provide `teams_base_dir()` an override hook reading an env var in tests.)

### 8.1 Happy path — locks & atomic write (`team/locks.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn with_lock_serializes_and_releases() {
        let dir = TempDir::new().unwrap();
        let lock = dir.path().join("x.lock");
        let out = with_lock(&lock, "test", || Ok(42)).unwrap();
        assert_eq!(out, 42);
        assert!(!lock.exists(), "lock must be released after body runs");
    }

    #[test]
    fn atomic_write_then_read_roundtrips() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("data.json");
        atomic_write(&p, "{\"a\":1}\n").unwrap();
        let v: serde_json::Value = read_json(&p).unwrap();
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn atomic_write_leaves_no_temp_on_success() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("d.json");
        atomic_write(&p, "{}").unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no .tmp files should remain");
    }
}
```

### 8.2 Mailbox send / list / ack cycle (`team/mailbox.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, to: &str, body: &str) -> TeamMessage {
        TeamMessage {
            version: 1, message_id: id.into(), from: "lead".into(), to: to.into(),
            kind: MessageKind::Message, body: body.into(), summary: None,
            references: vec![], timestamp: 1, correlation_id: None, color: None,
        }
    }
    fn ctx<'a>(members: &'a [String]) -> SendContext<'a> {
        SendContext { is_lead: true, active_members: members, reserved_recipients: &[],
                      recipient_unread_max_bytes: TEAM_RECIPIENT_UNREAD_MAX_BYTES }
    }

    #[test]
    fn send_then_list_then_ack() {
        let run = test_run_id(); // helper creates ~/.jcode/teams/runtime/<run> under temp HOME
        let members = vec!["worker".to_string()];
        send_message(&msg("m1", "worker", "hello"), &run, &ctx(&members)).unwrap();
        let unread = list_unread(&run, "worker").unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].body, "hello");
        acknowledge(&run, "worker", &["m1".into()]).unwrap();
        assert!(list_unread(&run, "worker").unwrap().is_empty());
    }

    #[test]
    fn duplicate_message_id_rejected() {
        let run = test_run_id();
        let members = vec!["worker".to_string()];
        send_message(&msg("dup", "worker", "a"), &run, &ctx(&members)).unwrap();
        let err = send_message(&msg("dup", "worker", "b"), &run, &ctx(&members)).unwrap_err();
        assert!(matches!(err, TeamError::DuplicateMessageId(_)));
    }

    #[test]
    fn broadcast_requires_lead() {
        let run = test_run_id();
        let members = vec!["a".into(), "b".into()];
        let mut c = ctx(&members); c.is_lead = false;
        let err = send_message(&msg("b1", "*", "hi"), &run, &c).unwrap_err();
        assert!(matches!(err, TeamError::BroadcastNotPermitted));
    }

    #[test]
    fn payload_too_large_rejected() {
        let run = test_run_id();
        let members = vec!["w".to_string()];
        let big = "x".repeat(TEAM_MESSAGE_MAX_BYTES + 1);
        let err = send_message(&msg("p", "w", &big), &run, &ctx(&members)).unwrap_err();
        assert!(matches!(err, TeamError::PayloadTooLarge));
    }

    #[test]
    fn backpressure_blocks_when_inbox_full() {
        let run = test_run_id();
        let members = vec!["w".to_string()];
        let mut c = ctx(&members);
        c.recipient_unread_max_bytes = 200; // tiny ceiling
        send_message(&msg("a", "w", &"x".repeat(150)), &run, &c).unwrap();
        let err = send_message(&msg("b", "w", &"x".repeat(150)), &run, &c).unwrap_err();
        assert!(matches!(err, TeamError::RecipientBackpressure));
    }

    #[test]
    fn list_unread_sorted_by_timestamp() {
        let run = test_run_id();
        let members = vec!["w".to_string()];
        let mut m_late = msg("late", "w", "2"); m_late.timestamp = 200;
        let mut m_early = msg("early", "w", "1"); m_early.timestamp = 100;
        send_message(&m_late, &run, &ctx(&members)).unwrap();
        send_message(&m_early, &run, &ctx(&members)).unwrap();
        let unread = list_unread(&run, "w").unwrap();
        assert_eq!(unread[0].message_id, "early");
        assert_eq!(unread[1].message_id, "late");
    }

    #[test]
    fn malformed_message_file_skipped() {
        let run = test_run_id();
        let members = vec!["w".to_string()];
        send_message(&msg("ok", "w", "good"), &run, &ctx(&members)).unwrap();
        // drop a junk file directly in the inbox
        std::fs::write(inbox_dir(&run, "w").join("junk.json"), b"{not json").unwrap();
        let unread = list_unread(&run, "w").unwrap();
        assert_eq!(unread.len(), 1, "malformed file is skipped, valid one survives");
    }
}
```

### 8.3 Task board — claim, deps, transitions (`team/tasklist.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_assigns_incrementing_ids() {
        let run = test_run_id();
        let t1 = create_task(&run, NewTask { subject:"a".into(), description:"".into(),
                                             blocks:vec![], blocked_by:vec![] }).unwrap();
        let t2 = create_task(&run, NewTask { subject:"b".into(), description:"".into(),
                                             blocks:vec![], blocked_by:vec![] }).unwrap();
        assert_eq!(t1.id, "1");
        assert_eq!(t2.id, "2");
    }

    #[test]
    fn claim_blocked_task_fails_until_dependency_completed() {
        let run = test_run_id();
        let dep = create_task(&run, NewTask{subject:"dep".into(),description:"".into(),
                                            blocks:vec![],blocked_by:vec![]}).unwrap();
        let blocked = create_task(&run, NewTask{subject:"main".into(),description:"".into(),
                                            blocks:vec![],blocked_by:vec![dep.id.clone()]}).unwrap();
        // cannot claim while dep is Pending
        assert!(claim_task(&run, &blocked.id, "w").is_err());
        // complete the dependency
        claim_task(&run, &dep.id, "w").unwrap();
        update_status(&run, &dep.id, TaskStatus::InProgress).unwrap();
        update_status(&run, &dep.id, TaskStatus::Completed).unwrap();
        // now claim succeeds
        let claimed = claim_task(&run, &blocked.id, "w").unwrap();
        assert_eq!(claimed.status, TaskStatus::Claimed);
        assert_eq!(claimed.owner.as_deref(), Some("w"));
    }

    #[test]
    fn double_claim_rejected() {
        let run = test_run_id();
        let t = create_task(&run, NewTask{subject:"x".into(),description:"".into(),
                                          blocks:vec![],blocked_by:vec![]}).unwrap();
        claim_task(&run, &t.id, "a").unwrap();
        assert!(claim_task(&run, &t.id, "b").is_err(), "already claimed");
    }

    #[test]
    fn invalid_transition_rejected() {
        let run = test_run_id();
        let t = create_task(&run, NewTask{subject:"x".into(),description:"".into(),
                                          blocks:vec![],blocked_by:vec![]}).unwrap();
        // Pending -> Completed is not allowed (must go through Claimed/InProgress)
        assert!(update_status(&run, &t.id, TaskStatus::Completed).is_err());
    }

    #[test]
    fn transitive_blockers_handles_cycle() {
        let run = test_run_id();
        // a blocked_by b, b blocked_by a (cycle) — must terminate.
        let a = create_task(&run, NewTask{subject:"a".into(),description:"".into(),
                                          blocks:vec![],blocked_by:vec!["2".into()]}).unwrap();
        let _b = create_task(&run, NewTask{subject:"b".into(),description:"".into(),
                                          blocks:vec![],blocked_by:vec!["1".into()]}).unwrap();
        let deps = transitive_blockers(&run, &a.id).unwrap();
        assert!(deps.contains(&"2".to_string()));
    }
}
```

### 8.4 Eligibility (`team/eligibility.rs`)

```rust
#[test]
fn read_only_agents_rejected() {
    for a in ["oracle", "librarian", "explore", "metis", "momus"] {
        assert!(assert_eligible(a).is_err(), "{a} must be rejected");
    }
}
#[test]
fn workers_eligible() {
    for a in ["sisyphus", "sisyphus-junior", "atlas"] {
        assert!(assert_eligible(a).is_ok(), "{a} must be eligible");
    }
}
```

### 8.5 Spec normalization (`team/runtime.rs`)

```rust
#[test]
fn first_member_promoted_to_lead() {
    let mut spec = TeamSpec {
        version:1, name:"t".into(), description:None, created_at:0,
        lead_agent_id:None, team_allowed_paths:None,
        members: vec![member("alpha"), member("beta")],
    };
    normalize_spec(&mut spec).unwrap();
    assert_eq!(spec.lead_agent_id.as_deref(), Some("alpha"));
}
#[test]
fn over_max_members_rejected() {
    let mut spec = TeamSpec { /* ... */ members: (0..9).map(|i| member(&format!("m{i}"))).collect(),
                              ..base_spec() };
    assert!(normalize_spec(&mut spec).is_err());
}
```

### 8.6 Integration test (gated by `#[ignore]`, requires tmux)

```rust
/// Runs only when tmux is present and TMUX env is set. CI invokes with `--ignored`
/// inside a `tmux new-session` wrapper.
#[test]
#[ignore = "requires a live tmux server"]
fn end_to_end_team_lifecycle() {
    // 1. create_team with 2 stub members (MemberSpawner returns fake session ids)
    // 2. assert runtime state file exists with status Active
    // 3. assert tmux has 2 new panes whose titles match member names
    // 4. send a message lead->worker, poll, ack
    // 5. create+claim+complete a task
    // 6. delete_team -> assert panes gone and state = Deleted
}
```

### 8.7 Reference tests to port

oh-my-openagent ships matching `*.test.ts` for every module — port their scenarios:
`team-mailbox/{send,inbox,poll,ack}.test.ts`, `team-tasklist/{claim,update,list,dependencies}.test.ts`,
`team-layout-tmux/{layout,rebalance-team-window,sweep-stale-team-sessions}.test.ts`,
`team-state-store/{locks,store,resume}.test.ts`, `team-runtime/{create,shutdown,status}.test.ts`.

---

## 9. Benchmarks

The mailbox and task store are the hot paths (every inter-agent message and every claim hits
the filesystem under a lock). We use `criterion` and measure on a tmpfs-backed temp dir to
isolate from disk variance.

### 9.1 What to measure

| Metric | Baseline | Target | How to measure |
|--------|----------|--------|----------------|
| `send_message` p50 (1 recipient, 1 KB body) | — | < 1 ms | criterion, tmpfs temp dir |
| `send_message` p99 | — | < 5 ms | criterion (lock contention excluded) |
| `list_unread` p50 (50 messages) | — | < 2 ms | criterion |
| `with_lock` acquire/release p50 (uncontended) | — | < 100 µs | criterion |
| `with_lock` under 4-thread contention | — | < 20 ms p99 | criterion + threads |
| `create_task` p50 | — | < 1 ms | criterion |
| `sweep_stale_team_sessions` (20 sessions) | — | < 50 ms | wall-clock (tmux-bound; gated) |
| Memory per active team (8 members) | — | < 1 MB resident (state cache) | `/proc` RSS delta around create_team |
| tmux pane creation (8 panes) | — | < 500 ms total | wall-clock, integration-gated |

Baselines are filled in on first run; targets are derived from oh-my-openagent's stated
"sub-ms file ops" budget and jcode's general "optimized to the bone" performance posture
(README: 14 ms time-to-first-frame, ~10 MB/extra session).

### 9.2 Benchmark harness (`crates/jcode-swarm-core/benches/team_mailbox.rs`)

```rust
use criterion::{criterion_group, criterion_main, Criterion, BatchSize};
use jcode_swarm_core::team::{mailbox::*, spec::*};
use tempfile::TempDir;

fn bench_send(c: &mut Criterion) {
    c.bench_function("send_message_1kb_single_recipient", |b| {
        b.iter_batched(
            || {
                // setup: fresh temp HOME + run dir per iteration
                let dir = TempDir::new().unwrap();
                unsafe { std::env::set_var("JCODE_TEAMS_BASE_OVERRIDE", dir.path()); }
                let run = uuid::Uuid::new_v4().to_string();
                jcode_swarm_core::team::paths::ensure_base_dirs(&run, &["w".into()]).unwrap();
                (dir, run, 0u64)
            },
            |(_dir, run, mut seq)| {
                seq += 1;
                let msg = TeamMessage {
                    version: 1, message_id: format!("m{seq}"), from: "lead".into(),
                    to: "w".into(), kind: MessageKind::Message,
                    body: "x".repeat(1024), summary: None, references: vec![],
                    timestamp: seq as i64, correlation_id: None, color: None,
                };
                let members = ["w".to_string()];
                let ctx = SendContext { is_lead: true, active_members: &members,
                    reserved_recipients: &[],
                    recipient_unread_max_bytes: TEAM_RECIPIENT_UNREAD_MAX_BYTES };
                send_message(&msg, &run, &ctx).unwrap();
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_list_unread(c: &mut Criterion) {
    c.bench_function("list_unread_50_messages", |b| {
        // setup: prefill 50 messages, then measure a single list_unread.
        let dir = TempDir::new().unwrap();
        unsafe { std::env::set_var("JCODE_TEAMS_BASE_OVERRIDE", dir.path()); }
        let run = uuid::Uuid::new_v4().to_string();
        jcode_swarm_core::team::paths::ensure_base_dirs(&run, &["w".into()]).unwrap();
        let members = ["w".to_string()];
        for i in 0..50 {
            let msg = TeamMessage { version:1, message_id: format!("m{i}"), from:"lead".into(),
                to:"w".into(), kind:MessageKind::Message, body:"hi".into(), summary:None,
                references:vec![], timestamp:i, correlation_id:None, color:None };
            let ctx = SendContext { is_lead:true, active_members:&members,
                reserved_recipients:&[], recipient_unread_max_bytes: usize::MAX };
            send_message(&msg, &run, &ctx).unwrap();
        }
        b.iter(|| { let _ = list_unread(&run, "w").unwrap(); });
    });
}

criterion_group!(benches, bench_send, bench_list_unread);
criterion_main!(benches);
```

Run with: `cargo bench -p jcode-swarm-core` (use `scripts/remote_build.sh` if local resources
are tight, per AGENTS.md).

---

## 10. Migration / Rollout

This **extends** existing code rather than replacing it, so migration is low-risk.

1. **Backward-compatible tool upgrade.** The current `team_create`/`team_delete` write
   `~/.jcode/teams/<name>.json`. The new runtime uses `~/.jcode/teams/runtime/<run_id>/`.
   The two coexist; the old flat config files are ignored by the new code. No on-disk
   migration needed. Optionally, a one-time importer reads any legacy `<name>.json` and seeds
   a `TeamSpec`.
2. **Feature flag.** Gate behind `[team] enabled = false` (default) for the first release,
   plus an env override `JCODE_EXPERIMENTAL_TEAMS=1` (mirrors Claude Code's
   `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`). The TUI widget only appears when a team is active,
   so there is zero UI impact when disabled.
3. **Graceful tmux degradation.** `can_visualize()` returns `false` outside tmux, so
   `create_team` still works headlessly (members spawn, mailbox + tasks function); only the
   pane layout is skipped. This matches oh-my-openagent's "tmux visualization unavailable,
   skipping" behavior and means non-tmux users are never blocked.
4. **Phased landing (PR sequence):**
   - **PR 1** — `team/spec.rs`, `paths.rs`, `locks.rs`, `eligibility.rs` + unit tests (no
     behavior change; pure additions). Land first, fully tested.
   - **PR 2** — `mailbox.rs`, `tasklist.rs` + unit tests.
   - **PR 3** — `layout.rs` + `state.rs` + `runtime.rs` (+ `#[ignore]` integration test).
   - **PR 4** — tool upgrades + `SwarmState.team_runtimes` wiring.
   - **PR 5** — TUI `info_widget_team.rs` + `WidgetKind::TeamView`.
   Each PR builds and passes `cargo test` independently; the feature is only user-reachable
   after PR 4, behind the flag.
5. **Rollback.** Setting `[team] enabled = false` disables tool registration; the modules
   compile but are inert. Removing the feature is deleting the `team/` module + the widget
   variant — no shared state is mutated destructively.
6. **Stale cleanup on upgrade.** On first launch of the new build, run
   `sweep_stale_team_sessions(active=∅)` once to reap any `jcode-team-*` sessions left by
   crashed pre-release experiments.

---

## 11. Known Limitations & Future Work

- [ ] **Tmux-only backend in Phase 1.** `BackendType::InProcess` (Claude Code's
      Shift+Down cycle inside one terminal) is typed but not wired. v2 adds it for users
      without tmux/iTerm2.
- [ ] **Single worktree per team initially.** Cross-worktree teams (each member in its own
      git worktree) are supported in the spec (`worktree_path`) but the integration/merge
      flow (Worktree Manager role from `SWARM_ARCHITECTURE.md`) is deferred.
- [ ] **No live message-injection loop yet.** `poll_messages` exists; wiring it into each
      member's turn loop as a soft-interrupt (jcode already has `SoftInterruptQueue`) is a
      follow-up so peers see messages mid-turn.
- [ ] **Lead is fixed** (matches Claude Code's limitation): no lead hand-off / promotion.
- [ ] **No nested teams** — members cannot spawn their own teams (enforced by eligibility +
      role checks).
- [ ] **Heartbeat-based staleness** for members (vs. only tmux session staleness) — add a
      `last_heartbeat` writer + a reaper that flips members to `errored` after timeout.
- [ ] **Full DAG graph widget** — Phase 1 shows a compact 3-task list; the animated mermaid
      DAG from `SWARM_ARCHITECTURE.md` (Plan info widget) is future work.
- [ ] **Message pruning** at `max_messages_per_run` is specified but the pruning job is a
      follow-up.
- [ ] **Windows tmux** — tmux is unavailable on native Windows; teams there fall back to
      headless (no panes) until the in-process backend lands.

---

## 12. Success Criteria Checklist

- [ ] `team_create` with N≤8 members spawns N headless sessions, ≤4 concurrently, and (in
      tmux) one titled pane per member.
- [ ] Members exchange messages durably via the file mailbox; `send`/`list_unread`/`ack`
      round-trip; broadcast is lead-only; duplicates, oversized payloads, and backpressure
      are rejected with the right `TeamError`.
- [ ] Task board: create with dependencies, atomic claim, validated status transitions; a
      task blocked by an incomplete dependency cannot be claimed.
- [ ] `rebalance` re-tiles panes when membership changes; `sweep_stale_team_sessions` kills
      orphaned `jcode-team-*` sessions and only those.
- [ ] Read-only agent types (`oracle`, `librarian`, `explore`, `metis`, `momus`) are rejected
      at create time with a clear message.
- [ ] TUI `TeamView` widget renders the live roster (status glyph + color + task/msg counts)
      and a compact task list with dependency arrows; it disappears when no team is active.
- [ ] `team_delete` removes panes + runtime dir and marks state `Deleted`; partial-create
      failures clean up (no orphaned panes/worktrees).
- [ ] Bounds enforced: wall-clock, max members, max parallel, message cap, 32 KB body cap.
- [ ] Crash resilience: killing a member mid-run leaves shared state intact; a new build's
      startup sweep reaps stale sessions.
- [ ] `cargo build`, `cargo test -p jcode-swarm-core`, and `cargo clippy` pass with no new
      warnings; existing tests unaffected (feature is additive + flag-gated).

---

> **Status of this document:** complete implementation-grade plan. A junior engineer can take
> §5 module-by-module, paste the Rust, fill the few caller-specific hooks (`MemberSpawner`,
> tmux caller-window resolution, `TeamInfo` feed in `tui_state.rs`), and ship behind the
> `[team] enabled` flag following the §10 PR sequence.
