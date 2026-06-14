# jcode ↔ Claude Code Parity

> Feature-by-feature comparison of subagent/agent UI/UX between jcode (`quangdang46/jcode`) and Claude Code.  
> Each row tracks implementation status, source references, and remaining work.

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Complete — matches Claude Code UX |
| ⚠️ | Partial — works but missing depth |
| ❌ | Not implemented |

---

## Section A — Core TUI (Running Items)

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Running items list** | Interactive list below status bar: subagents, shell commands, background tasks. ↓/↑ navigate, Enter detail, Esc close. | `src/hooks/useBackgroundAgentTasks.ts`, `src/hooks/useTasksV2.ts` | `ui_running_items.rs`, `ui.rs` (chunks[8]), `input.rs` (Ctrl+O) | ✅ | — |
| **Detail overlay** | Rounded border popup with live status. Real-time update (rebuilt per frame). Shows status/kind/ID/session/elapsed. Backspace or Ctrl+C to cancel. | `src/components/agents/AgentDetail.tsx` | `ui_running_items.rs`: `draw_running_item_detail()` | ✅ | — |
| **Attach to subagent session** | Enter on subagent/swarm member → switch to that agent's session via `queue_resume_session(sid)`. Shows live transcript. | `src/hooks/useRemoteSession.ts` | `input.rs`, `key_handling.rs`: Enter → `workspace_client.queue_resume_session(sid)` | ✅ | — |
| **Stop/kill item** | Cancel running subagent/tool/background task via existing interrupt infrastructure. | `src/tasks/stopTask.ts`, `src/hooks/useCancelRequest.ts` | `input.rs`: Ctrl+C / Backspace → `cancel_requested = true` | ✅ | — |
| **`/tasks` command** | Standalone command listing running/completed background tasks. Attach to task output, stop/kill tasks. | `src/commands/tasks/index.ts`, `src/commands/tasks/tasks.tsx`, `src/hooks/useTasksV2.ts` | — | ❌ | Add `PickerKind::Tasks`, `open_tasks_picker()`. Data exists via `background::global().running_snapshot()`. |
| **Context window visualization** | Per-subagent context token usage shown in banner or info widget. | `src/commands/context/index.ts`, `src/components/PromptInput/useSwarmBanner.ts` | — | ❌ | Track per-subagent token usage. Render in detail overlay or info widget. |

---

## Section B — `/agents` Command

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Running tab** | Tab 0: live subagents, background tasks, batch tools, swarm members. Enter opens detail or running items list. | `src/commands/agents/index.ts`, `src/commands/agents/agents.tsx` | `openers.rs`: `open_agents_picker()` + `build_running_tab_entries()`. Tab/Right switches tab. | ✅ | — |
| **Library tab** | Tab 1: agent definitions from disk. `+ Create new agent` (manual TOML via $EDITOR). `+ Generate via AI` (prompt → queue to current model). Color badge display. | `src/components/agents/AgentsList.tsx`, `src/components/agents/agentFileUtils.ts`, `src/components/agents/AgentEditor.tsx`, `src/components/agents/generateAgent.ts` | `openers.rs`: `open_agents_picker()` loads via `AgentRegistry`. `run_agent_creation_flow()` parses TOML, saves. `agent_color_icon()` badge. | ✅ | — |
| **Color picker** | Interactive 8-swatch picker with live preview. Set agent color from UI. | `src/components/agents/ColorPicker.tsx` | — | ❌ | Add `PickerKind::ColorPicker`. 8 color entries + "Automatic". |
| **Edit menu** | Change model/tools/color without editing file. Inline pickers for each field. | `src/components/agents/AgentEditor.tsx` | $EDITOR opens raw TOML file | ❌ | Add model picker, tools list, color picker to edit flow. |
| **`/agents save`** | Save generated agent TOML from last model response. Parse from code block, write to disk. | — (CCB auto-saves after AI generation) | — | ❌ | Parse last assistant message for ```toml block. Save to `~/.jcode/agents/`. |
| **AI generation auto-save** | After model generates agent definition in chat, auto-parse and save without user action. | `src/components/agents/generateAgent.ts` (Claude API → programmatic save) | User queues message manually, response appears in chat | ❌ | Hook into turn completion. Detect TOML in response. Save automatically. |

---

## Section C — Agent Definitions

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **File format** | TOML-based (jcode) vs Markdown + YAML (CCB). Fields: id, display_name, model_override, tool_names, system_prompt, ... | `.claude/agents/*.md` YAML frontmatter | `definition.rs`: `AgentDefinition` struct (TOML) | ✅ | — |
| **Registry & loading** | 3-tier priority: Builtin < UserGlobal < ProjectLocal. `load_directory()`, `register_builtin()`, `iter_sorted()`, conflict resolution. | `.claude/agents/` 4 scopes | `registry.rs`: `AgentRegistry` | ✅ | — |
| **Storage scopes** | Agent file locations. CCB: 4 scopes (managed/project/user/plugin). jcode: 2 scopes (user/project). | `src/components/agents/agentFileUtils.ts`: `getAgentDirectoryPath(SettingSource)` | User: `~/.jcode/agents/`. Project: `.jcode/agents/`. | ⚠️ | Add managed scope (read-only builtin dir) and plugin scope. |
| **Tool restrictions** | `tool_names` whitelist, `disallowed_tools` denylist, `spawnable_agents` whitelist. Enforced at runtime. | `.claude/agents/*.md` `tools:` field, `src/Tool.ts` | `definition.rs`: all three fields defined. Runtime enforcement in agent runtime. | ✅ | — |
| **Permission modes** | Override permission_mode per agent (Plan/AcceptEdits). Override max_turns. | `.claude/agents/*.md` `permissionMode:`, `maxTurns:` | `definition.rs`: `permission_mode: Option<PermissionMode>`, `max_turns: Option<u32>` | ✅ | — |
| **Model override** | Override model per agent type (Swarm/Review/Judge/Memory/Ambient). Stored in `model_prefs.json`. | CCB: agent `model:` field | `inline_interactive/helpers.rs`: `save_agent_model_override()`, `load_agent_model_override()` | ✅ | — |
| **Agent prompts** | 5 prompt slots: system_prompt, instructions_prompt, step_prompt, spawner_prompt. Cache sharing via `inherit_parent_system_prompt`. | CCB AgentTool, skill system | `definition.rs`: all prompt fields. Built-in agents use spawner/instructions/system prompts. | ✅ | — |
| **Agent colors** | 8 named colors: red/blue/green/yellow/purple/orange/pink/cyan. Badge in agent list. | `agentColorManager.ts`: `AgentColorName`, `src/components/agents/ColorPicker.tsx` | `definition.rs`: `color: Option<String>`. `agent_color_icon()` badges. Built-in agents assigned colors. | ✅ | Proper ratatui Span colored rendering (currently plain `●` char). Need `color` field on `PickerEntry`. |

---

## Section D — Agent Runtime

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Built-in agents** | 4 shipped agents: basher (terminal), code-reviewer, editor (code edits), file-picker (file discovery). | CCB built-in agents | `.jcode/agents/*.toml`: 4 TOML files with full configs | ✅ | — |
| **Agent spawning** | Spawn subagent from parent session. Pass context, inherit prompt, configure tools. | `src/utils/swarm/spawnInProcess.ts`, `src/utils/forkedAgent.ts` | `jcode-agent-runtime`: spawn via `AgentTarget` + model resolution | ✅ | — |
| **Agent lifecycle** | Start → running → completed/failed/stopped. Lifecycle visible in running items and `/agents` Running tab. | `src/tasks/LocalAgentTask/LocalAgentTask.tsx` | `running_items.rs`: status icons. `SwarmMemberStatus` from server events. | ✅ | — |
| **Background agents** | Agents that run in background (not blocking main session). Progress tracking, notifications. | `src/hooks/useBackgroundAgentTasks.ts`, `src/tasks/LocalAgentTask/LocalAgentTask.tsx` | `background::global()`, `BackgroundTaskManager` | ✅ | — |
| **Agent teams** | Multi-agent coordination with task DAG. TeamView widget, tasklist. | `src/utils/swarm/teamHelpers.ts`, `src/coordinator/coordinatorMode.ts` | `info_widget_swarm_background.rs`: TeamView widget | ⚠️ | TeamView widget is informational only. No interactive team management. |

---

## Section E — Extensions & Plugins

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Plugin agents** | Load agents from plugins. Plugin-defined agent files with scoped availability. | `src/utils/plugins/loadPluginAgents.ts`, `src/utils/plugins/pluginLoader.ts` | — | ❌ | Need plugin system integration with AgentRegistry. |
| **Agent hooks** | Pre/post hook system for agent lifecycle events. Hooks defined in agent file. | `src/utils/hooks/execAgentHook.ts`, `src/utils/hooks/registerFrontmatterHooks.ts` | — | ❌ | Need to define hook points in agent runtime. |
| **ACP protocol** | Agent Communication Protocol for IDE integration (Zed/Cursor). Bridge agent control. | `src/services/acp/agent.ts`, `src/services/acp/bridge.ts` | — | ❌ | Separate feature, not TUI-related. |

---

## Summary

### By Section

| Section | Features | ✅ Complete | ⚠️ Partial | ❌ Missing |
|---------|----------|-------------|-------------|-----------|
| A — Core TUI | 6 | 4 | 0 | 2 |
| B — /agents Command | 6 | 2 | 0 | 4 |
| C — Agent Definitions | 8 | 7 | 1 | 0 |
| D — Agent Runtime | 5 | 4 | 1 | 0 |
| E — Extensions | 3 | 0 | 0 | 3 |
| **Total** | **28** | **17 (61%)** | **2 (7%)** | **9 (32%)** |

### Next Priorities

| Priority | Feature | Section | Effort | Why |
|----------|---------|---------|--------|-----|
| P0 | `/tasks` command | A | Low | Reuses existing `background::global()` infrastructure. Pickable with Enter attach. |
| P0 | `/agents save` | B | Low | Parse ` ```toml ` from last assistant message, write to `~/.jcode/agents/`. |
| P1 | AI generation auto-save | B | Medium | Hook into turn completion. Detect TOML, save automatically. |
| P1 | Color picker UI | B | Medium | 8 color entries + ratatui Span styling. |
| P2 | Agent edit menu | B | Medium | Model/tools/color inline pickers. |
| P2 | Context window | A | Medium | Per-agent token tracking + rendering. |
| P2 | Agent scopes | C | Low | Add managed (read-only) scope dir. |
| P3 | Plugin agents | E | High | Plugin loader integration. |
| P3 | Agent hooks | E | High | Lifecycle hook system. |
| P3 | ACP protocol | E | High | Cross-IDE agent protocol. |
