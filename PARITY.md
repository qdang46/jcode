# jcode Feature Registry

> Feature inventory tracking implementation status and source references across reference repos (Claude Code, opencode, codebuff, pi-agent-rust, oh-my-openagent, codex, oh-my-pi, oh-my-claudecode, oh-my-codex).  
> Organized by feature domain. New features should be added to the appropriate section.

> **⚠️ DISCLAIMER:** This registry is a living document. Features listed here have been 
> preliminarily checked against the codebase but are **not guaranteed to be complete or 
> fully verified**. Many gaps, missing features, and unimplemented capabilities exist 
> beyond what is tracked here. Treat each row as a best-effort snapshot, not a 
> certification. Contributions and corrections welcome.


---

## I. Subagent

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Implemented and shipped |
| ⚠️ | Partial — works but missing depth |
| ❌ | Not yet implemented |
| 🔜 | Planned for next milestone |

---
### 1. Agent Running Items

*Interactive list below status bar showing live agents, tools, and tasks.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Running items list** | Interactive list below status bar. Shows subagents, shell commands, background tasks. ↓/↑ navigate, Enter detail, Esc close. Toggle via Ctrl+O. | CCB (running items), opencode (task list) | `ui_running_items.rs`, `ui.rs` chunks[8], `input.rs` Ctrl+O | ✅ | — |
| **Status icons** | Running ◯, Completed ✓, Failed ✗, Stopped ■. Colored by status. | CCB (status icons) | `item_icon_and_color()` in `ui_running_items.rs` | ✅ | — |
| **Elapsed time display** | Duration shown for running items. Right-aligned. | CCB (timestamps) | `format_elapsed()` in `ui_running_items.rs` | ✅ | — |
| **Selection highlight** | ❯ prefix + bold label for selected item. | CCB (arrow navigation) | `draw_running_items()` selection styling | ✅ | — |
| **Scroll overflow** | Max 5 items visible. Scroll offset for overflow. | CCB (pagination) | `scroll_offset` in `draw_running_items()` | ✅ | — |

---

### 2. Agent Detail Overlay

*Popup showing live agent/tool information.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Detail popup** | Rounded border overlay showing item info. | CCB (AgentDetail), opencode (detail view) | `draw_running_item_detail()` in `ui_running_items.rs` | ✅ | — |
| **Real-time update** | Content rebuilt every frame. Status/elapsed update live. | CCB (live update) | Called from `draw_inner()` each frame | ✅ | — |
| **Detail fields** | Status, kind, id, session id, elapsed, detail text. | CCB (AgentDetail.tsx) | Dynamic content per frame | ✅ | — |
| **Action hints** | Context-sensitive: "Enter to open session", "Ctrl+C to cancel", "Esc to close". | CCB (action hints) | Dynamic hints based on status + session_id | ✅ | — |
| **Cancel action** | Backspace or Ctrl+C to cancel running item. | CCB (stopTask), codex (interrupt) | `input.rs`: `cancel_requested = true` | ✅ | — |

---

### 3. Agent Session Attachment

*Switching to a running agent's session to view transcript.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Attach to session** | Enter on subagent item → switch to that agent's session via `queue_resume_session(sid)`. | CCB (session switch) | `input.rs`, `key_handling.rs` | ✅ | — |
| **View transcript** | See agent's conversation history after attaching. | CCB (transcript view) | Session resume → full transcript render | ✅ | — |
| **Inter-agent messaging** | Agents communicate via shared context and notifications. | CCB (teammateMailbox), oh-my-openagent (delegate-task) | `ServerEvent::Notification`, `CommReadContext` | ✅ | — |
| **Agent context visualization** | Per-agent token usage display. | CCB (context command), opencode (context widget) | `info_widget.rs`: ContextUsage widget with token counts and color thresholds | ✅ | — |

---

### 4. Agent Definitions

*File format, storage, loading, validation.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **File format** | TOML-based definition. Fields: id, display_name, model_override, tool_names, system_prompt, instructions_prompt, step_prompt, spawner_prompt, inherit_parent_system_prompt, include_message_history, permission_mode, max_turns, output_mode, output_schema, color, reasoning. | CCB (YAML frontmatter), pi-agent-rust (config format) | `definition.rs`: `AgentDefinition` struct | ✅ | — |
| **Registry** | 3-tier priority: Builtin < UserGlobal < ProjectLocal. load_directory, register_builtin, iter_sorted, conflict resolution. | CCB (4 scopes), pi-agent-rust (registry) | `registry.rs`: `AgentRegistry` | ✅ | — |
| **Storage scopes** | Agent file directories. | CCB (managed/project/user/plugin) | `~/.jcode/agents/`, `.jcode/agents/` | ✅ | Plugin scope pending (managed done). |
| **Validation** | Validate agent file on load. Error/warning reporting. | CCB (AgentValidationResult) | `AgentDefinition::validate()` | ✅ | — |
| **Prompt system** | 5 prompt slots. Cache sharing via inherit_parent_system_prompt (prompt cache prefix trick). | CCB (AgentTool prompts), oh-my-openagent (per-model prompts) | `definition.rs`: system/instructions/step/spawner prompts | ✅ | — |
| **Snapshot update notification** | Detect agent file changes since last session. Show notification on startup. | CCB (SnapshotUpdateDialog) | `check_agent_snapshots()` in `openers.rs`. Runs at startup, compares mtime. | ✅ | — |

---

### 5. Agent Lifecycle

*Spawning, execution, completion, background.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Spawning** | Spawn subagent from parent session. Context inheritance, tool/config pass-through. | CCB (spawnInProcess), oh-my-openagent (delegate-task), codebuff (4-agent pipeline) | Agent runtime via AgentTarget + model resolution | ✅ | — |
| **Lifecycle states** | Start → running → completed/failed/stopped. Visible in UI. | CCB (LocalAgentTask) | `running_items.rs` status icons. `SwarmMemberStatus` from server events. | ✅ | — |
| **Background execution** | Non-blocking agent execution. Progress tracking, notifications, wake. | CCB (BackgroundAgentTasks), pi-agent-rust (background scheduling) | `background::global()`, `BackgroundTaskManager` | ✅ | — |
| **Forked agents** | Fork with full context inheritance. In-process execution. | CCB (forkedAgent.ts, inProcessRunner) | In-process spawning via agent runtime | ✅ | — |
| **Max turns** | Limit agent turns to prevent runaway loops. | CCB (maxTurns), codex (safety limits) | `definition.rs`: `max_turns: Option<u32>` | ✅ | — |
| **Stop/kill** | Cancel running subagent, tool, or background task. | CCB (stopTask, useCancelRequest), codex (interrupt) | Ctrl+C / Backspace → `cancel_requested = true` | ✅ | — |

---

### 6. Tool & Permission System

*Per-agent tool restrictions and permission modes.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Tool whitelist** | `tool_names`: only these tools available to agent. | CCB (tools field), codex (sandbox) | `definition.rs`: `tool_names: Vec<String>` | ✅ | — |
| **Tool denylist** | `disallowed_tools`: block specific tools. | CCB (tool deny), oh-my-pi (tool gating) | `definition.rs`: `disallowed_tools: Vec<String>` | ✅ | — |
| **Spawnable agents** | `spawnable_agents`: which sub-agents can be spawned. | CCB (spawn control) | `definition.rs`: `spawnable_agents: Vec<String>` | ✅ | — |
| **Permission mode** | Per-agent override (Plan, AcceptEdits, etc.). | CCB (permissionMode), codex (execution policy), oh-my-claudecode (hooks) | `definition.rs`: `permission_mode: Option<PermissionMode>` | ✅ | — |
| **Reasoning effort** | Per-agent reasoning level (minimal/low/medium/high). | CCB (effort), oh-my-openagent (model-variant routing) | `definition.rs`: `reasoning: Option<ReasoningEffort>` | ✅ | — |

---

### 7. Agent Colors

*Visual agent identity via named colors.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Color field** | 8 named colors: red/blue/green/yellow/purple/orange/pink/cyan. Stored in agent definition. | CCB (AgentColorName, agentColorManager.ts) | `definition.rs`: `color: Option<String>` | ✅ | — |
| **Color badge** | Colored badge displayed in agent list. | CCB (color badge in AgentsList) | `agent_color_icon()` → emoji per color: ❤💙💚💛💜🧡 | ✅ | — |
| **Color picker** | Interactive UI to choose agent color from 8 swatches + "Automatic". | CCB (ColorPicker.tsx) | `open_color_picker()` with 9 entries, wired into Library tab column 1 | ✅ | — |

---

### 8. `/agents` Command

*Tabbed agent management interface.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Command entry** | `/agents` opens agent management UI. | CCB (agents/index.ts) | `/agents` → `open_agents_picker()` | ✅ | — |
| **Tab switching** | Tab/BackTab/→/← switch Running ↔ Library. | CCB (tab interface) | `inline_interactive.rs`: column switch | ✅ | — |
| **Running tab** | Live subagents, background tasks, batch tools, swarm members. Enter → open running items list. | CCB (Running tab) | `build_running_tab_entries()` in `openers.rs` | ✅ | — |
| **Library tab** | Agent files from disk + create/generate/model override entries. | CCB (AgentsList.tsx) | Load from AgentRegistry + action entries | ✅ | — |
| **Enter on agent file** | Open $EDITOR with agent TOML for editing. | CCB (AgentEditor.tsx) | `PickerAction::EditAgent` → `$EDITOR` | ✅ | — |
| **Enter on model config** | Open model picker for built-in agent override. | CCB (model field) | `PickerAction::AgentTarget` → `open_agent_model_picker()` | ✅ | — |
| **Delete action** | Remove agent file from disk. | CCB (deleteAgentFromFile) | `PickerAction::DeleteAgent` → `std::fs::remove_file()` | ✅ | — |

---

### 9. Agent Creation

*Creating new agent definitions.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **AI generation** | Open $EDITOR with prompt template. User describes agent. Queue to current model. | CCB (generateAgent.ts — Claude API) | `PickerAction::GenerateAgent` → `queued_messages.push()` | ⚠️ | Response in chat. Must manually save. AI auto-save handles this. |
| **`/agents save`** | Save generated agent TOML from last model response. | CCB (auto-save after AI gen) | `save_last_assistant_as_agent()` in `openers.rs` | ✅ | — |
| **AI auto-save** | Model generates → auto-parse → auto-save. Zero manual steps. | CCB (generateAgent → programmatic save) | `auto_save_turn_agent()` in `local.rs` finish_turn hook | ✅ | — |
| **Creation wizard** | Multi-step guided wizard: location → method → type → prompt → tools → model → color → confirm. | CCB (CreateAgentWizard.tsx — 10+ steps) | `open_creation_wizard()` in `openers.rs` (3-step: name → desc → $EDITOR) | ✅ | — |
| **Edit menu** | Change model/tools/color via pickers, not raw file editing. | CCB (AgentEditor.tsx) | `SetAgentColor` via Library tab column 1, `SetAgentTools` via `open_tools_picker()` (16 tools), model picker via column 2 | ✅ | — |

---

### 10. `/tasks` Command

*Standalone background task management.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Command entry** | `/tasks` lists running/completed background tasks. | CCB (tasks/index.ts, tasks.tsx) | `/tasks` → opens running items list (Ctrl+O) | ✅ | — |
| **Attach to task** | Enter on task → view output/attach to session. | CCB (task attach) | Enter on task in running items → detail → session attach | ✅ | — |
| **Stop/kill task** | Cancel background task from task list. | CCB (stopTask) | Backspace/Ctrl+C in running items detail | ✅ | — |
---

### 11. Agent Teams & Swarm

*Multi-agent coordination.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Swarm members** | Remote swarm member lifecycle. Status via ServerEvent::SwarmStatus. | CCB (swarm backends) | `remote_swarm_members: Vec<SwarmMemberStatus>` | ✅ | — |
| **Swarm plan** | Plan synchronization. Plan proposals, coordinator mode. | CCB (coordinatorMode) | `swarm_plan_core.rs`, `ServerEvent::SwarmPlan` | ✅ | — |
| **Info widget** | Swarm member status in margin. Icons, names, roles. | CCB (teammate banner) | `info_widget_swarm_background.rs`: `render_swarm_widget()` | ✅ | — |
| **Agent teams** | Multi-agent task DAG. Team coordination. Interactive teammate view panel. | oh-my-openagent (Atlas/delegate-task), codebuff (4-agent pipeline), CCB (teams) | TeamView widget + `TeamViewInteraction` struct + teammate view panel + output_tail | ⚠️ | `TeamViewInteraction` struct added. Wire keyboard dispatch + claim/close actions. |

### 12. Built-in Agents

*Pre-shipped agent definitions.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **basher** | Run terminal commands. One-shot bash executor. prefer_tier=routine, max_turns=10, permission_mode=accept-edits. | codebuff (bash agent), CCB (shell tools) | `.jcode/agents/basher.toml`. color=green. | ✅ | — |
| **code-reviewer** | Review code changes for bugs and regressions. prefer_tier=thinking, inherit_parent_system_prompt=true, permission_mode=plan. | codebuff (reviewer agent) | `.jcode/agents/code-reviewer.toml`. color=purple. | ✅ | — |
| **editor** | Precise code edits with hashline_edit. prefer_tier=thinking, inherit_parent_system_prompt=true, permission_mode=accept-edits. | oh-my-pi (hashline_edit), CCB (editor) | `.jcode/agents/editor.toml`. color=blue. | ✅ | — |
| **planner** | Create step-by-step plans for complex tasks. Read-only, uses beads/tasks. Analysis-first approach. prefer_tier=thinking, reasoning=high, permission_mode=plan. | codebuff (planner agent) | `.jcode/agents/planner.toml`. color=orange. | ✅ | — |
| **file-picker** | Find relevant files in codebase. prefer_tier=routine, permission_mode=plan, max_turns=5. | codebuff (file-picker agent) | `.jcode/agents/file-picker.toml`. color=cyan. | ✅ | — |
---

### 13. Model Override (Built-in Agent Types)

*Hardcoded agent types for model routing.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Swarm override** | Model override for swarm subagents. | CCB (agent model config) | `AgentModelTarget::Swarm` via `model_prefs.json` | ✅ | — |
| **Review override** | Model override for review agent. | CCB | `AgentModelTarget::Review` | ✅ | — |
| **Judge override** | Model override for judge agent. | CCB | `AgentModelTarget::Judge` | ✅ | — |
| **Memory override** | Model override for memory agent. | CCB | `AgentModelTarget::Memory` | ✅ | — |
| **Ambient override** | Model override for ambient agent. | CCB | `AgentModelTarget::Ambient` | ✅ | — |

## II. Permission System

*Tool-level permission classification, mode management, dialog UI, and rule CRUD.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **6 permission modes** | Default/AcceptEdits/Plan/Auto/DontAsk/BypassPermissions. Mode cycling via Alt+P, Shift+Tab, `/permissions`. | CCB (PermissionMode union) | `permission.rs`: `PermissionMode` enum. `input.rs`: Alt+P, BackTab. `dcg_bridge.rs`: `cycle_mode()`, `set_mode()`. | ✅ | — |
| **Tool execution pause** | When permission needed, dialog shows + tool execution pauses via `await_permission_response()`. User approves → tool continues. Model never sees error. | CCB (interactiveHandler flow) | `turn_execution.rs`: `validate_tool_allowed` async. `dcg_bridge.rs`: `await_permission_response()`, `signal_permission_response()`. | ✅ | — |
| **Permission dialog** | Rounded border overlay. 4 options: Approve/Approve all/Always allow/Deny. ←→ navigate, Enter confirm, Esc reject. | CCB (PermissionDialog.tsx) | `ui_overlays.rs`: `draw_permission_dialog_overlay()`, `append_option_row()`. | ✅ | — |
| **Tool-specific dialogs** | bash shows full command `$ git push`, edit shows file diff `─ old / + new`, write shows file path + content preview. | CCB (BashPermissionRequest.tsx, FileEditPermissionRequest.tsx) | `ui_overlays.rs`: `build_bash_permission_lines()`, `build_edit_permission_lines()`, `build_write_permission_lines()`. | ✅ | — |
| **Worker badge** | Dialog title shows `[session: abc-12345]` when permission request comes from a different session (subagent). | CCB (WorkerBadge) | `ui_overlays.rs`: `title_suffix` with session_id. | ✅ | — |
| **Risk level / explainer** | LOW/MEDIUM/HIGH badge in dialog. Rule-based classification based on tool + input (e.g., `rm -rf` → HIGH). | CCB (permissionExplainer.ts) | `dcg_bridge.rs`: `RiskLevel` enum, `explain_tool_call()`. `ui_overlays.rs`: risk badge rendering. | ✅ | — |
| **Denial tracking** | Track consecutive + total denials per session. 3 consecutive → warning shown. Reset on approval. | CCB (denialTracking.ts: maxConsecutive=3, maxTotal=20) | `dcg_bridge.rs`: `DENIAL_COUNTS`, `record_denial()`, `record_approval()`, `denial_limit_exceeded()`. `input.rs`: call on approve/deny. | ✅ | — |
| **Permission timeout** | Track when dialog was shown (`pending_permission_at`). Auto-clear after timeout. | CCB (request timeout) | `app.rs`: `pending_permission_at`. `local.rs`: set on bus event. `conversation_state.rs`: reset. | ✅ | — |
| **Plan mode notice** | When entering Plan mode via Alt+P, status shows "Plan mode: writes are blocked". | CCB (EnterPlanMode dialog) | `input.rs`: Alt+P handler shows notice. | ✅ | — |
| **Mode transition safety** | Strip dangerous tools (bash, write, edit, subagent, etc.) from session allow-list when entering Auto mode. | CCB (permissionSetup.ts: stripDangerousPermissionsForAutoMode) | `dcg_bridge.rs`: `strip_dangerous_permissions_for_mode()`, `is_dangerous_allow_rule()`. `input.rs`: call on Auto enter. | ✅ | — |
| **Auto-allow list** | 39 READ_ONLY + 23 STATEFUL_SAFE tools auto-allowed in Default mode. Auto-allowed lists: `is_legacy_auto_allowed()`. | CCB (SAFE_YOLO_ALLOWLISTED_TOOLS) | `dcg_bridge.rs`: `READ_ONLY_ACTIONS`, `STATEFUL_SAFE_ACTIONS`. `safety.rs`: `AUTO_ALLOWED`. | ✅ | — |
| **Graceful tool failure** | When permission denied, tool reports error via ToolResult(is_error) + Bus::ToolUpdated(Error). Turn continues to next tool. | CCB (tool execution error) | `turn_loops.rs`, `turn_streaming_mpsc.rs`: `if let Err(e) = validate_tool_allowed().await { ... continue; }`. | ✅ | — |
| **`/permissions` command** | Show mode, list modes, cycle, set by name. Also: `rules` list, `allow <tool>`, `deny <tool>`, `revoke` all. | CCB (/permissions command) | `state_ui.rs`: `/permissions` handler with CRUD subcommands. | ✅ | — |
| **Session allow-list** | Per-session approved-tool cache. `approve_session_action()`, `approve_session_all()`, `session_allows_action()`. Always-allow persisted to config. | CCB (session rules, always-allow config) | `dcg_bridge.rs`: `SESSION_ALLOWED_ACTIONS`. `config.rs`: `always_allow_tools`. | ✅ | — |
| **Sandbox integration** | Auto-sandbox flagged dangerous commands (Docker/container). | CCB (sandbox integration) | — | ❌ | Requires container/sandbox infrastructure. Separate project. |


## III. Hooks System

*Lifecycle hooks for tool execution, session management, permission events, agent lifecycle, compaction, and setup.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **PreToolUse** | Blocking gate: runs before every tool call. Exit 0=allow, 2=block. Timeout configurable. | CCB (preToolUse), jcode HOOKS.md | `tool/mod.rs`: dispatch via `HookEvent::PreToolUse`. | ✅ | — |
| **PostToolUse** | Fire-and-forget observer after successful tool call. | CCB (postToolUse) | `tool/mod.rs`: dispatch via `HookEvent::PostToolUse`. | ✅ | — |
| **PostToolUseFailure** | Fire-and-forget observer after tool call failure. | CCB (postToolUseFailure) | `tool/mod.rs`: dispatch via `HookEvent::PostToolUseFailure`. | ✅ | — |
| **ToolError** | Fire-and-forget diagnostic on tool execution error. | CCB (ToolError) | `tool/mod.rs`: dispatch via `HookEvent::ToolError`. | ✅ | — |
| **UserPromptSubmit** | Blocking gate: can deny prompt before entering conversation. | CCB (userPromptSubmit) | `turn_execution.rs`: dispatch via `HookEvent::UserPromptSubmit`. | ✅ | — |
| **UserPromptExpansion** | Fire-and-forget diagnostic after prompt expansion. | CCB (UserPromptExpansion) | `turn_execution.rs`: dispatch via `HookEvent::UserPromptExpansion`. | ✅ | — |
| **SessionStart** | Fire-and-forget observer on session creation. | CCB (sessionStart) | `agent.rs`: dispatch via `HookEvent::SessionStart`. | ✅ | — |
| **SessionEnd** | Fire-and-forget observer on session close. | CCB (sessionEnd) | `agent.rs`: dispatch via `HookEvent::SessionEnd`. | ✅ | — |
| **SessionUpdated** | Fire-and-forget observer on session update. | CCB (SessionUpdated) | `agent.rs`: dispatch via `HookEvent::SessionUpdated`. | ✅ | — |
| **SessionDiff** | Fire-and-forget observer on file diff detection. | CCB (SessionDiff) | `turn_loops.rs`, `turn_streaming_mpsc.rs`: dispatch via `HookEvent::SessionDiff`. | ✅ | — |
| **SessionError** | Fire-and-forget observer on session error. | CCB (SessionError) | `client_lifecycle.rs`: dispatch via `HookEvent::SessionError`. | ✅ | — |
| **SessionIdle** | Fire-and-forget observer on session idle timeout. | CCB (SessionIdle) | `client_lifecycle.rs`: dispatch via `HookEvent::SessionIdle`. | ✅ | — |
| **PermissionRequest** | Blocking: runs when permission prompt is shown. | CCB (PermissionRequest) | `dcg_bridge.rs`: dispatch via `HookEvent::PermissionRequest`. | ✅ | — |
| **PermissionDenied** | Fire-and-forget observer on permission denial. | CCB (PermissionDenied) | `dcg_bridge.rs`: dispatch via `HookEvent::PermissionDenied`. | ✅ | — |
| **PermissionAsked** | Blocking: runs when pre-approval is requested. | CCB (PermissionAsked) | `dcg_bridge.rs`: dispatch via `HookEvent::PermissionAsked`. | ✅ | — |
| **PermissionReplied** | Fire-and-forget observer on permission reply. | CCB (PermissionReplied) | `dcg_bridge.rs`: dispatch via `HookEvent::PermissionReplied`. | ✅ | — |
| **AgentStart** | Fire-and-forget observer on agent start. | CCB (AgentStart) | `agent.rs`: dispatch via `HookEvent::AgentStart`. | ✅ | — |
| **AgentEnd** | Fire-and-forget observer on agent end. | CCB (AgentEnd) | `agent.rs`: dispatch via `HookEvent::AgentEnd`. | ✅ | — |
| **SubagentStart** | Fire-and-forget observer on subagent spawn. | CCB (SubagentStart) | `tool/task.rs`: dispatch via `HookEvent::SubagentStart`. | ✅ | — |
| **SubagentStop** | Fire-and-forget observer on subagent stop. | CCB (SubagentStop) | `tool/task.rs`: dispatch via `HookEvent::SubagentStop`. | ✅ | — |
| **TurnEnd** | Fire-and-forget observer on turn completion. Extra: duration, model, status, last text. | CCB (TurnEnd) | `turn_execution.rs`: dispatch via `HookEvent::TurnEnd`. | ✅ | — |
| **Stop** | Blocking: runs on session stop/shutdown. | CCB (Stop) | `client_lifecycle.rs`: dispatch via `HookEvent::Stop`. | ✅ | — |
| **PreCompact** | Blocking: runs before compaction starts. | CCB (PreCompact) | `compaction.rs`: dispatch via `HookEvent::PreCompact`. | ✅ | — |
| **PostCompact** | Fire-and-forget observer after compaction. | CCB (PostCompact) | `compaction.rs`: dispatch via `HookEvent::PostCompact`. | ✅ | — |
| **AutoCompactionControl** | Fire-and-forget observer for auto-compaction. | CCB (AutoCompactionControl) | `compaction.rs`: dispatch via `HookEvent::AutoCompactionControl`. | ✅ | — |
| **TaskCreated** | Fire-and-forget observer on task creation. | CCB (TaskCreated) | `tool/todo.rs`: dispatch via `HookEvent::TaskCreated`. | ✅ | — |
| **TaskCompleted** | Fire-and-forget observer on task completion. | CCB (TaskCompleted) | `tool/todo.rs`: dispatch via `HookEvent::TaskCompleted`. | ✅ | — |
| **Setup** | Fire-and-forget observer on agent creation (initial setup). | CCB (Setup) | `agent.rs`: dispatch via `HookEvent::Setup`. | ✅ | — |
| **Custom events** | User-defined hook events via TOML config. | CCB (Custom) | `config.rs`: `HookEvent::Custom(String)`. | ✅ | — |
| **Legacy v1 bridge** | `turn_end`→TurnEnd, `session_start/end`→SessionStart/End, `pre_tool`→PreToolUse, `post_tool`→PostToolUse+Failure. Config via `[hooks]` TOML. | jcode HOOKS.md | `config.rs`: `legacy_v1_to_v2_handlers()`. | ✅ | — |
| **Spawn hook** | Custom terminal spawn (`JCODE_SPAWN_HOOK`). Route headed sessions to tmux/kitty/zellij. | CCB (spawn hook) | `terminal_launch.rs`: spawn hook with `JCODE_SPAWN_*` env metadata. | ✅ | — |
| **Focus hook** | Custom window focus (`JCODE_FOCUS_HOOK`). Bring session window to front. | CCB (focus hook) | `terminal_launch.rs`: focus hook with `JCODE_FOCUS_*` env metadata. | ✅ | — |
| **Recursion guard** | `JCODE_HOOKS_DISABLED=1` suppresses hooks in nested jcode calls. | jcode HOOKS.md | `execute.rs`: recursion guard set in hook env. | ✅ | — |

## IV. Keyword System

*Natural language keyword triggers that activate persistent workflow modes, inject system prompts, and manage mode lifecycle across turns.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Keyword detection** | Scan user input for predefined keyword triggers (`$ultrawork`, `$ralplan`, etc.) with exact + fuzzy matching. Levenshtein distance ≤ 2 for aliases. | CCB (keyword detection) | `detector.rs`: `detect_keywords()`, `find_fuzzy()`, `levenshtein_distance()`. Sanitizer strips ANSI, normalizes whitespace. | ✅ | — |
| **Keyword registry** | 14 keywords + aliases, priority-sorted, deduplicated by workflow. Keywords: `$ultrawork`, `$ralplan`, `$ultragoal`, `$ultraqa`, `$deep-interview`, `$ultrathink`, `$deepsearch`, `$tdd`, `$code-review`, `$security-review`, `$analyze`, `$wiki`, `canceljcode`, `ai-slop-cleaner`. | CCB (keyword registry) | `registry.rs`: `KeywordEntry` struct, `build_registry()` with OnceLock. 14 WorkflowKind variants. | ✅ | — |
| **Mode state persistence** | Active modes persisted to `.jcode/state/modes.toml`. Turn counting, auto-expiry after 10 turns, cancel all. | CCB (mode state) | `state.rs`: `ModeState`, `ActiveMode`, `update_modes()`, `load_state()`, `save_state()`, `clear_modes()`. | ✅ | — |
| **Workflow execution** | Execute active workflows each turn. Get handler → build prompt → apply actions (deferred spawns for subagent). Heavy workflows suppressed for Simple tasks (< 50 chars). | CCB (workflow executor) | `workflow/executor.rs`: `process_turn()`, `execute_active_workflows()`, `apply_actions()`, `build_workflow_prompt()`. | ✅ | — |
| **System prompt injection** | Keyword prompt injected into system prompt's dynamic part. Both TUI and agent-runtime paths run `process_turn()` independently. | CCB (system prompt injection) | `turn_memory.rs` (TUI path), `prompting.rs` (agent-runtime path): both call `jcode_keywords::process_turn()`. | ✅ | — |
| **User feedback** | Status notice when keyword activates a mode. Shows "🧠 Ultrawork mode activated" in status bar. | CCB (mode feedback) | `turn_memory.rs`: post-`process_turn()` check → `self.set_status_notice()`. | ✅ | — |
| **Task size classification** | Simple (< 50 chars) / Medium (50-200 chars) / Heavy (> 200 chars). Heavy workflows suppressed for Simple tasks. | CCB (task size) | `task_size.rs`: `classify()`, `should_suppress()`. | ✅ | — |
| **Conflict detection** | Detect conflicting active modes (e.g., TDD + Ultrawork). Log warnings. | CCB (conflict detection) | `conflict.rs`: `check_conflicts()`, `format_warning()`. | ✅ | — |
| **14 workflow handlers** | Ultrawork, Ultragoal, Ultraqa, Ralplan, DeepInterview, TDD, CodeReview, SecurityReview, Ultrathink, Deepsearch, Analyze, Wiki, AiSlopCleaner, Cancel. Each has `build_prompt()`, `should_suppress()`, `phase_name()`. | CCB (workflow handlers) | `workflow/` directory: 14 handler modules. | ✅ | — |
| **Deferred spawns** | Subagent spawn actions queued for later execution when SubagentTool is available. | CCB (deferred spawns) | `workflow/executor.rs`: `DeferredSpawn`, queued in `execute_active_workflows()` for Ultrawork and Ralplan workflows. | ⚠️ | Wiring to actual SubagentTool dispatch is pending (issue #391). |

## V. Goal System

*Multi-repo goal management: set objectives, track progress, auto-continuation, and success criteria across turns.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Hierarchical goals** | Epics → Goals → Milestones → Steps → Beads. Full nesting with status per level. | CCB (flat), codex (goals+criteria), oh-my-openagent (team tasklist), jcode (beads_rust) | `jcode-beads-bridge`: `Goal`, `GoalMilestone`, `GoalStep`. `GoalCreateInput` with success_criteria. | ✅ | — |
| **`/goal` CLI command** | `/goal` — show active goals. `/goal <objective>` — set new. `/goal clear` — clear all. `/goal resume` — resume session goal. | CCB (`/goal` set/status/clear/pause/resume/continue/complete) | `commands.rs`: `handle_goal_or_mission_command()` with set/status/clear/resume. | ✅ | — |
| **Auto-continuation** | After each turn, if goal is active and not complete, auto-queue continuation message. `goal_continuation_disabled` flag. | CCB (auto-continuation) | `local.rs`: `finish_turn()` checks active goals → queues "Continue working toward goal". `app.goal_continuation_disabled`. | ✅ | — |
| **Success criteria** | Per-goal success criteria list. Checked for completion status. | codex (UlwLoopItem.successCriteria with pass/fail status per criterion) | `GoalCreateInput.success_criteria: Vec<String>`. Passed through beads_rust. | ✅ | — |
| **Side panel display** | Goals overview in side panel. Detail page per goal. Attach to session. | jcode (beads_rust side panel) | `open_goals_overview_for_session()`, `open_goal_for_session()`, `write_goal_page()`. | ✅ | — |
| **Dependencies** | Goal-blocking relationships via `blockers` + `beads_dep`. | oh-my-openagent (team tasklist dependencies) | `Goal.blockers: Vec<String>`, `beads_dep` tool for dependency graph. | ✅ | — |
| **Progress tracking** | Progress percentage per goal. Updated via `update_goal()`. | CCB (token budget, turns) | `Goal.progress_percent: Option<u8>`. Updated through beads lifecycle. | ✅ | — |
| **Goal lifecycle** | Status: active / done / cancelled / blocked. Create → update → complete. | CCB (set/clear/pause/resume/complete) | `GoalStatus` enum with full lifecycle. `create_goal()`, `update_goal()`, `load_goal()`. | ✅ | — |

## VI. Session System

*Session persistence, resume, cross-agent conversion, export, and compact.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **JSONL snapshot + journal** | Session stored as JSON snapshot + append-only journal. Incremental save, atomic writes, backup files. | pi-agent-rust (SQLite), jcode (JSONL) | `persistence.rs`: `load_from_path()`, snapshot + journal merge. `load()`, `save()` with backup rotation. | ✅ | — |
| **Session resume** | `jcode --resume <id>` to resume any session. Session picker with preview. | CCB (`claude --resume`), pi-agent-rust (`pi --session`) | `session_picker.rs`: full resume UI with preview. `workspace_client::queue_resume_session()`. | ✅ | — |
| **Cross-agent session resume** | Convert sessions between 12 providers (jcode, CC, aider, opencode, codex, cursor, cline, pi, gemini, vibe, openclaw, chatgpt). `casr convert` pipeline. | CASR (cross_agent_session_resumer) | CASR v0.1.4 with 12 providers. `ConversionPipeline::convert()` with detection→read→validate→write→verify. Atomic write with backup. | ✅ | — |
| **Session graph / memory topology** | Build graph topology from memory entries. Compute graph node scores for relevance ranking. | jcode (info_widget_graph) | `info_widget_graph.rs`: `build_graph_topology()`, `graph_node_score()`, `GraphEdge`, `GraphNode`. | ✅ | — |
| **`/session` command** | View/manage current session. session info, history, resume. | CCB (`/session`) | `/session` command with session details. | ✅ | — |
| **`/compact` command** | Compact session to reduce context window pressure. Micro-compact options. | CCB (`/compact`) | `/compact` command with mode selection. PreCompact/PostCompact hooks. | ✅ | — |
| **`/export` command** | Export current conversation to `.txt` file. Format: Markdown with role headers. | CCB (`/export <filename>`) | `commands.rs`: `handle_export_command()` → writes to filename, shows message count + KB. | ✅ | — |
| **`/transfer` command** | Transfer session to another jcode instance (remote). | CCB (session transfer) | `/transfer` command. | ✅ | — |
| **Teammate view** | View subagent's stream inline without switching sessions. Panel with live status + output_tail + session load. | CCB (teammateView) | `viewing_teammate_session_id` field. Teammate view panel + output_tail + session file loading via snapshot. | ✅ | — |
| **Session allow-list** | Per-session approved-tool cache for permission mode. `approve_session_action()`, `session_allows_action()`. | CCB (session permissions) | `dcg_bridge.rs`: `SESSION_ALLOWED_ACTIONS`. `approve_session_action()`, `clear_session_allowed_action()`. | ✅ | — |
| **Session idle / error** | Session idle timeout handling. Session error reporting. | CCB (SessionIdle, SessionError) | `client_lifecycle.rs`: SessionIdle + SessionError hook dispatches. | ✅ | — |

| Section | Features | ✅ Complete | ⚠️ Partial | ❌ Missing |
|---------|----------|-------------|-------------|-----------|
| I-1 — Running Items | 5 | 5 | 0 | 0 |
| I-2 — Detail Overlay | 5 | 5 | 0 | 0 |
| I-3 — Session Attachment | 4 | 4 | 0 | 0 |
| I-4 — Agent Definitions | 6 | 5 | 1 | 0 |
| I-5 — Agent Lifecycle | 6 | 6 | 0 | 0 |
| I-6 — Tool & Permission | 5 | 5 | 0 | 0 |
| I-7 — Agent Colors | 3 | 3 | 0 | 0 |
| I-8 — `/agents` Command | 7 | 7 | 0 | 0 |
| I-9 — Agent Creation | 5 | 4 | 1 | 0 |
| I-10 — `/tasks` Command | 3 | 3 | 0 | 0 |
| I-11 — Teams & Swarm | 4 | 3 | 1 | 0 |
| I-12 — Built-in Agents | 5 | 5 | 0 | 0 |
| I-13 — Model Override | 5 | 5 | 0 | 0 |
| II — Permission System | 15 | 14 | 0 | 1 |
| III — Hooks System | 33 | 33 | 0 | 0 |
| IV — Keyword System | 10 | 9 | 1 | 0 |
| V — Goal System | 8 | 8 | 0 | 0 |
| VI — Session System | 11 | 11 | 0 | 0 |
| VII — Benchmarking | 18 | 13 | 5 | 0 |
| VIII — Tools | 71 | 71 | 0 | 0 |
| IX — Provider System | 18 | 18 | 0 | 0 |
| X — Plugin System | 18 | 18 | 0 | 0 |
| XI — Desktop App | 15 | 15 | 0 | 0 |
| XII — Embedding & Memory | 7 | 7 | 0 | 0 |
| XIII — Auth & Secrets | 5 | 5 | 0 | 0 |
| XIV — Reference Gaps | 18 | 0 | 6 | 12 |
| **Total** | **310** | **283 (91%)** | **14 (5%)** | **13 (4%)** |

### Missing / Partial Features

| Priority | Feature | Section | Effort | Reference | jcode Impl |
|----------|---------|---------|--------|-----------|------------|
| — | Agent scopes (managed) | I-4 | Low | CCB: 4 scopes | ✅ `SourceKind::Managed` added. Managed dir: `~/.jcode/managed-agents/` |
| — | Agent teams interactive | I-11 | Low | CCB: teammate view | ⚠️ `/agents` Running tab + running items list provide navigation. TeamViewInteraction struct added. |
| — | Deferred spawns | IV | Low | CCB: subagent spawn | ⚠️ DeferredSpawn queued, keyword prompt injected. Model spawns via subagent tool. |
| — | Sandbox integration | II | High | CCB: sandbox | ❌ Skipped per request |

## VII. Benchmarking

*Edit quality benchmarks, eval framework, and performance measurement scripts.*

> **⚠️ PRELIMINARY:** This section was added during a brief codebase scan. Features listed here
> are **not fully verified**. Some may require external dependencies (API keys, jcode binary in
> PATH, ONNX models, rustfmt, etc.) to actually run end-to-end. Treat status indicators as
> tentative until each feature is independently validated.

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Edit benchmark** | Mutation-based edit benchmark harness. Generates tasks via tree-sitter AST mutations (25 mutation types), runs agents in parallel (best-of-N), verifies with rustfmt normalization. | oh-my-pi (typescript-edit-benchmark) | `evals/jcode-edit-bench/`: `generate.rs`, `runner.rs`, `verify.rs`, `mutation.rs`, `difficulty.rs`, `report.rs`, `formatter.rs`, `fixtures.rs` | ✅ | — |
| **Difficulty scoring** | Scores each mutation (0-20) based on file length, code density, nesting depth, repeated lines, function count. | oh-my-pi (scoreDifficulty) | `difficulty.rs`: `score_difficulty()`, `analyze_file()`, `file_matches_difficulty()`, `min_score_for_difficulty()` | ✅ | — |
| **Edit benchmark CLI** | 4 subcommands: `generate` (create fixtures), `run` (execute benchmark), `list` (list tasks), `check` (validate fixtures). | oh-my-pi (CLI) | `bin/jcode-edit-bench.rs`: CLI with `GenerateConfig`, `BenchmarkConfig`. | ✅ | — |
| **Parallel agent runner** | Semaphore-limited concurrent agent subprocesses via `jcode agent run`. Timeout + retry per attempt. | oh-my-pi (runner.ts) | `runner.rs`: `run_benchmark()`, `run_single_attempt()` with tokio semaphore (max 8 concurrent). | ✅ | — |
| **Report generation** | JSON + Markdown report output. Task-level summarization, best-of-N selection, pass rates, token/tool-call stats. | oh-my-pi (report.ts) | `report.rs`: `generate_json_report()`, `generate_markdown_report()`, `pick_best_run_index()`, `summarize_task()`. | ✅ | — |
| **Fixture management** | Load tasks from fixture directories (input/expected/prompt/metadata). Validate fixture integrity. | oh-my-pi (fixtures) | `fixtures.rs`: `load_tasks_from_dir()`, `validate_fixtures()`, `list_files()`, `save_task()`. | ✅ | — |
| **JBench eval framework** | Git-commit-reconstruction eval framework. Reconstruct commits from parent, compare agent diff vs ground truth. | codebuff (BuffBench) | `evals/jbench/`: `types.rs`, `agent_runner.rs`, `judge.rs`, `lessons.rs`. CLI via `bin/jbench.rs`. | ⚠️ | Library crate says `unimplemented!()` stubs. Real API calls (reqwest) exist in judge/lessons. Needs end-to-end validation. |
| **Agent runner** | Spawn jcode agent in prepared repo clone, capture diff + trace. Resolves agent from AgentRegistry. | codebuff (agent-runner.ts) | `agent_runner.rs`: `run_agent_in_repo()`, `extract_diff_from_repo()`. | ⚠️ | Code spawns `jcode` subprocess. Needs jcode binary in PATH + registered agent. Not tested. |
| **Three-judge pipeline** | Grade agent diffs with 3 frontier models in parallel (gpt-5, gemini-pro, claude-sonnet). Median overall_score. | codebuff (judge.ts) | `judge.rs`: `JudgeProviderKind` (OpenAI, Anthropic), `judge_commit_result()`, `median_score()`. | ⚠️ | Code exists with real reqwest calls. Requires API keys (`JBENCH_API_KEY`). Not tested end-to-end. |
| **Lessons extractor** | Compare agent diff vs ground truth → distilled lessons for system prompt improvement. | codebuff (lessons-extractor.ts) | `lessons.rs`: `Lesson` struct, `RunLessonsConfig`, `extract_lessons()`. | ⚠️ | Code exists with API calls. Requires API keys. Not tested. |
| **TUI rendering benchmark** | Measure TUI frame rendering performance with synthetic session data. | jcode | `src/bin/tui_bench.rs`: ratatui TestBackend, configurable message count. | ✅ | — |
| **Memory recall benchmark** | Offline memory retrieval accuracy harness. Uses real MemoryGraph, all-MiniLM-L6-v2 ONNX embedding. | jcode | `src/bin/memory_recall_bench.rs`: `score_and_filter` with cosine + gap filter. Data outside repo. | ⚠️ | Binary exists. Requires ONNX model + external data. Needs end-to-end test. |
| **Startup time benchmark** | Measure cold client startup time in isolated JCODE_HOME/JCODE_RUNTIME_DIR. | jcode | `scripts/bench_startup.py`: PTY-based startup profiling with regression check. | ✅ | — |
| **Tool call benchmark** | Measure execution time for each tool with representative inputs. | jcode | `scripts/benchmark_tools.sh`: CSV results, configurable iterations. | ✅ | — |
| **Swarm benchmark** | Compare single agent vs swarm on Anthropic Performance Take-Home (VLIW SIMD kernel). | jcode | `scripts/benchmark_swarm.py`, `scripts/benchmark_takehome.py`: timed trials, configurable timeout. | ✅ | — |
| **Compile benchmark** | Measure cargo check/build/release compilation times. | jcode | `scripts/bench_compile.sh`: targets for check, build, release-jcode. | ✅ | — |
| **Self-dev checkpoint bench** | Benchmark self-development checkpoint operations. | jcode | `scripts/bench_selfdev_checkpoints.sh`: timing for dev loop steps. | ✅ | — |
| **Terminal bench campaign** | Run terminal-based benchmark campaigns with harbor deployment. | jcode | `scripts/run_terminal_bench_campaign.py`, `scripts/run_terminal_bench_harbor.sh`: parallel campaign orchestration. | ✅ | — |

---

---

## VIII. Tools

*All 64 registered agent tools, organized by category. Tools are the primary interface between the agent and the system.*

> **Note:** Some tools expose multiple sub-commands (e.g., `beads_list`, `beads_create`, etc.).
> Each sub-command is counted separately in the registry.

### 1. File Operations

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **read** | Read file contents from workspace. Supports line ranges, syntax highlighting. | CCB (read), codebuff (read) | `tool/read.rs`: `ReadTool` | ✅ |
| **write** | Write content to a new or existing file. | CCB (write), codex (write) | `tool/write.rs`: `WriteTool` | ✅ |
| **edit** | Find-and-replace text edits on existing files. | CCB (edit), oh-my-pi (edit) | `tool/edit.rs`: `EditTool` | ✅ |
| **multiedit** | Apply multiple edits in a single operation. | oh-my-pi (multiedit) | `tool/multiedit.rs`: `MultiEditTool` | ✅ |
| **patch** | Apply unified diff patches to files. | CCB (patch) | `tool/patch.rs`: `PatchTool` | ✅ |
| **apply_patch** | Apply a patch file created by diff. | CCB (apply_patch) | `tool/apply_patch.rs`: `ApplyPatchTool` | ✅ |
| **ffs_hashline_edit** | Hash-anchored editing via FFS engine (struct: HashlineEditTool). | oh-my-pi (hashline_edit) | `tool/hashline_edit.rs`: `HashlineEditTool`, registered as `ffs_hashline_edit` | ✅ |
| **ffs_propose_hashline** | Hashline edit via the FFS engine. | oh-my-pi (ffs_hashline) | `tool/ffs_engine_tools.rs`: FfsHashlineEditTool | ✅ |
| **propose_edit** | Propose an edit for user approval before applying. | codebuff (propose) | `tool/propose_edit.rs`: `ProposeEditTool` | ✅ |
| **propose_write** | Propose a file write for user approval. | codebuff (propose) | `tool/propose_write.rs`: `ProposeWriteTool` | ✅ |

### 2. Search & Code Understanding

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **codesearch** | Semantic code search across workspace. | oh-my-openagent (ripgrep-cli) | `tool/codesearch.rs`: `CodeSearchTool` | ✅ |
| **agentgrep** | AI-powered grep with context-aware pattern matching. | — | `tool/agentgrep.rs`: `AgentGrepTool` | ✅ |
| **ffs_glob** | Fast file system glob — list files matching a pattern. | codebuff (code-map) | `tool/ffs_glob.rs`: `FfsGlobTool` | ✅ |
| **ffs_grep** | Fast file system grep — search file contents with regex. | CCB (grep) | `tool/ffs_grep.rs`: `FfsGrepTool` | ✅ |
| **ffs_multi_grep** | Run multiple grep queries in a single operation. | — | `tool/ffs_multi_grep.rs`: `FfsMultiGrepTool` | ✅ |
| **ffs_outline** | Show structural outline of source files (functions, types, etc.). | codebuff (code-map) | `tool/ffs_outline.rs`: `FfsOutlineTool` | ✅ |
| **ffs_symbol** | Find symbol definitions and references. | oh-my-pi (LSP) | `tool/ffs_symbol.rs`: `FfsSymbolTool` | ✅ |
| **ffs_callers** | Find all callers of a function/method. | oh-my-pi (LSP) | `tool/ffs_engine_tools.rs`: `FfsCallersTool` | ✅ |
| **ffs_callees** | Find all callees (functions called within a function). | oh-my-pi (LSP) | `tool/ffs_engine_tools.rs`: `FfsCalleesTool` | ✅ |
| **ffs_refs** | Find all references to a symbol across the codebase. | oh-my-pi (LSP) | `tool/ffs_engine_tools.rs`: `FfsRefsTool` | ✅ |
| **ffs_find** | Find files by name, path, or pattern. | CCB (find) | `tool/ffs_engine_tools.rs`: `FfsFindTool` | ✅ |
| **ffs_dispatch** | Dispatch FFS queries to the optimal engine. | — | `tool/ffs_engine_tools.rs`: `FfsDispatchTool` | ✅ |
| **ffs_flow** | Data-flow and control-flow analysis. | codebuff (code-map) | `tool/ffs_engine_tools.rs`: `FfsFlowTool` | ✅ |
| **lsp** | Language Server Protocol operations (goToDefinition, findReferences, hover, etc.). | oh-my-pi (13 LSP ops) | `tool/lsp.rs`: `LspTool` (9 operations) | ✅ |

### 3. Shell & Execution

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **bash** | Execute shell commands with interactive terminal support. | CCB (bash), codex (shell) | `tool/bash.rs`: `BashTool` | ✅ |
| **bg** | Manage background tasks: list, wait, inspect output. | CCB (background tasks) | `tool/bg.rs`: `BgTool` | ✅ |
| **browser** | Web browser automation — navigate, click, extract content. | CCB (Chrome Use) | `tool/browser.rs`: `BrowserTool` | ✅ |
| **macos_computer_use** | macOS screen capture and UI automation (Vision-based). | CCB (Computer Use) | `tool/computer.rs`: `ComputerTool` (macOS only) | ✅ |

### 4. Web & Communication

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **websearch** | Search the web via configured search engine. | CCB (Web Search) | `tool/websearch.rs`: `WebSearchTool` | ✅ |
| **webfetch** | Fetch content from URLs with proper headers and parsing. | CCB (webfetch) | `tool/webfetch.rs`: `WebFetchTool` | ✅ |
| **gmail** | Gmail integration — read, search, send emails. | — | `tool/gmail.rs`: `GmailTool` | ✅ |

### 5. Agent Coordination

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **swarm** | Multi-agent swarm coordination via shared plan + status. | CCB (SwarmPlan), codebuff (4-agent) | `tool/swarm.rs` | ✅ |
| **team_create** | Create a new team with members and roles. | oh-my-openagent (Atlas) | `tool/team.rs`: `TeamCreateTool` | ✅ |
| **team_status** | View team status: members, tasks, dependencies. | oh-my-openagent | `tool/team.rs`: `TeamStatusTool` | ✅ |
| **team_task_create** | Create a task in a team task board. | oh-my-openagent | `tool/team.rs`: `TeamTaskCreateTool` | ✅ |
| **team_task_list** | List tasks for a team with filter options. | oh-my-openagent | `tool/team.rs`: `TeamTaskListTool` | ✅ |
| **team_task_claim** | Claim a task as the current agent. | oh-my-openagent | `tool/team.rs`: `TeamTaskClaimTool` | ✅ |
| **team_send_message** | Send a message to a team member's mailbox. | CCB (teammateMailbox) | `tool/team.rs`: `TeamSendMessageTool` | ✅ |
| **team_delete** | Delete an entire team configuration. | — | `tool/team.rs`: `TeamDeleteTool` | ✅ |
| **team_shutdown** | Gracefully shut down a team run. | — | `tool/team.rs`: `TeamShutdownTool` | ✅ |

### 6. Issue Tracking (Beads)

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **beads_list** | List issues with filtering and status display. | jcode (beads-rs) | `tool/beads.rs`: `BeadsListTool` | ✅ |
| **beads_create** | Create a new issue with title, body, and metadata. | jcode (beads-rs) | `tool/beads.rs`: `BeadsCreateTool` | ✅ |
| **beads_ready** | Mark issue as ready for work. | jcode (beads-rs) | `tool/beads.rs`: `BeadsReadyTool` | ✅ |
| **beads_claim** | Claim an issue for the current agent. | jcode (beads-rs) | `tool/beads.rs`: `BeadsClaimTool` | ✅ |
| **beads_close** | Close an issue with resolution status. | jcode (beads-rs) | `tool/beads.rs`: `BeadsCloseTool` | ✅ |
| **beads_dep** | Manage issue dependencies (blocked-by, blocks). | jcode (beads-rs) | `tool/beads.rs`: `BeadsDepTool` | ✅ |

### 7. Memory & Knowledge

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **memory** | Store, recall, and search across-session memories. | CCB (claude.md), pi-agent-rust | `tool/memory.rs`: `MemoryTool` | ✅ |
| **session_search** | RAG search across all past session transcripts. | CCB (session search) | `tool/session_search.rs`: `SessionSearchTool` | ✅ |
| **notepad_read_priority** | Read priority (auto-injected) notes. | jcode (notepad) | `tool/notepad.rs` | ✅ |
| **notepad_write_priority** | Write priority notes (requires confirmation). | jcode (notepad) | `tool/notepad.rs` | ✅ |
| **notepad_read_working** | Read working (scratchpad) notes. | jcode (notepad) | `tool/notepad.rs` | ✅ |
| **notepad_write_working** | Write working notes. | jcode (notepad) | `tool/notepad.rs` | ✅ |
| **notepad_read_manual** | Read user-authored notes. | jcode (notepad) | `tool/notepad.rs` | ✅ |
| **notepad_write_manual** | Write manual notes. | jcode (notepad) | `tool/notepad.rs` | ✅ |
| **notepad_stats** | Show per-tier notepad sizes. | jcode (notepad) | `tool/notepad.rs` | ✅ |
| **notepad_prune** | Clear working notes only. | jcode (notepad) | `tool/notepad.rs` | ✅ |

### 8. Goal & Initiative

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **initiative** | Set, track, and complete goals with milestones. | CCB (goal/goals) | `tool/goal.rs`: `InitiativeTool` | ✅ |
| **todo** | Task management for the current session (todo add/list/done). | CCB (todo) | `tool/todo.rs`: `TodoTool` | ✅ |
| **schedule** | Schedule actions for future execution. | CCB (schedule) | `tool/schedule.rs` | ✅ |
| **selfdev** | Self-development: reflect on usage, suggest improvements. | — | `tool/selfdev.rs` | ✅ |

### 9. Utility

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **ls** | List directory contents with file metadata. | CCB (ls) | `tool/ls.rs`: `LsTool` | ✅ |
| **open** | Open a file or URL in the default system handler. | CCB (open) | `tool/open.rs`: `OpenTool` | ✅ |
| **side_panel** | Display rich content in the TUI side panel. | CCB (side panel) | `tool/side_panel.rs`: `SidePanelTool` | ✅ |
| **invalid** | Graceful handling of invalid/unknown tool calls. | CCB | `tool/invalid.rs`: `InvalidTool` | ✅ |
| **dcp_compress** | Compress/decompress/recompress context data via DCP. | — | `tool/dcp_compress.rs`: `DcpCompressTool` | ✅ |

### 10. Project Templates

*(Included for completeness — these are not agent tools but project scaffold templates.)*

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **Template: basic** | Minimal `main.rs` + `Cargo.toml` Rust project. | — | Embedded template | ✅ |
| **Template: agent** | Agent project with jcode agent-runtime dependency. | — | Embedded template | ✅ |
| **Template: tool** | Custom tool scaffold with Tool trait impl. | — | Embedded template | ✅ |
| **Template: provider** | Custom provider scaffold with Provider trait impl. | — | Embedded template | ✅ |
| **Template: desktop-app** | Desktop app scaffold with jcode-desktop. | — | Embedded template | ✅ |
| **Template: plugin** | Plugin scaffold with manifest, capabilities, security config. | — | Embedded template | ✅ |
## IX. Provider System

*Multi-provider abstraction layer with auth, failover, pricing, model selection, and 10 provider backends.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Provider trait** | 40+ methods: complete, complete_split, set_model, model_routes, prefetch_models, refresh_model_catalog, reasoning_effort, service_tier, transport, native_compaction. | oh-my-pi (Provider abstraction), opencode (Provider interface) | `jcode-provider-core/src/lib.rs`: `Provider` trait with full API. | ✅ | — |
| **MultiProvider** | 10 provider slots (Claude, Anthropic, OpenAI, Copilot, Antigravity, Gemini, Cursor, Bedrock, OpenRouter) + N named OpenAI-compatible profiles. All hot-swappable. | oh-my-pi (40+ providers), opencode (provider abstraction) | `jcode-base/src/provider/mod.rs`: `MultiProvider` struct, 10 slots + N named profiles. | ✅ | — |
| **NamedProviderConfig** | OpenAI-compatible endpoint registration with per-model context_window, extra_body, auth type, reasoning_effort flag. | oh-my-pi (OpenAI-compatible) | `config-types/lib.rs:369`: `NamedProviderConfig` with full field set. | ✅ | — |
| **ProviderConfig** | default_model, default_provider, reasoning_effort, service_tier, native_compaction, cross_provider_failover, preserve_reasoning_context, stream_idle_timeout, error_classification. | oh-my-pi (config.yml), opencode (config.json) | `config-types/lib.rs:1016`: `ProviderConfig` with all fields. | ✅ | — |
| **Config persistence** | default_model + default_provider saved to TOML config. Loaded on startup, restored to session.model for new sessions. | oh-my-pi (role-based config.yml), opencode (model.json) | `config_file.rs`: `set_default_model()`, `set_default_provider()`. `tui_lifecycle.rs`: restored from config on new session. | ✅ | — |
| **Session model persistence** | session.model saved in session.json. Restored on resume. Also restored from config.default_model on new session. | oh-my-pi (session config) | `finalize_model_switch()` saves to session. `tui_lifecycle.rs:255-258` restores from config. | ✅ | — |
| **model_prefs.json** | Recent model selection (5 entries) + per-model variant preferences. Versioned JSON file. | opencode (model.json), oh-my-pi (model cache) | `inline_interactive.rs:190-238`: `load_model_prefs()`, `save_model_prefs()`, `model_prefs_push_recent()`. | ✅ | — |
| **Model catalog service** | TTL-cached model lists per scope, in-flight refresh tracking, disk-cache hydration for offline startup. | oh-my-pi (provider catalog) | `model_catalog.rs`: `ModelCatalogService` with caching. | ✅ | — |
| **Cross-provider failover** | Configurable auto/countdown/manual failover. Retryable error detection (429/5xx/quota). Exponential backoff (2s→4s→6s). Account-level failover before cross-provider. | CCB (failover), oh-my-pi (provider routing) | `model_context.rs:54`: `apply_provider_switch_for_failover()`. `config-types`: `CrossProviderFailoverMode`. | ✅ | — |
| **Model routing** | Thinking/routine split via env vars (JCODE_ROUTING_THINKING/ROUTINE). Cheap+premium tier routing. | oh-my-pi (model-routing), oh-my-openagent (resolveModel) | `src/model_routing.rs`: routing with env-var MVP. | ✅ | — |
| **Model tooltip** | Show cost, context window, latency when selecting a model in picker. | oh-my-pi (model metadata) | `ui_inline_interactive.rs:175-192`: 📊 stats rendering. | ✅ | — |
| **Free/Latest tags** | `[Free]` `[Latest]` tags displayed next to model entries in picker. | opencode (tags), oh-my-pi (model tags) | `ui_inline_interactive.rs:97-112`: `is_free` → `[Free]`, `is_latest` → `[Latest]`. | ✅ | — |
| **Variant sub-dialog** | Separate picker entries per reasoning effort (low/medium/high). Effort selection persisted to model_prefs.json variant field. | opencode (effort picker), oh-my-pi (variant selector) | `inline_interactive.rs:1164-1203`: entries per effort. `ModelPreferences.variant` persistence. | ✅ | — |
| **Cyclic model switching** | Tab/Shift+Tab cycle through recent models. Ctrl+Tab next, Ctrl+Shift+Tab previous. | CCB (model switch), opencode (model cycling) | `cycle_model()` in `input.rs:1615`. `model_switch_keys` from config. | ✅ | — |
| **Auth route system** | AuthRoute canonicalization, ResolvedCredential, active_auth_method_label, session_provider_key. Full OAuth-vs-API-key dual auth. | oh-my-pi (auth), opencode (provider auth) | `auth.rs`: full auth route system. | ✅ | — |
| **Reasoning effort** | Anthropic output_config + OpenAI Responses API reasoning_effort. Per-session setting. | opencode (effort), oh-my-pi (reasoning) | `model_context.rs`: reasoning_effort get/set. `config-types`: reasoning fields. | ✅ | — |
| **Service tier** | OpenAI priority/flex service tier selection. | oh-my-pi (tier) | `config-types`: service_tier field. Provider passes through. | ✅ | — |
| **Native compaction** | OpenAI auto/explicit/off native compaction at configurable threshold. | oh-my-pi (compaction) | `config-types`: native_compaction field. | ✅ | — |
| **Stream idle timeout** | Configurable timeout for idle streaming connections. | oh-my-pi (timeout) | `config-types`: stream_idle_timeout. | ✅ | — |
| **Error classification** | Classify provider errors as retryable vs non-retryable. Retryable: 429/5xx/overloaded/quota. Non-retryable: 400/401/403/invalid. | oh-my-pi (error handling) | `config-types/lib.rs:978`: retryable error classification. | ✅ | — |
| **30+ built-in profiles** | Pre-configured provider profiles for Anthropic, OpenAI, Gemini, Bedrock, etc. with newest-model auto-select. | oh-my-pi (40+ providers) | `provider_catalog.rs`: 30+ built-in profiles with quality tier ranking. | ✅ | — |
| **Status bar display** | Status bar shows: mode icon, model name, provider name, context usage bar (tokens). Custom shell command (Layer 3). | CCB (status line) | `ui_input.rs:540-592`: mode icon + model + provider + context bar + Layer 3 command. | ✅ | — |
## X. Plugin System

*Full plugin runtime with manifest, security capabilities, dispatcher, TUI host, native plugins, transpiler, loader, and server.*

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **Plugin manifest** | TOML/JSON manifest with name, version, entry point, capabilities, features, settings, engines, tags. | opencode (extension system), oh-my-openagent (plugin) | `manifest.rs`: `PluginManifest`, `PluginKind`, `PluginEntry`, `PluginCapabilities`, `SettingSchema` | ✅ |
| **Security capabilities** | Capability chain with deny-list, allow-list, global defaults, access mode. `Deny` by default. | pi-agent-rust (WASM capability gates) | `security.rs`: `CapabilityChain`, `AccessDecision`, `AccessMode`, `CapabilityAction` | ✅ |
| **Plugin events** | Lifecycle events: install, uninstall, enable, disable. Handler actions: allow, deny, prompt. | CCB (hooks), oh-my-claudecode | `events.rs`: `PluginEvent`, `HandlerAction`, `HandlerResult`, `PermissionDecision` | ✅ |
| **Config & discovery** | Plugin discovery paths, package name validation, plugin source resolution. | — | `config.rs`: `DiscoveryPaths`, `PluginConfig`, `PluginSource` | ✅ |
| **Runtime sandbox** | JavaScript plugin sandbox via rquickjs (QuickJS). Dual-timeout for info vs actionable calls. | pi-agent-rust (WASM sandbox) | `sandbox.rs`: `SandboxContext`, `DualTimeout`, isolated JS runtime | ✅ |
| **Plugin loader** | Load plugins from disk, validate manifest, instantiate runtime. | opencode (loader) | `loader.rs`: async plugin loading with validation | ✅ |
| **Plugin registry** | Global plugin registry: install, list, get, uninstall. Version tracking. | — | `registry.rs`: `PluginRegistry` | ✅ |
| **Plugin dispatcher** | Dispatch events/tools/capabilities to the correct plugin handler. | oh-my-openagent (delegate) | `dispatcher.rs`: event routing to plugin handlers | ✅ |
| **Native plugins** | Rust-native plugin support (compiled alongside jcode). | — | `native.rs`: native plugin ABI | ✅ |
| **Plugin transpiler** | Transpile plugin source to compatible JS/TS for the sandbox. | — | `transpiler.rs` | ✅ |
| **TUI plugin host** | Render plugin-provided UI in the terminal. Plugin panels, status widgets. | opencode (TUI extensions) | `tui_api.rs`, `tui_system.rs`: TUI integration for plugins | ✅ |
| **Plugin server** | Serve plugin capabilities to remote clients over HTTP/WS. | — | `server.rs`: plugin server endpoint | ✅ |
| **Timer/scheduler** | Time-based plugin execution scheduling. | — | `timer.rs`: scheduled plugin tasks | ✅ |
| **Plugin audit** | Audit trail for plugin operations and security decisions. | — | `audit.rs`: capability check logging | ✅ |
| **Error handling** | Structured plugin error types with context. | — | `errors.rs`: `PluginError` with typed variants | ✅ |
| **Bridge API** | Bridge between plugin runtime and jcode core. Tool registration, event hooks. | oh-my-claudecode (hooks) | `bridge.rs`: plugin ↔ jcode bridge | ✅ |
| **Preflight checks** | Validate plugin before installation: manifest, permissions, dependencies. | — | `preflight.rs`: pre-install validation | ✅ |
| **Serde helpers** | Serialization helpers for plugin data interchange. | — | `serde.rs`: plugin-specific serialization | ✅ |

## XI. Desktop App

*Native desktop application with rich text rendering, IPC, animations, gallery, issue browser, and workspace management.*

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **Desktop protocol** | Host-worker message protocol with compatibility negotiation. | opencode (desktop) | `desktop_protocol.rs`: `DesktopHostToWorkerMessage`, `DesktopWorkerToHostMessage`, compatibility versioning | ✅ |
| **Desktop IPC** | Stdio-based IPC between host and worker processes. JSON envelope framing. | opencode (desktop IPC) | `desktop_ipc.rs`: `DesktopIpcFrame`, `send_frame()`, `recv_frame()` | ✅ |
| **Rich text rendering** | ANSI-colored rich text with background colors, 256-color, truecolor support for desktop. | — | `desktop_rich_text.rs`: ANSI color parsing, rendering pipeline | ✅ |
| **Desktop scene** | Scene graph for compositing desktop UI elements (panels, overlays, animations). | — | `desktop_scene.rs`: scene graph with parent-child transforms | ✅ |
| **Single session view** | Full single-session desktop view with message history, input, tool display. | opencode (desktop session) | `single_session.rs`, `single_session_render.rs`: session rendering | ✅ |
| **Workspace** | Multi-session workspace management with session list, navigation, notifications. | opencode (desktop workspace) | `workspace.rs`: workspace layout, session switching | ✅ |
| **Desktop gallery** | Image/media gallery view for rendered outputs. | — | `desktop_gallery.rs`: inline media viewer | ✅ |
| **Issue browser** | Browse and manage GitHub issues from within the desktop app. | — | `desktop_issue_browser.rs`, `desktop_issue_cache.rs`: issue listing, caching | ✅ |
| **Desktop config** | Desktop-specific configuration: fonts, colors, layout, behavior. | — | `desktop_config.rs`, `desktop_prefs.rs`: user preferences | ✅ |
| **Desktop animations** | Smooth transitions and animated effects for desktop UI. | — | `animation.rs`: easing curves, frame-based animation | ✅ |
| **Session events** | Real-time session event processing for desktop UI updates. | — | `desktop_session_events.rs`: event → UI update pipeline | ✅ |
| **Worker host** | WebView worker host for rendering HTML content in desktop. | — | `desktop_worker_host.rs`: embedded WebView worker | ✅ |
| **Power inhibit** | Prevent system sleep during agent activity. | — | `power_inhibit.rs`: sleep inhibition (macOS via IOKit) | ✅ |
| **Session data** | Session data model for desktop with display roles, tool calls, content blocks. | — | `session_data.rs`: desktop session data structures | ✅ |
| **Desktop UI engine** | Core desktop UI engine: event loop, rendering, layout. | opencode (desktop engine) | `desktop_ui_engine.rs`: main loop, compositor, input handling | ✅ |

## XII. Embedding & Memory Pipeline

*ONNX-based embedding model for semantic search, memory palace adapter, and cross-session memory types.*

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **ONNX embedding model** | all-MiniLM-L6-v2 sentence embedding via tract + tokenizers. | pi-agent-rust (embeddings) | `embedding/lib.rs`: `RunnableEmbeddingModel`, `TopKItem`, batch inference | ✅ |
| **Tokenization** | HuggingFace tokenizers for embedding model input. | — | `tokenizers::Tokenizer`: sub-word tokenization | ✅ |
| **Memory palace adapter** | Adapt memory entries for the MemPalace spaced-repetition system. | — | `mempalace-adapter/`: memory → palace entry conversion | ✅ |
| **Memory types** | Memory entry types: facts, preferences, corrections, tags, clusters. | CCB (memory types) | `memory-types/`: `MemoryEntry`, `MemoryGraph`, scoring | ✅ |
| **Memory graph** | Graph-based memory topology with edges (has_tag, supersedes, contradicts). | CCB (memory graph) | `graph_topology.rs`: `GraphNode`, `GraphEdge`, `build_graph_topology()`, `graph_node_score()` | ✅ |
| **Session search** | Cross-session RAG search across all past session transcripts. | CCB (session search) | `tool/session_search.rs`: `SessionSearchTool` with index warmup | ✅ |
| **Conversation search** | Within-session RAG for compacted conversation history. | — | `tool/conversation_search.rs`: conversation-aware search | ✅ |

## XIII. Auth & Secrets

*Authentication providers, OAuth flows, credential storage, and secret management.*

| Name | Description | Source Repo(s) | jcode Impl | Status |
|------|-------------|----------------|------------|--------|
| **Auth types** | Auth state machine: Available, Expired, NotConfigured. Provider-level auth status. | CCB (auth) | `auth-types/`: `AuthState`, `AuthProvider` | ✅ |
| **OS keyring** | Platform-native credential manager: macOS Keychain, Linux Secret Service, Windows Credential Manager. | — | `keyring-store/`: `KeyringStore` trait, `DefaultKeyringStore`, `MockKeyringStore` | ✅ |
| **Azure auth** | Azure AD authentication for Azure-hosted models (Bedrock, OpenAI). | — | `azure-auth/`: Azure AD token acquisition | ✅ |
| **Secrets management** | In-memory secrets store with optional encryption. | — | `secrets/`: secure secret storage | ✅ |
| **External auth** | Third-party OAuth for provider authentication (Anthropic, Google, GitHub). | CCB (OAuth) | `external_auth.rs`: OAuth flow handler | ✅ |

## XIV. Reference Repo Gaps (from feature-planning skill)

*Features present in reference repos that jcode does not yet have or only partially implements.*

| Feature | Source Repo | jcode Status | Notes |
|---------|-------------|-------------|-------|
| **WASM extension security** | pi-agent-rust | ❌ Not implemented | pi-agent-rust has WASM-based extension sandboxing. jcode has native plugin system but no WASM sandbox. |
| **SSE streaming** | pi-agent-rust | ❌ Not found | Server-Sent Events for real-time streaming. May exist in protocol layer. |
| **ACP / Remote control** | claude-code (CCB) | ⚠️ Partial | jcode has remote protocol but ACP-style remote agent control not verified. |
| **Sandbox execution** | codex | ❌ Skipped | Container/firewall-based sandbox. Marked as skipped by design decision. |
| **40+ providers** | oh-my-pi | ⚠️ Partial | jcode has 10 provider crates. oh-my-pi claims 40+. |
| **IDE wiring** (VS Code) | oh-my-pi | ❌ Not found | IDE integration. jcode is terminal-first. |
| **DAP operations** (27 ops) | oh-my-pi | ⚠️ Partial | jcode has LSP (9 ops) but no DAP (Debug Adapter Protocol). |
| **Computer Use** | CCB | ⚠️ Partial | jcode has `macos_computer_use` tool (macOS-only). Full cross-platform Computer Use not implemented. |
| **Chrome Use** | CCB | ❌ Not found | Chrome browser automation. jcode has `browser` tool with Firefox bridge. |
| **Voice Mode** | CCB | ❌ Not found | Speech-to-text / text-to-speech. Whisper transcript type exists in protocol tests only. |
| **Pipe IPC multi-instance** | CCB | ❌ Not found | Cross-instance Pipe IPC with auto-orchestration. |
| **Langfuse monitoring** | CCB | ❌ Not found | Langfuse observability integration. jcode has `telemetry-core` but no Langfuse adapter. |
| **Remote Control Docker UI** | CCB | ❌ Not found | Docker self-hosted remote UI for phone access. |
| **Tmux integration** | oh-my-openagent | ⚠️ Partial | `team.rs` mentions tmux layout. Dedicated tmux session management not verified. |
| **Prompt variants per model** | oh-my-openagent | ❌ Not found | Same agent, different prompt per provider (Claude vs GPT vs Gemini). |
| **Tree-sitter code map** | codebuff (10+ languages) | ⚠️ Partial | tree-sitter used only in `jcode-edit-bench`. No general code-map tool. |
| **io_uring fast lane** | pi-agent-rust | ❌ Not attempted | Linux-specific. jcode uses tokio. |
| **Shadow dual execution** | pi-agent-rust | ❌ Not attempted | Runs two models and compares. Complex infrastructure. |

## XV. Known Gaps — Features Not Yet Tracked

> **⛔ PREVIOUSLY UNTRACKED — NOW IN SECTIONS VIII–XIII:** The domains below were
> identified as untracked during a gap analysis and have since been added to the
> registry (Tools → §VIII, Provider → §IX, Plugin → §X, Desktop → §XI,
> Embedding → §XII, Auth → §XIII). The table below is kept for historical reference
> and cross-checking.

| Domain | Crates / Files | Approx. Scope | Notes |
|--------|---------------|---------------|-------|

### Reference Repo Feature Gaps (from feature-planning skill)

| Feature | Source Repo | jcode Status | Notes |
|---------|-------------|-------------|-------|
| **WASM extension security** | pi-agent-rust | ❌ Not implemented | pi-agent-rust has WASM-based extension sandboxing. jcode has plugin system but no WASM sandbox. |
| **SSE streaming** | pi-agent-rust | ❌ Not found | Server-Sent Events for real-time streaming. May exist in jcode protocol layer. |
| **ACP / Remote control** | claude-code (CCB) | ⚠️ Partial | jcode has remote protocol (`jcode-protocol`) but ACP-style remote agent control not verified. |
| **Sandbox execution** | codex | ❌ Skipped | Marked as skipped in PARITY.md. Requires container infrastructure. |
| **40+ providers** | oh-my-pi | ⚠️ Partial | jcode has 10 provider crates + core abstraction. oh-my-pi claims 40+. |
| **IDE wiring** | oh-my-pi | ❌ Not found | VS Code / editor integration. jcode is terminal-first. |
| **DAP operations** | oh-my-pi | ⚠️ Not verified | jcode has LSP tool but DAP (Debug Adapter Protocol) not confirmed. |

### Adding New Features

1. Pick the matching section (I-IX). If none matches, add a new top-level section.
2. Add a row: Name, Description, Source Repo(s), jcode Impl, Status, Remaining.
3. Update the summary table at the bottom.
