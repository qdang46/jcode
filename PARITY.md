# jcode Feature Registry

> Feature inventory tracking implementation status and source references across reference repos (Claude Code, opencode, codebuff, pi-agent-rust, oh-my-openagent, codex, oh-my-pi, oh-my-claudecode, oh-my-codex).  
> Organized by feature domain. New features should be added to the appropriate section.

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
## 1. Agent Running Items

*Interactive list below status bar showing live agents, tools, and tasks.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Running items list** | Interactive list below status bar. Shows subagents, shell commands, background tasks. ↓/↑ navigate, Enter detail, Esc close. Toggle via Ctrl+O. | CCB (running items), opencode (task list) | `ui_running_items.rs`, `ui.rs` chunks[8], `input.rs` Ctrl+O | ✅ | — |
| **Status icons** | Running ◯, Completed ✓, Failed ✗, Stopped ■. Colored by status. | CCB (status icons) | `item_icon_and_color()` in `ui_running_items.rs` | ✅ | — |
| **Elapsed time display** | Duration shown for running items. Right-aligned. | CCB (timestamps) | `format_elapsed()` in `ui_running_items.rs` | ✅ | — |
| **Selection highlight** | ❯ prefix + bold label for selected item. | CCB (arrow navigation) | `draw_running_items()` selection styling | ✅ | — |
| **Scroll overflow** | Max 5 items visible. Scroll offset for overflow. | CCB (pagination) | `scroll_offset` in `draw_running_items()` | ✅ | — |

---

## 2. Agent Detail Overlay

*Popup showing live agent/tool information.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Detail popup** | Rounded border overlay showing item info. | CCB (AgentDetail), opencode (detail view) | `draw_running_item_detail()` in `ui_running_items.rs` | ✅ | — |
| **Real-time update** | Content rebuilt every frame. Status/elapsed update live. | CCB (live update) | Called from `draw_inner()` each frame | ✅ | — |
| **Detail fields** | Status, kind, id, session id, elapsed, detail text. | CCB (AgentDetail.tsx) | Dynamic content per frame | ✅ | — |
| **Action hints** | Context-sensitive: "Enter to open session", "Ctrl+C to cancel", "Esc to close". | CCB (action hints) | Dynamic hints based on status + session_id | ✅ | — |
| **Cancel action** | Backspace or Ctrl+C to cancel running item. | CCB (stopTask), codex (interrupt) | `input.rs`: `cancel_requested = true` | ✅ | — |

---

## 3. Agent Session Attachment

*Switching to a running agent's session to view transcript.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Attach to session** | Enter on subagent item → switch to that agent's session via `queue_resume_session(sid)`. | CCB (session switch) | `input.rs`, `key_handling.rs` | ✅ | — |
| **View transcript** | See agent's conversation history after attaching. | CCB (transcript view) | Session resume → full transcript render | ✅ | — |
| **Inter-agent messaging** | Agents communicate via shared context and notifications. | CCB (teammateMailbox), oh-my-openagent (delegate-task) | `ServerEvent::Notification`, `CommReadContext` | ✅ | — |
| **Agent context visualization** | Per-agent token usage display. | CCB (context command), opencode (context widget) | — | ❌ | Track per-agent tokens. Render in detail/info widget. |

---

## 4. Agent Definitions

*File format, storage, loading, validation.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **File format** | TOML-based definition. Fields: id, display_name, model_override, tool_names, system_prompt, instructions_prompt, step_prompt, spawner_prompt, inherit_parent_system_prompt, include_message_history, permission_mode, max_turns, output_mode, output_schema, color, reasoning. | CCB (YAML frontmatter), pi-agent-rust (config format) | `definition.rs`: `AgentDefinition` struct | ✅ | — |
| **Registry** | 3-tier priority: Builtin < UserGlobal < ProjectLocal. load_directory, register_builtin, iter_sorted, conflict resolution. | CCB (4 scopes), pi-agent-rust (registry) | `registry.rs`: `AgentRegistry` | ✅ | — |
| **Storage scopes** | Agent file directories. | CCB (managed/project/user/plugin) | `~/.jcode/agents/`, `.jcode/agents/` | ⚠️ | Add managed (read-only) + plugin scope. |
| **Validation** | Validate agent file on load. Error/warning reporting. | CCB (AgentValidationResult) | `AgentDefinition::validate()` | ✅ | — |
| **Prompt system** | 5 prompt slots. Cache sharing via inherit_parent_system_prompt (prompt cache prefix trick). | CCB (AgentTool prompts), oh-my-openagent (per-model prompts) | `definition.rs`: system/instructions/step/spawner prompts | ✅ | — |

---

## 5. Agent Lifecycle

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

## 6. Tool & Permission System

*Per-agent tool restrictions and permission modes.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Tool whitelist** | `tool_names`: only these tools available to agent. | CCB (tools field), codex (sandbox) | `definition.rs`: `tool_names: Vec<String>` | ✅ | — |
| **Tool denylist** | `disallowed_tools`: block specific tools. | CCB (tool deny), oh-my-pi (tool gating) | `definition.rs`: `disallowed_tools: Vec<String>` | ✅ | — |
| **Spawnable agents** | `spawnable_agents`: which sub-agents can be spawned. | CCB (spawn control) | `definition.rs`: `spawnable_agents: Vec<String>` | ✅ | — |
| **Permission mode** | Per-agent override (Plan, AcceptEdits, etc.). | CCB (permissionMode), codex (execution policy), oh-my-claudecode (hooks) | `definition.rs`: `permission_mode: Option<PermissionMode>` | ✅ | — |
| **Reasoning effort** | Per-agent reasoning level (minimal/low/medium/high). | CCB (effort), oh-my-openagent (model-variant routing) | `definition.rs`: `reasoning: Option<ReasoningEffort>` | ✅ | — |

---

## 7. Agent Colors

*Visual agent identity via named colors.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Color field** | 8 named colors: red/blue/green/yellow/purple/orange/pink/cyan. Stored in agent definition. | CCB (AgentColorName, agentColorManager.ts) | `definition.rs`: `color: Option<String>` | ✅ | — |
| **Color badge** | Colored badge displayed in agent list. | CCB (color badge in AgentsList) | `agent_color_icon()` → `●` prefix in entry name | ⚠️ | Proper ratatui Span colored rendering (currently plain `●` char). |
| **Color picker** | Interactive UI to choose agent color from 8 swatches + "Automatic". | CCB (ColorPicker.tsx) | — | ❌ | `PickerKind::ColorPicker` with 9 entries. |

---

## 8. `/agents` Command

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

## 9. Agent Creation

*Creating new agent definitions.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Manual creation** | Open $EDITOR with TOML template. Parse and save to disk. | CCB (manual create) | `run_agent_creation_flow()` in `openers.rs` | ✅ | — |
| **AI generation** | Open $EDITOR with prompt template. User describes agent. Queue to current model. | CCB (generateAgent.ts — Claude API) | `PickerAction::GenerateAgent` → `queued_messages.push()` | ⚠️ | Response in chat. Must manually save. Auto-save missing. |
| **`/agents save`** | Save generated agent TOML from last model response. | CCB (auto-save after AI gen) | — | ❌ | Parse ` ```toml ` from last assistant message. |
| **AI auto-save** | Model generates → auto-parse → auto-save. Zero manual steps. | CCB (generateAgent → programmatic save) | — | ❌ | Hook turn completion. Auto-detect TOML. Auto-save. |
| **Creation wizard** | Multi-step guided wizard: location → method → type → prompt → tools → model → color → confirm. | CCB (CreateAgentWizard.tsx — 10+ steps) | Single $EDITOR step | ❌ | Multi-step wizard with inline pickers. |
| **Edit menu** | Change model/tools/color via pickers, not raw file editing. | CCB (AgentEditor.tsx) | Opens $EDITOR with raw TOML | ❌ | Model picker, tools list, color picker. |

---

## 10. `/tasks` Command

*Standalone background task management.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Command entry** | `/tasks` lists running/completed background tasks. | CCB (tasks/index.ts, tasks.tsx) | — | ❌ | Add `PickerKind::Tasks`. |
| **Attach to task** | Enter on task → view output/attach to session. | CCB (task attach) | — | ❌ | Data exists via `background::global().running_snapshot()`. |
| **Stop/kill task** | Cancel background task from task list. | CCB (stopTask) | — | ❌ | Cancel via `BackgroundTaskManager::cancel()`. |

---

## 11. Agent Teams & Swarm

*Multi-agent coordination.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Swarm members** | Remote swarm member lifecycle. Status via ServerEvent::SwarmStatus. | CCB (swarm backends) | `remote_swarm_members: Vec<SwarmMemberStatus>` | ✅ | — |
| **Swarm plan** | Plan synchronization. Plan proposals, coordinator mode. | CCB (coordinatorMode) | `swarm_plan_core.rs`, `ServerEvent::SwarmPlan` | ✅ | — |
| **Info widget** | Swarm member status in margin. Icons, names, roles. | CCB (teammate banner) | `info_widget_swarm_background.rs`: `render_swarm_widget()` | ✅ | — |
| **Agent teams** | Multi-agent task DAG. Team coordination. | oh-my-openagent (Atlas/delegate-task), codebuff (4-agent pipeline), CCB (teams) | TeamView widget | ⚠️ | Informational only. No interactive team management. |

---

## 12. Built-in Agents

*Pre-shipped agent definitions.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **basher** | Run terminal commands. One-shot bash executor. prefer_tier=routine, max_turns=10, permission_mode=accept-edits. | codebuff (bash agent), CCB (shell tools) | `.jcode/agents/basher.toml`. color=green. | ✅ | — |
| **code-reviewer** | Review code changes for bugs and regressions. prefer_tier=thinking, inherit_parent_system_prompt=true, permission_mode=plan. | codebuff (reviewer agent) | `.jcode/agents/code-reviewer.toml`. color=purple. | ✅ | — |
| **editor** | Precise code edits with hashline_edit. prefer_tier=thinking, inherit_parent_system_prompt=true, permission_mode=accept-edits. | oh-my-pi (hashline_edit), CCB (editor) | `.jcode/agents/editor.toml`. color=blue. | ✅ | — |
| **file-picker** | Find relevant files in codebase. prefer_tier=routine, permission_mode=plan, max_turns=5. | codebuff (file-picker agent) | `.jcode/agents/file-picker.toml`. color=cyan. | ✅ | — |

---

## 13. Model Override (Built-in Agent Types)

*Hardcoded agent types for model routing.*

| Name | Description | Source Repo(s) | jcode Impl | Status | Remaining |
|------|-------------|----------------|------------|--------|-----------|
| **Swarm override** | Model override for swarm subagents. | CCB (agent model config) | `AgentModelTarget::Swarm` via `model_prefs.json` | ✅ | — |
| **Review override** | Model override for review agent. | CCB | `AgentModelTarget::Review` | ✅ | — |
| **Judge override** | Model override for judge agent. | CCB | `AgentModelTarget::Judge` | ✅ | — |
| **Memory override** | Model override for memory agent. | CCB | `AgentModelTarget::Memory` | ✅ | — |
| **Ambient override** | Model override for ambient agent. | CCB | `AgentModelTarget::Ambient` | ✅ | — |

---

## Summary

| Section | Features | ✅ Complete | ⚠️ Partial | ❌ Missing |
|---------|----------|-------------|-------------|-----------|
| 1 — Running Items | 5 | 5 | 0 | 0 |
| 2 — Detail Overlay | 5 | 5 | 0 | 0 |
| 3 — Session Attachment | 4 | 3 | 0 | 1 |
| 4 — Agent Definitions | 5 | 4 | 1 | 0 |
| 5 — Agent Lifecycle | 6 | 6 | 0 | 0 |
| 6 — Tool & Permission | 5 | 5 | 0 | 0 |
| 7 — Agent Colors | 3 | 1 | 1 | 1 |
| 8 — `/agents` Command | 7 | 7 | 0 | 0 |
| 9 — Agent Creation | 6 | 1 | 1 | 4 |
| 10 — `/tasks` Command | 3 | 0 | 0 | 3 |
| 11 — Teams & Swarm | 4 | 3 | 1 | 0 |
| 12 — Built-in Agents | 4 | 4 | 0 | 0 |
| 13 — Model Override | 5 | 5 | 0 | 0 |
| **Total** | **62** | **49 (79%)** | **4 (6%)** | **9 (15%)** |

### Missing / Partial Features (Priority)

| Priority | Feature | Section | Effort | Reference |
|----------|---------|---------|--------|-----------|
| P0 | `/tasks` command | 10 | Low | CCB: `src/commands/tasks/` |
| P0 | `/agents save` | 9 | Low | Parse ` ```toml ` from assistant message |
| P1 | AI auto-save | 9 | Medium | Hook turn completion |
| P1 | Color picker UI | 7 | Medium | CCB: ColorPicker.tsx (8 swatches) |
| P2 | Agent edit menu | 9 | Medium | CCB: AgentEditor.tsx |
| P2 | Agent scopes | 4 | Low | CCB: 4 scopes -> add managed + plugin |
| P2 | Context visualization | 3 | Medium | CCB: context command |
| P2 | Creation wizard | 9 | High | CCB: CreateAgentWizard.tsx (10+ steps) |
| P3 | Interactive team mgmt | 11 | High | oh-my-openagent: delegate-task |
| — | Color badge rendering | 7 | Low | Plain `●` -> ratatui Span color |

### Adding New Features

1. Pick the matching section (1-13). If none matches, add a new section.
2. Add a row: Name, Description, Source Repo(s), jcode Impl, Status, Remaining.
3. Update the summary table at the bottom.
