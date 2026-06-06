# Implementation Plan: Multi-Agent System for jcode
> Generated from research across 9 repos + jcode codebase analysis
> Goal: Full multi-agent orchestration — model-driven delegation, team pipeline, DAG parallelism, agent tree lifecycle

---

## 1. Executive Summary

jcode currently has swarm visualization infrastructure (TUI, protocol, prompts) but **zero agent spawning/driving logic**. The LLM can talk about swarm helpers in prompts, but there's no actual `agent` tool, no agent tree, no sub-agent lifecycle, and no team pipeline.

This plan builds a production-grade multi-agent system by synthesizing the best patterns from codex (AgentPath tree + mailbox, proven in Rust), Claude Code (tool-based delegation, the model drives everything), oh-my-pi (DAG wave parallelism), codebuff (LLM-derived pipeline + cost aggregation), and oh-my-claudecode (team lifecycle + file-based shared state). The result is a three-surface system: **model-driven delegation** (LLM calls `agent` tool), **team pipeline** (CLI-driven multi-step workflow), and **batch processing** (programmatic multi-agent jobs).

---

## 2. Architecture Decision

### Chosen Approach: Hybrid Tree + Tool + Wave

```
┌─────────────────────────────────────────────────────────┐
│                    AgentControl                           │
│  (central registry: tree, threads, names, mailboxes)     │
│                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
│  │ /root        │  │ /root/       │  │ /root/       │   │
│  │ (user        │  │ explorer     │  │ worker       │   │
│  │  session)    │  │ (read-only)  │  │ (execute)    │   │
│  └──────┬───────┘  └──────────────┘  └──────────────┘   │
│         │                                                │
│  ┌──────┴───────┐                                        │
│  │ /root/worker │                                        │
│  │ /code-review │                                        │
│  │ (sub-task)   │                                        │
│  └──────────────┘                                        │
└─────────────────────────────────────────────────────────┘
```

Three delegation modes, one agent tree:

| Mode | Trigger | Use Case | Parallelism |
|------|---------|----------|-------------|
| **Tool-based** | LLM calls `agent` tool | Model decides to delegate | Sync/async/fork |
| **Team pipeline** | `jcode team` CLI | Plan→PRD→Exec→Verify→Fix | DAG wave |
| **Batch** | `jcode agent batch` CSV | Parallel research/review jobs | FuturesUnordered |

### Alternatives Considered

| Approach | Source Repo | Pros | Cons | Decision |
|----------|-------------|------|------|----------|
| AgentPath tree + mailbox | codex | Hierarchical addressing, async decoupling, Rust-native, production-tested | Higher initial complexity | **PRIMARY** — best fit for Rust codebase |
| Tool-based delegation | CC | Model drives everything, simple mental model, proven UX | No automated pipeline | **PRIMARY** — best UX for interactive use |
| DAG wave parallelism | oh-my-pi | Clean dependency resolution, parallel by default | Requires DAG definition upfront | **SECONDARY** — for team pipeline only |
| Centralized orchestrator | codebuff | LLM-pipeline means flexible | Spawning overhead per step | **SECONDARY** — for team pipeline |
| Tmux teams | oh-my-claudecode | Pragmatic, visible | OS-level coupling, fragile | **REFERENCE** — file-based state pattern |
| Single monolithic agent | pi-agent-rust | Simplest, zero overhead | No delegation at all | **REJECTED** — doesn't meet goal |
| Protocol-first | opencode | Clean abstraction | Over-engineered for our needs | **REJECTED** — too abstract |

---

## 3. Data Structures & Types

```rust
// === Core Agent Tree ===

/// Unique path in the agent tree.
/// Examples: "/root", "/root/explorer", "/root/worker/code-review"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPath(Arc<str>);

impl AgentPath {
    pub fn root() -> Self { Self("/root".into()) }
    pub fn parent(&self) -> Option<AgentPath>;
    pub fn child(&self, name: &str) -> AgentPath;
    pub fn is_descendant_of(&self, ancestor: &AgentPath) -> bool;
}

/// Agent identity — registered in AgentControl.
#[derive(Debug, Clone)]
pub struct AgentEntry {
    pub id: AgentId,              // UUID
    pub path: AgentPath,          // Tree position
    pub name: String,             // Human-readable nickname (unique pool)
    pub role: AgentRole,
    pub config: AgentConfig,
    pub state: AgentState,
    pub created_at: Instant,
    pub ancestry: AgentAncestry,  // parent_id, ancestor_ids
    pub mailbox: Option<MailboxSender>,
}

/// Role determines default model, tools, and permissions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AgentRole {
    /// General agent — full tool access, plans and executes
    Default,
    /// Read-only investigator — grep, read, glob, websearch only
    Explorer,
    /// Execute known plan — limited tools, no planning
    Worker,
    /// Orchestrator — delegates subtasks, synthesizes results
    Orchestrator,
}

/// Agent config bundle — inspired by opencode + codex role profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub model: Option<String>,           // None = inherit parent
    pub system_prompt: Option<String>,   // None = inherit, Some = override
    pub tools: AgentToolPolicy,
    pub permissions: AgentPermissionBound,
    pub max_turns: u32,                  // Hard stop
    pub max_cost: Option<f64>,           // Cost cap (USD)
    pub timeout: Option<Duration>,       // Wall-clock timeout
}

/// What tools this agent can use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentToolPolicy {
    /// Inherit parent's tool policy
    Inherit,
    /// Explicit allow list
    Allow(HashSet<String>),
    /// Inherit + add
    Extend(HashSet<String>),
    /// No tools (chat-only)
    None,
}

/// Permission boundary — bubble model from CC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPermissionBound {
    pub max_risk_level: RiskLevel,       // Can't exceed this
    pub allow_approve: bool,             // Can approve own requests
    pub pre_approved: Vec<String>,       // Always-ok tool calls
}

// === Mailbox (from codex) ===

/// One-shot channel for agent communication.
type MailboxSender = tokio::sync::oneshot::Sender<AgentMessage>;
type MailboxReceiver = tokio::sync::oneshot::Receiver<AgentMessage>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub from: AgentPath,
    pub kind: AgentMessageKind,
    pub payload: serde_json::Value,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentMessageKind {
    /// "Do this subtask, report back"
    Task { prompt: String, max_turns: u32 },
    /// "Here are the results"
    Result { output: String, cost: Option<f64> },
    /// "I need more context"
    RequestInfo { question: String },
    /// "Here's the info you requested"
    Info { data: serde_json::Value },
    /// "Stop what you're doing"
    Cancel,
}

// === Agent spawn tool input/output ===

/// The `agent` tool that the LLM calls.
#[derive(Debug, Deserialize)]
pub struct AgentToolInput {
    /// Role: "explorer", "worker", "orchestrator", or "default"
    pub role: String,
    /// What to do
    pub prompt: String,
    /// Sync (wait), async (fire-and-forget), fork (share prompt cache)
    #[serde(default = "default_mode")]
    pub mode: AgentSpawnMode,
    /// Optional tools to add beyond role defaults
    #[serde(default)]
    pub extra_tools: Vec<String>,
    /// Optional max turns for this sub-agent
    #[serde(default = "default_subagent_turns")]
    pub max_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum AgentSpawnMode {
    #[default]
    /// Wait for completion, return result
    Sync,
    /// Fire and forget — results logged but not returned
    Async,
    /// Spawn with current prompt cache — zero cold start
    Fork,
}

/// What the LLM sees after `agent` tool completes.
#[derive(Debug, Serialize)]
pub struct AgentToolOutput {
    pub agent_id: String,
    pub agent_path: String,
    pub result: Option<String>,        // None for async
    pub turn_count: u32,
    pub cost: Option<f64>,
    pub timed_out: bool,
}

// === Agent tree registry ===

/// Central agent tree — thread-safe, tree-addressed.
pub struct AgentControl {
    tree: Arc<RwLock<AgentTreeInner>>,
    name_pool: Arc<Mutex<HashSet<String>>>,
    thread_limits: AgentThreadLimits,
}

struct AgentTreeInner {
    agents: HashMap<AgentPath, AgentEntry>,
    parent_children: HashMap<AgentPath, Vec<AgentPath>>,
    next_id: u64,
}

pub struct AgentThreadLimits {
    pub max_depth: u32,                // Default: 5
    pub max_siblings: u32,             // Default: 10
    pub max_total: u32,                // Default: 50
}

// === DAG pipeline (from oh-my-pi) ===

/// A plan step in the DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub agent_role: AgentRole,
    pub prompt: String,
    pub depends_on: Vec<String>,       // Step IDs that must complete first
    pub timeout: Option<Duration>,
}

/// Wave = set of steps that can run in parallel.
pub struct ExecutionWave {
    pub wave_index: usize,
    pub steps: Vec<PlanStep>,
}
```

---

## 4. Pseudocode — Core Algorithm

### 4a. Spawn Sub-Agent (Tool-Based Delegation)

```
FUNCTION spawn_agent(parent_session, input: AgentToolInput):
  // 1. Validate
  role  = RESOLVE_ROLE(input.role)
  VALIDATE parent_session can spawn(role)
  CHECK AgentControl.thread_limits (depth < max_depth, siblings < max_siblings)

  // 2. Build AgentConfig from role defaults + input overrides
  config = AgentConfig {
    model:     role.default_model ?? parent_session.model,
    tools:     role.default_tools + input.extra_tools,
    permissions: role.default_permissions,
    max_turns: input.max_turns,
    ...
  }

  // 3. Create mailbox
  (tx, rx) = oneshot::channel()

  // 4. Register in AgentTree
  path = parent_session.path.child(autoname())
  entry = AgentEntry { path, role, config, mailbox: tx, ... }
  AgentControl.register(entry)

  // 5. Fire SubagentStart hook
  FIRE_HOOK(SubagentStart { parent_path: parent.path, child_path: path, role })

  // 6. Handle mode:
  IF input.mode == Sync:
    // Run sub-agent in same task, await result
    result = RUN_AGENT_SESSION(config, input.prompt, parent_context)
    AgentControl.complete(path)
    FIRE_HOOK(SubagentStop { path, result })
    RETURN AgentToolOutput { result, ... }

  ELIF input.mode == Async:
    // Spawn separate tokio task, no waiting
    task = tokio::spawn(async {
      result = RUN_AGENT_SESSION(config, input.prompt, parent_context)
      AgentControl.complete(path)
      FIRE_HOOK(SubagentStop { path, result })
    })
    RETURN AgentToolOutput { agent_id: path, result: None, ... }

  ELIF input.mode == Fork:
    // Share parent's prompt cache, zero cold start
    cached_prompt = parent_session.get_prompt_cache()
    task = tokio::spawn(async {
      result = RUN_AGENT_SESSION(config, input.prompt,
                                  parent_context, cached_prompt)
      AgentControl.complete(path)
      FIRE_HOOK(SubagentStop { path, result })
    })
    RETURN AgentToolOutput { agent_id: path, result: None, ... }

  END
END
```

### 4b. Agent Turn Loop (Sub-Agent Runtime)

```
FUNCTION run_agent_session(config, prompt, parent_context, cached_prompt?):
  // 1. Create isolated session context
  session = AgentSession {
    config,
    context: parent_context.clone(),
    prompt_cache: cached_prompt,
    turn_count: 0,
    accumulated_cost: 0.0,
    mailbox: rx from spawn,
  }

  // 2. Execute turn loop
  WHILE session.turn_count < config.max_turns:
    // Check mailbox for parent messages
    IF session.mailbox has message:
      IF message.kind == Cancel:
        RETURN Result { output: "cancelled", ... }
      ELIF message.kind == RequestInfo:
        SEND response back via oneshot
        CONTINUE

    // Normal LLM turn
    response = LLM_CALL(session.context)
    session.turn_count++
    session.accumulated_cost += response.cost

    // Process tool calls
    FOR tool_call in response.tool_calls:
      IF tool_call.name == "agent":
        // Nested delegation — recursive spawn
        sub_result = spawn_agent(session, tool_call.input)
        ADD sub_result to session.context
      ELSE:
        result = EXECUTE_TOOL(tool_call)
        ADD result to session.context

      // Check cost cap
      IF config.max_cost && session.accumulated_cost > config.max_cost:
        RETURN Result { output: "cost limit exceeded", ... }

    // Check if done (no tool calls = final answer)
    IF response.tool_calls is empty:
      RETURN Result { output: response.text, cost: session.accumulated_cost }

  RETURN Result { output: "max turns reached", ... }
END
```

### 4c. Team Pipeline (DAG Wave Execution)

```
FUNCTION execute_team_pipeline(steps: Vec<PlanStep>):
  // 1. Build DAG from depends_on edges
  dag = BUILD_DAG(steps)  // adjacency list + in-degree count

  // 2. Decompose into topological waves
  waves = TOPOLOGICAL_WAVES(dag)
  // Wave 0: steps with no dependencies
  // Wave 1: steps whose deps are all in wave 0
  // ...

  // 3. Execute wave by wave
  step_results = Map<StepId, AgentToolOutput>

  FOR wave in waves:
    // Run all steps in this wave in parallel
    handles = []
    FOR step in wave:
      handle = tokio::spawn(async {
        // Inherit context from parent + prev wave results
        context = BUILD_CONTEXT(step, step_results)
        result = spawn_agent(parent, {
          role: step.agent_role,
          prompt: step.prompt,
          mode: Sync,
        })
        // Store result for dependent steps
        step_results[step.id] = result
      })
      handles.push(handle)

    // Wait for entire wave (fail-one = fail-wave)
    FOR handle in handles:
      await handle

    // Fire wave-complete hook
    FIRE_HOOK(WaveComplete { wave_index: wave.wave_index })

  RETURN step_results
END
```

---

## 5. Implementation Code & Modules

### New Cargo Crate: `jcode-agent-tree`

```
crates/jcode-agent-tree/
  Cargo.toml
  src/
    lib.rs           — re-exports
    path.rs          — AgentPath type
    entry.rs         — AgentEntry, AgentConfig, AgentRole
    control.rs       — AgentControl (registry, thread limits)
    mailbox.rs       — MailboxSender/Receiver, AgentMessage
    serialization.rs — tree save/restore
```

### `path.rs`

```rust
use std::sync::Arc;
use serde::{Serialize, Deserialize};

/// Tree-addressed agent path.
/// Always starts with "/root". Examples:
///   "/root"
///   "/root/explorer"
///   "/root/worker/code-review"
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentPath(Arc<str>);

impl AgentPath {
    pub fn root() -> Self {
        Self("/root".into())
    }

    /// Parse from string — validates format.
    pub fn parse(s: &str) -> Result<Self, AgentPathError> {
        if !s.starts_with('/') {
            return Err(AgentPathError::InvalidFormat);
        }
        if s == "/" {
            return Err(AgentPathError::TooShort);
        }
        // Must not end with /
        if s.ends_with('/') && s.len() > 1 {
            return Err(AgentPathError::TrailingSlash);
        }
        Ok(Self(s.into()))
    }

    /// Create child path: /root/foo + "bar" = /root/foo/bar
    pub fn child(&self, name: &str) -> Self {
        let parent = self.0.as_ref();
        if parent.ends_with('/') {
            Self(format!("{}{}", parent, name).into())
        } else {
            Self(format!("{}/{}", parent, name).into())
        }
    }

    /// Parent path or None if root.
    pub fn parent(&self) -> Option<Self> {
        let s = self.0.as_ref();
        if s == "/root" {
            return None;
        }
        let last_slash = s.rfind('/')?;
        if last_slash == 0 {
            return Some(Self("/root".into()));
        }
        Some(Self(s[..last_slash].into()))
    }

    /// Depth: /root = 0, /root/explorer = 1
    pub fn depth(&self) -> usize {
        self.0.chars().filter(|&c| c == '/').count().saturating_sub(1)
    }

    /// Is this path a descendant of ancestor?
    pub fn is_descendant_of(&self, ancestor: &AgentPath) -> bool {
        let self_s = self.0.as_ref();
        let anc_s = ancestor.0.as_ref();
        self_s.starts_with(anc_s) && self_s.len() > anc_s.len()
            && self_s.as_bytes().get(anc_s.len()) == Some(&b'/')
    }

    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}
```

### `control.rs`

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex, oneshot};
use std::time::Instant;

use crate::path::AgentPath;
use crate::entry::{AgentEntry, AgentRole, AgentConfig, AgentState};

/// Maximum thread limits for safety.
const MAX_DEPTH: u32 = 10;
const MAX_SIBLINGS: u32 = 32;
const MAX_TOTAL: u32 = 200;

/// Central agent tree — thread-safe singleton.
pub struct AgentControl {
    inner: Arc<RwLock<AgentTreeInner>>,
    name_pool: Arc<Mutex<NamePool>>,
    limits: AgentThreadLimits,
}

struct AgentTreeInner {
    agents: HashMap<AgentPath, AgentEntry>,
    parent_children: HashMap<AgentPath, Vec<AgentPath>>,
    next_id: u64,
}

pub struct AgentThreadLimits {
    pub max_depth: u32,
    pub max_siblings: u32,
    pub max_total: u32,
}

impl Default for AgentThreadLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_DEPTH,
            max_siblings: MAX_SIBLINGS,
            max_total: MAX_TOTAL,
        }
    }
}

impl AgentControl {
    pub fn new() -> Self {
        let inner = AgentTreeInner {
            agents: HashMap::new(),
            parent_children: HashMap::new(),
            next_id: 1,
        };
        Self {
            inner: Arc::new(RwLock::new(inner)),
            name_pool: Arc::new(Mutex::new(NamePool::new())),
            limits: AgentThreadLimits::default(),
        }
    }

    /// Register a new agent in the tree.
    /// Returns error if thread limits would be exceeded.
    pub async fn register(
        &self,
        parent_path: &AgentPath,
        name: &str,
        role: AgentRole,
        config: AgentConfig,
        mailbox: oneshot::Sender<...>,
    ) -> Result<AgentPath, AgentControlError> {
        let mut inner = self.inner.write().await;

        // Check max total
        if inner.agents.len() as u32 >= self.limits.max_total {
            return Err(AgentControlError::MaxTotalAgents);
        }

        // Check depth
        let depth = parent_path.depth() + 1;
        if depth > self.limits.max_depth {
            return Err(AgentControlError::MaxDepth(depth));
        }

        // Check siblings
        let siblings = inner.parent_children.get(parent_path)
            .map(|v| v.len())
            .unwrap_or(0);
        if siblings >= self.limits.max_siblings as usize {
            return Err(AgentControlError::MaxSiblings(siblings));
        }

        // Generate unique name
        let unique_name = self.name_pool.lock().unwrap()
            .allocate(name);

        let path = parent_path.child(&unique_name);
        let id = inner.next_id;

        let entry = AgentEntry {
            id,
            path: path.clone(),
            name: unique_name.clone(),
            role,
            config,
            state: AgentState::Spawning,
            created_at: Instant::now(),
            mailbox,
        };

        inner.agents.insert(path.clone(), entry);
        inner.parent_children
            .entry(parent_path.clone())
            .or_default()
            .push(path.clone());
        inner.next_id += 1;

        Ok(path)
    }

    /// Find agent by path.
    pub async fn get(&self, path: &AgentPath) -> Option<AgentEntry> {
        self.inner.read().await.agents.get(path).cloned()
    }

    /// List children of a path.
    pub async fn children(&self, path: &AgentPath) -> Vec<AgentPath> {
        self.inner.read().await
            .parent_children.get(path)
            .cloned()
            .unwrap_or_default()
    }

    /// Shutdown an agent and all its descendants (recursive).
    pub async fn shutdown_tree(&self, path: &AgentPath) {
        let mut inner = self.inner.write().await;
        let children = inner.parent_children.get(path).cloned().unwrap_or_default();

        for child_path in &children {
            if let Some(entry) = inner.agents.get(child_path) {
                if let Some(tx) = &entry.mailbox {
                    let _ = tx.send(AgentMessage::shutdown());
                }
            }
        }
        // Remove from parent's children list
        if let Some(parent) = path.parent() {
            if let Some(siblings) = inner.parent_children.get_mut(&parent) {
                siblings.retain(|p| p != path);
            }
        }
        inner.agents.remove(path);
    }

    /// Complete an agent (success or failure)
    pub async fn complete(&self, path: &AgentPath, state: AgentState) {
        let mut inner = self.inner.write().await;
        if let Some(entry) = inner.agents.get_mut(path) {
            entry.state = state;
        }
    }

    /// Serialize the agent tree for display.
    pub async fn snapshot(&self) -> Vec<AgentEntry> {
        self.inner.read().await.agents.values().cloned().collect()
    }
}

// === Name pool (unique agent nicknames) ===

struct NamePool {
    used: HashSet<String>,
    counters: HashMap<String, u64>,
}

impl NamePool {
    fn new() -> Self {
        Self {
            used: HashSet::new(),
            counters: HashMap::new(),
        }
    }

    fn allocate(&mut self, base: &str) -> String {
        let counter = self.counters.entry(base.to_string()).or_insert(0);
        *counter += 1;
        let name = format!("{}-{}", base, *counter);
        self.used.insert(name.clone());
        name
    }
}
```

### Modifications to Existing Files

#### `crates/jcode-app-core/src/agent/mod.rs` — New `agent` tool

```rust
/// The `agent` tool — lets the LLM spawn sub-agents.
pub struct AgentTool {
    agent_control: Arc<AgentControl>,
    session_registry: Arc<SessionRegistry>,
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str { "agent" }
    fn description(&self) -> &str {
        "Spawn a sub-agent to work on a task. Use sync mode to get the result back, \
         async for fire-and-forget, fork to reuse the current prompt cache. \
         Roles: explorer (read-only), worker (execute), orchestrator (plan+delegate)."
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> ToolOutput {
        let input: AgentToolInput = serde_json::from_value(input)?;
        // Validate role
        let role = AgentRole::from_str(&input.role)
            .map_err(|_| ToolError::InvalidParam("role"))?;

        // Build config from role defaults + overrides
        let config = self.build_config(&ctx, role, &input);

        // Create mailbox
        let (tx, rx) = oneshot::channel();

        // Register in tree
        let parent_path = ctx.agent_path();  // from session runtime
        let path = self.agent_control.register(
            &parent_path, &role.to_string(), role, config, tx
        ).await?;

        // Fire hook
        fire_hook(HookEvent::SubagentStart {
            parent: parent_path.to_string(),
            child: path.to_string(),
            role: role.to_string(),
        }).await;

        // ... spawn session and run ...
    }
}
```

#### `src/cli/args.rs` — New subcommands

```rust
pub(crate) enum Command {
    // ... existing ...
    /// Multi-agent team orchestration
    #[command(subcommand)]
    Team(TeamCommand),
    /// Sub-agent tree management
    #[command(subcommand)]
    Agent(AgentCommand),
}

#[derive(Subcommand)]
pub(crate) enum TeamCommand {
    /// Start a team pipeline from a plan file
    Start {
        /// Path to plan file (YAML/TOML)
        plan: PathBuf,
        /// Number of parallel workers
        #[arg(long, default_value = "4")]
        workers: u32,
    },
    /// Show team status
    Status,
    /// Stop a running team
    Stop {
        /// Team ID (from `team start`)
        team_id: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum AgentCommand {
    /// List all sub-agents in tree
    List,
    /// Show agent tree
    Tree,
    /// Kill a sub-agent by path
    Kill {
        path: String,
    },
    /// Get agent status
    Status {
        path: String,
    },
}
```

#### `src/cli/dispatch.rs` — Route new commands

```rust
Command::Team(cmd) => {
    match cmd {
        TeamCommand::Start { plan, workers } => {
            let plan = parse_plan_file(&plan)?;
            runtime.execute_team_pipeline(plan, workers).await?;
        }
        TeamCommand::Status => {
            let tree = runtime.agent_control().snapshot().await;
            // Print formatted table
        }
        TeamCommand::Stop { team_id } => {
            runtime.agent_control()
                .shutdown_tree(&AgentPath::parse(&format!("/root/{}", team_id))?)
                .await;
        }
    }
}
```

#### Integration into Agent Turn Loop

In `turn_streaming_mpsc.rs`, the existing soft-interrupt points already provide hooks for sub-agent injection:

- **Point A (pre-API)**: Check sub-agent mailbox for incoming messages (Cancel, RequestInfo)
- **Point B (post-response)**: Process `agent` tool calls from the model
- **Point C (between tools)**: Check for sub-agent result availability
- **Point D (after all tools)**: Fire SubagentStop hooks, propagate results

```rust
// In the agent turn loop, after tool call processing:
if tool_call.name == "agent" {
    let input: AgentToolInput = serde_json::from_value(tool_call.input)?;
    let result = AgentTool::execute(input, ctx).await;
    // result goes back as a regular tool result
    context.add_tool_result(tool_call.id, result);
}
```

---

## 6. Configuration & Wiring

### `~/.jcode/config.toml` — Agent section

```toml
[agents]
# Max sub-agents in the tree
max_total = 50
# Max delegation depth
max_depth = 5
# Max siblings per parent
max_siblings = 10
# Default agent timeout
default_timeout = "300s"
# Default max turns
default_max_turns = 50

[agents.roles.explorer]
model = "claude-sonnet-4-20250514"
tools = ["read", "grep", "glob", "websearch", "web_fetch"]
max_turns = 20
permissions = { max_risk_level = "read_only", allow_approve = false }

[agents.roles.worker]
model = "claude-sonnet-4-20250514"
tools = ["read", "write", "edit", "bash", "grep", "glob"]
max_turns = 50
permissions = { max_risk_level = "standard", allow_approve = false }

[agents.roles.orchestrator]
model = "claude-opus-4-20250514"
tools = "*"    # All available tools
max_turns = 30
permissions = { max_risk_level = "elevated", allow_approve = true }
```

### Env Vars (in `disable-registry` style)

| Env Var | Effect |
|---------|--------|
| `JCODE_DISABLE_AGENT_TREE=1` | Disable all multi-agent features |
| `JCODE_MAX_AGENTS=10` | Override max_total at process level |
| `JCODE_AGENT_TIMEOUT_MS=60000` | Per-agent timeout override |

### Integration Points Checklist

| File | Change | Priority |
|------|--------|----------|
| `Cargo.toml` (workspace) | Add `jcode-agent-tree` crate | P0 |
| `crates/jcode-agent-tree/src/lib.rs` | New crate — AgentPath, AgentTree, Mailbox | P0 |
| `crates/jcode-app-core/src/tool/mod.rs` | Register `AgentTool` | P0 |
| `crates/jcode-app-core/src/agent/turn_streaming_mpsc.rs` | Handle `agent` tool calls in turn loop | P0 |
| `src/cli/args.rs` | Add `Team` + `Agent` subcommands | P1 |
| `src/cli/dispatch.rs` | Route team/agent commands | P1 |
| `crates/jcode-base/src/config.rs` | Add `[agents]` config section | P1 |
| `crates/jcode-protocol/src/wire.rs` | Add SubagentStart/Stop events | P1 |
| `crates/jcode-tui/src/tui/app.rs` | Display agent tree in side panel | P2 |
| `crates/jcode-tui/src/tui/ui.rs` | Agent tree widget | P2 |

---

## 7. Repo References

| Feature Aspect | Repo | File | Link |
|----------------|------|------|------|
| AgentPath tree | codex | cli/kernel/agents/agent_path.rs | https://github.com/openai/codex/blob/main/cli/kernel/agents/agent_path.rs |
| Mailbox | codex | cli/kernel/agents/mailbox.rs | https://github.com/openai/codex/blob/main/cli/kernel/agents/mailbox.rs |
| AgentControl | codex | cli/kernel/agents/agent_control.rs | https://github.com/openai/codex/blob/main/cli/kernel/agents/agent_control.rs |
| Batch CSV | codex | cli/kernel/agents/spawn.rs | https://github.com/openai/codex/blob/main/cli/kernel/agents/spawn.rs |
| Agent tool | CC | src/tools/agent.ts | https://github.com/claude-code-best/claude-code/blob/main/src/tools/agent.ts |
| Subagent hooks | CC | src/services/hooks.ts | https://github.com/claude-code-best/claude-code/blob/main/src/services/hooks.ts |
| DAG wave | oh-my-pi | src/agent/swarm/DAGSwarm.ts | https://github.com/can1357/oh-my-pi/blob/main/src/agent/swarm/DAGSwarm.ts |
| EventBus | oh-my-pi | src/agent/EventBus.ts | https://github.com/can1357/oh-my-pi/blob/main/src/agent/EventBus.ts |
| Pipeline orchestration | codebuff | src/orchestrator/Buffy.ts | https://github.com/CodebuffAI/codebuff/blob/main/src/orchestrator/Buffy.ts |
| Team pipeline | oh-my-claudecode | src/team/index.ts | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/src/team/index.ts |
| Spawn agent | oh-my-openagent | src/agents/agentOrchestration.ts | https://github.com/code-yeongyu/oh-my-openagent/blob/main/src/agents/agentOrchestration.ts |
| Fork subagent | oh-my-claudecode | src/team/agents.ts | https://github.com/Yeachan-Heo/oh-my-claudecode/blob/main/src/team/agents.ts |
| Agent posture gating | oh-my-codex | src/orchestrator/posture.ts | https://github.com/Yeachan-Heo/oh-my-codex/blob/main/src/orchestrator/posture.ts |
| jcode existing swarm TUI | jcode | crates/jcode-tui/src/tui/app.rs | — |
| jcode existing orchestration API | jcode | src/orchestration_api.rs | — |

---

## 8. Test Cases

### Unit Tests

```rust
// === AgentPath tests ===
#[test]
fn test_agent_path_root() {
    let root = AgentPath::root();
    assert_eq!(root.as_str(), "/root");
    assert_eq!(root.depth(), 0);
    assert!(root.parent().is_none());
}

#[test]
fn test_agent_path_child() {
    let root = AgentPath::root();
    let explorer = root.child("explorer");
    assert_eq!(explorer.as_str(), "/root/explorer");
    assert_eq!(explorer.depth(), 1);
    assert_eq!(explorer.parent().unwrap().as_str(), "/root");
}

#[test]
fn test_agent_path_is_descendant() {
    let root = AgentPath::root();
    let worker = root.child("worker");
    let task = worker.child("code-review");
    assert!(task.is_descendant_of(&root));
    assert!(task.is_descendant_of(&worker));
    assert!(!worker.is_descendant_of(&task));
}

#[test]
fn test_agent_path_parse_valid() {
    let p = AgentPath::parse("/root/explorer").unwrap();
    assert_eq!(p.as_str(), "/root/explorer");
}

#[test]
fn test_agent_path_parse_invalid() {
    assert!(AgentPath::parse("/").is_err());
    assert!(AgentPath::parse("root").is_err());
}

// === AgentControl tests ===

#[tokio::test]
async fn test_register_agent() {
    let ctrl = AgentControl::new();
    let root = AgentPath::root();
    let (tx, _rx) = oneshot::channel();

    let path = ctrl.register(&root, "explorer", AgentRole::Explorer,
        AgentConfig::default(), tx).await.unwrap();

    assert!(path.as_str().starts_with("/root/explorer-"));
    assert!(ctrl.get(&path).await.is_some());
}

#[tokio::test]
async fn test_max_depth_enforced() {
    let ctrl = AgentControl::new();
    let mut path = AgentPath::root();
    for i in 0..12 {   // max_depth = 10
        let (tx, _rx) = oneshot::channel();
        let result = ctrl.register(&path, "deep", AgentRole::Worker,
            AgentConfig::default(), tx).await;
        if i >= 10 {
            assert!(result.is_err());
        } else {
            path = result.unwrap();
        }
    }
}

#[tokio::test]
async fn test_shutdown_tree() {
    let ctrl = AgentControl::new();
    let root = AgentPath::root();
    let (tx1, _rx1) = oneshot::channel();
    let (tx2, _rx2) = oneshot::channel();
    let p1 = ctrl.register(&root, "a", AgentRole::Explorer,
        AgentConfig::default(), tx1).await.unwrap();
    let p2 = ctrl.register(&p1, "b", AgentRole::Worker,
        AgentConfig::default(), tx2).await.unwrap();

    ctrl.shutdown_tree(&root).await;
    assert!(ctrl.get(&p1).await.is_none());
    assert!(ctrl.get(&p2).await.is_none());
}

// === AgentTool tests ===

#[tokio::test]
async fn test_agent_tool_spawn_sync() {
    // Setup: create session, register AgentTool, call with input
    let tool = AgentTool::new(agent_control, session_registry);
    let input = serde_json::json!({
        "role": "explorer",
        "prompt": "Check if Cargo.toml exists",
        "mode": "sync"
    });
    let ctx = ToolContext::test();
    let output = tool.execute(input, ctx).await;
    assert!(output.result.is_some());
    assert!(output.turn_count > 0);
}

#[tokio::test]
async fn test_agent_tool_invalid_role() {
    let tool = AgentTool::new(agent_control, session_registry);
    let input = serde_json::json!({
        "role": "superhero",  // Invalid
        "prompt": "Do something"
    });
    let result = tool.execute(input, ToolContext::test()).await;
    assert!(result.is_err());
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_subagent_result_propagates_to_parent() {
    // 1. Start parent session via orchestration API
    // 2. Parent calls `agent` tool with sync mode
    // 3. Sub-agent runs, does some work, returns result
    // 4. Verify parent's next turn includes sub-agent result
    todo!("End-to-end: spawn parent → parent spawns child → child returns → parent sees result");
}

#[tokio::test]
async fn test_agent_tree_persistence() {
    // 1. Create agent tree with multiple agents
    // 2. Serialize to JSON
    // 3. Deserialize
    // 4. Verify all paths and entries match
    todo!("Agent tree save/restore round-trip");
}

#[tokio::test]
async fn test_team_pipeline_dag_wave() {
    // 1. Define 5-step DAG: step2 depends on step1, step3 on step1, step4 on step2+3
    // 2. Execute pipeline
    // 3. Verify wave order: wave0=[step1], wave1=[step2,step3], wave2=[step4]
    // 4. Verify all results present
    todo!("DAG execution respects topological order");
}
```

---

## 9. Benchmarks

| Metric | Baseline (no multi-agent) | Target | How to Measure |
|--------|---------------------------|--------|----------------|
| Sub-agent spawn latency | N/A | < 100ms (in-process) | `time` before/after `register()` call |
| Sub-agent LLM first-token | N/A | Same as parent (fork) + 500ms (sync) | Measure TTFT of sub-agent vs parent |
| Memory per sub-agent | N/A | < 50MB baseline + 10MB per active agent | `alloc` profiling |
| Agent tree — 100 agents | N/A | Lookup < 1µs, register < 10µs | Criterion bench |
| DAG wave — 20 steps / 4 waves | N/A | Total < serial time / 3 | Integration timer |
| Cost tracking overhead | N/A | < 0.1% of total API cost | Differential measurement |

---

## 10. Migration / Rollout

**Phase 1 — Foundation (estimate: 1-2 weeks)**
- New crate `jcode-agent-tree` with AgentPath, AgentControl, Mailbox
- Unit tests for tree operations
- No agent tool yet — infrastructure only
- **Risk**: None (new crate, no existing code touched)

**Phase 2 — Agent Tool (estimate: 1 week)**
- `AgentTool` implementation: sync + async + fork modes
- Integration into agent turn loop
- Wire hooks (SubagentStart/SubagentStop) to existing hook system
- **Risk**: Medium — turn loop changes must not break single-agent mode

**Phase 3 — CLI + Config (estimate: 1 week)**
- `jcode agent list/tree/kill/status` commands
- `jcode team start/status/stop` commands
- `[agents]` config section in config.toml
- **Risk**: Low — CLI and config are additive

**Phase 4 — Team Pipeline + Batch (estimate: 1 week)**
- DAG pipeline executor (plan file → waves → results)
- Batch CSV agent spawning
- TUI agent tree visualization
- **Risk**: Low — builds on Phase 1-3 foundation

### Feature Flag
All multi-agent functionality gated behind `JCODE_DISABLE_AGENT_TREE` kill-switch (from disable-env system). When disabled, `agent` tool returns "multi-agent disabled" error, team CLI commands error out, and agent tree stays empty.

---

## 11. Known Limitations & Future Work

- [ ] **Cross-process sub-agents**: Current design is in-process only. Future: sub-agents as separate `jcode` processes via the protocol layer.
- [ ] **Agent checkpoint/resume**: Sub-agents that survive parent restart — requires session persistence.
- [ ] **Prompt cache sharing (Fork)**: Full fork mode requires the LLM provider to support prompt cache snapshots. Phase 1 fork = copy context (not true cache sharing).
- [ ] **Inter-agent streaming**: Sub-agents can only communicate via mailbox messages (discrete), not streaming. Future: SSE-based streaming between agents.
- [ ] **Cost optimization**: No sub-agent cost optimization yet (e.g., cheaper model for explorer).
- [ ] **Agent governance**: No per-user agent quotas, no team-based agent pools.
- [ ] **Swarm replay export**: jcode already has `export_swarm_video()` in the TUI — tie this into agent tree history.

---

## 12. Success Criteria Checklist

- [ ] `AgentPath` type supports hierarchical addressing, parent/child traversal, depth checks
- [ ] `AgentControl` enforces thread limits (depth, siblings, total)
- [ ] Mailbox-based communication works: parent sends task, agent receives, agent sends result, parent receives
- [ ] `agent` tool call spawns a sub-agent with correct role defaults
- [ ] Sync mode: parent waits, gets result with turn count + cost
- [ ] Async mode: parent continues immediately, result logged
- [ ] SubagentStart/SubagentStop hooks fire correctly
- [ ] `jcode agent list` shows all active agents with paths
- [ ] `jcode agent kill /root/worker-1` terminates agent + children
- [ ] `jcode agent tree` prints hierarchical tree view
- [ ] `jcode team start` reads plan file, executes waves, reports results
- [ ] `jcode team stop <id>` cancels all running agents in team
- [ ] DAG pipeline executes steps in correct topological wave order
- [ ] Cost aggregation: parent's cost includes all children's costs
- [ ] `JCODE_DISABLE_AGENT_TREE=1` disables all multi-agent features
- [ ] Existing single-agent behavior unchanged (regression test pass)
- [ ] 50 concurrent agents don't overwhelm the runtime
