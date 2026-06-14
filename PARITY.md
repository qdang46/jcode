# jcode ↔ Claude Code Parity — Subagents

> Feature-by-feature comparison of **subagent/agent features** between jcode (`quangdang46/jcode`) and Claude Code.  
> Only subagent-related features are tracked here. Other domains (model providers, LSP/DAP, deployment, etc.) are excluded.

---

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Complete — matches Claude Code UX |
| ⚠️ | Partial — works but missing depth |
| ❌ | Not implemented |

---

## 1. Agent Definitions

*File format, storage, loading, validation.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **File format** | TOML-based definition. Fields: id, display_name, model_override, tool_names, ... | `.claude/agents/*.md` YAML frontmatter | `definition.rs`: `AgentDefinition` struct | ✅ | — |
| **Registry & loading** | 3-tier priority: Builtin < UserGlobal < ProjectLocal. load_directory, register_builtin, iter_sorted, conflict resolution. | 4 scopes (managed/project/user/plugin) | `registry.rs`: `AgentRegistry` | ✅ | — |
| **Storage scopes** | Agent file directories. | managed, project, user, plugin | `~/.jcode/agents/`, `.jcode/agents/` | ⚠️ | Add managed scope (read-only) + plugin scope. Currently 2/4. |
| **Validation** | Validate agent file on load. Error/warning reporting. | AgentValidationResult | `AgentDefinition::validate()` | ✅ | — |
| **Agent prompts** | 5 slots: system_prompt, instructions_prompt, step_prompt, spawner_prompt. Cache sharing via inherit_parent_system_prompt. | AgentTool prompts, skill system | `definition.rs`: all prompt fields. Built-in agents use them. | ✅ | — |

---

## 2. Agent Lifecycle

*Spawning, running, completion, status tracking.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Spawning** | Spawn subagent from parent. Pass context, inherit prompt, configure tools. | `src/utils/swarm/spawnInProcess.ts` | Agent runtime spawn via AgentTarget + model resolution | ✅ | — |
| **Lifecycle states** | Start → running → completed/failed/stopped. Visible in UI. | `src/tasks/LocalAgentTask/LocalAgentTask.tsx` | `running_items.rs`: status icons. SwarmMemberStatus from server. | ✅ | — |
| **Background execution** | Run agents in background (non-blocking). Progress tracking, notifications, wake. | `src/hooks/useBackgroundAgentTasks.ts` | `background::global()`, `BackgroundTaskManager` | ✅ | — |
| **Forked agents** | Fork with full context inheritance. In-process execution. | `src/utils/forkedAgent.ts` | In-process spawning via agent runtime | ✅ | — |
| **Max turns** | Limit agent turns to prevent runaway. | maxTurns field | `definition.rs`: `max_turns: Option<u32>` | ✅ | — |
| **Stop/kill** | Cancel running subagent, tool, or background task. | `src/tasks/stopTask.ts`, `src/hooks/useCancelRequest.ts` | Ctrl+C / Backspace → `cancel_requested = true` | ✅ | — |

---

## 3. Agent UI — Running Items

*Below status bar interactive list of live agents/tools/tasks.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Running items list** | Interactive list below status bar: subagents, shell commands, background tasks. ↓/↑ navigate, Enter detail, Esc close. | `src/hooks/useBackgroundAgentTasks.ts` | `ui_running_items.rs`, `ui.rs` (chunks[8]), Ctrl+O toggle | ✅ | — |
| **Status icons** | Running (◯), Completed (✓), Failed (✗), Stopped (■). Colored per status. | Agent status icons | `ui_running_items.rs`: `item_icon_and_color()` | ✅ | — |
| **Elapsed time** | Show duration for running items. Right-aligned. | Task timers | `format_elapsed()` in `ui_running_items.rs` | ✅ | — |
| **Selection highlight** | ❯ indicator for selected item. Bold label. | Arrow selection | `draw_running_items()`: `❯` prefix + bold style | ✅ | — |
| **Scroll for long lists** | Max 5 items visible. Scroll offset for overflow. | Pagination | `scroll_offset` calculation in `draw_running_items()` | ✅ | — |

---

## 4. Agent UI — Detail Overlay

*Popup with live status when Enter is pressed on an item.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Overlay popup** | Rounded border popup showing item info. | `src/components/agents/AgentDetail.tsx` | `draw_running_item_detail()` in `ui_running_items.rs` | ✅ | — |
| **Real-time update** | Content rebuilt every frame. Status/elapsed update live. | Live rendering | Called from `draw_inner()` each frame | ✅ | — |
| **Detail fields** | Shows: status, kind, id, session, elapsed, detail text. | Agent detail view | Dynamic content built per frame | ✅ | — |
| **Action hints** | "Enter to open session", "Ctrl+C to cancel", "Esc to close". | Action hints | Dynamic hints based on item status + session_id | ✅ | — |
| **Session attachment** | Enter while detail open → switch to subagent's session. | `src/hooks/useRemoteSession.ts` | `workspace_client.queue_resume_session(sid)` | ✅ | — |

---

## 5. Agent UI — `/agents` Command

*Tabbed interface: Running tab (live) + Library tab (saved agents).*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Command registration** | `/agents` launches agent management UI. | `src/commands/agents/index.ts` | `/agents` → `open_agents_picker()` | ✅ | — |
| **Tab switching** | Tab/BackTab/→/← switch between Running and Library tabs. | Tab interface | `inline_interactive.rs`: Tab ↔ column switch | ✅ | — |
| **Running tab entries** | Live subagents, background tasks, batch tools, swarm members. | Running tab | `build_running_tab_entries()` in `openers.rs` | ✅ | — |
| **Running → Enter** | Enter on running item → close picker, open running items list. | Open running item | `running_items_state.visible = true` | ✅ | — |
| **Library tab entries** | Agent files from disk + create/generate actions. | Library tab | Load from AgentRegistry | ✅ | — |
| **Enter on agent file** | Open $EDITOR with agent TOML file. | `src/components/agents/AgentEditor.tsx` | `PickerAction::EditAgent` → `$EDITOR` | ✅ | — |
| **Enter on model override** | Open model picker for agent type (Swarm/Review/etc.). | Agent model config | `PickerAction::AgentTarget` → `open_agent_model_picker()` | ✅ | — |
| **Delete agent** | Remove agent TOML file from disk. | `src/components/agents/agentFileUtils.ts` | `PickerAction::DeleteAgent` → `std::fs::remove_file` | ✅ | — |

---

## 6. Agent Management — Library Actions

*Create, edit, generate, delete agent definitions.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Create agent (manual)** | Open $EDITOR with TOML template. Parse and save to disk. | Manual creation | `run_agent_creation_flow()` in `openers.rs` | ✅ | — |
| **Generate via AI** | Open $EDITOR with prompt template. User describes agent. Queue to current model. | `src/components/agents/generateAgent.ts` | `PickerAction::GenerateAgent` → `queued_messages.push()` | ⚠️ | Response appears in chat. Must manually save. Auto-save missing. |
| **Edit agent** | Open $EDITOR with agent TOML file. Save changes. | `src/components/agents/AgentEditor.tsx` | `PickerAction::EditAgent` → `$EDITOR` flow | ✅ | — |
| **Delete agent** | Remove agent file from disk. | `src/components/agents/agentFileUtils.ts` | `PickerAction::DeleteAgent` → `std::fs::remove_file` | ✅ | — |
| **Color badge** | Display color indicator in agent list entry. | Color badge in list | `agent_color_icon()` → `●` prefix in name | ✅ | Actual colored rendering (ratatui Span). Currently plain char. |

---

## 7. Agent Interaction

*Attaching to sessions, viewing transcripts, communicating with agents.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Attach to running agent** | Enter on subagent item → switch to that agent's session. View live transcript. | Session switching | `queue_resume_session(sid)` on Enter | ✅ | — |
| **View agent transcript** | Open agent's conversation history. | Agent transcript view | Via session resume → shows full transcript | ✅ | — |
| **Inter-agent messaging** | Agents communicate via shared context and notifications. | `src/utils/teammateMailbox.ts` | ServerEvent::Notification, CommReadContext | ✅ | — |
| **Agent teams** | Multi-agent coordination with task DAG. TeamView widget. | `src/utils/swarm/teamHelpers.ts` | TeamView widget in info_widget | ⚠️ | Informational only. No interactive team management. |
| **Agent context visualization** | Per-agent token usage display. | Context command | — | ❌ | Track per-agent tokens. Render in detail/info widget. |

---

## 8. Agent Configuration

*Model override, tools, permissions, colors.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Model override** | Per-agent model selection. Override via model_prefs.json or agent file. | Agent model field | `inline_interactive/helpers.rs`: save/load overrides. Agent file: `model_override`. | ✅ | — |
| **Tool whitelist** | `tool_names`: only these tools available. | tools field | `definition.rs`: `tool_names: Vec<String>` | ✅ | — |
| **Tool denylist** | `disallowed_tools`: block specific tools. | Tool deny system | `definition.rs`: `disallowed_tools: Vec<String>` | ✅ | — |
| **Spawnable agents** | `spawnable_agents`: which sub-agents can be spawned. | Spawn control | `definition.rs`: `spawnable_agents: Vec<String>` | ✅ | — |
| **Permission mode** | Per-agent permission override (Plan, AcceptEdits, etc.). | permissionMode field | `definition.rs`: `permission_mode: Option<PermissionMode>` | ✅ | — |
| **Agent colors** | 8 named colors: red/blue/green/yellow/purple/orange/pink/cyan. | `agentColorManager.ts` | `definition.rs`: `color: Option<String>`. Badge in list. | ✅ | Ratatui Span colored rendering. Color picker UI. |
| **Reasoning effort** | Per-agent reasoning level (minimal/low/medium/high). | effort field | `definition.rs`: `reasoning: Option<ReasoningEffort>` | ✅ | — |

---

## 9. Agent File Operations

*File I/O for agent definitions.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Load agents from disk** | Read `.toml` files from agent directories. | `src/components/agents/agentFileUtils.ts` | `AgentRegistry::load_directory()` | ✅ | — |
| **Save agent to disk** | Write agent definition as `.toml` file. | `saveAgentToFile()` | `run_agent_creation_flow()` → `std::fs::write()` | ✅ | — |
| **Delete agent from disk** | Remove agent `.toml` file. | `deleteAgentFromFile()` | `PickerAction::DeleteAgent` → `std::fs::remove_file()` | ✅ | — |
| **Built-in agents** | 4 shipped agents: basher, code-reviewer, editor, file-picker. | Built-in agents | `.jcode/agents/*.toml` + colors assigned | ✅ | — |
| **`/agents save`** | Save generated agent TOML from chat response. | Auto-save after AI gen | — | ❌ | Parse ```toml from last assistant message. Write to `~/.jcode/agents/`. |
| **AI generation auto-save** | Model generates → auto-parse → auto-save. No manual step. | `generateAgent.ts` | Response in chat. User must manually save. | ❌ | Hook into turn completion. Auto-detect TOML. Auto-save. |

---

## 10. Agent UI Customization

*Visual customization of agent appearance.*

| Name | Features | References (CCB) | jcode Impl | Progress | Remaining |
|------|----------|-------------------|------------|----------|-----------|
| **Color picker UI** | Interactive picker with 8 color swatches + preview. | `src/components/agents/ColorPicker.tsx` | — | ❌ | Add `PickerKind::ColorPicker`. 8 entries + "Automatic". |
| **Agent edit menu** | Change model/tools/color via pickers (not raw file). | `src/components/agents/AgentEditor.tsx` | Opens $EDITOR with raw TOML | ❌ | Model picker, tools list, color picker. |
| **Create wizard** | Multi-step wizard: location → method → type → prompt → tools → model → color → confirm. | `CreateAgentWizard.tsx` (10+ steps) | Single $EDITOR step | ❌ | Multi-step wizard with inline pickers. |

---

## Summary

| Section | Features | ✅ Complete | ⚠️ Partial | ❌ Missing |
|---------|----------|-------------|-------------|-----------|
| 1 — Agent Definitions | 5 | 4 | 1 | 0 |
| 2 — Agent Lifecycle | 6 | 6 | 0 | 0 |
| 3 — Agent UI: Running Items | 5 | 5 | 0 | 0 |
| 4 — Agent UI: Detail Overlay | 5 | 5 | 0 | 0 |
| 5 — Agent UI: /agents Command | 8 | 8 | 0 | 0 |
| 6 — Library Actions | 5 | 4 | 1 | 0 |
| 7 — Agent Interaction | 5 | 4 | 1 | 0 |
| 8 — Agent Configuration | 7 | 7 | 0 | 0 |
| 9 — Agent File Operations | 6 | 4 | 0 | 2 |
| 10 — Agent UI Customization | 3 | 0 | 0 | 3 |
| **Total** | **55** | **47 (85%)** | **3 (5%)** | **5 (9%)** |

### Missing Features (Priority Order)

| Priority | Feature | Section | Effort | Note |
|----------|---------|---------|--------|------|
| P0 | `/agents save` | 9 | Low | Parse ` ```toml ` from last assistant message. |
| P1 | AI auto-save | 9 | Medium | Hook turn completion → detect TOML → save. |
| P1 | Color picker UI | 10 | Medium | 8 swatches + Preview. |
| P2 | Agent edit menu | 10 | Medium | Model/tools/color inline pickers. |
| P2 | Agent creation wizard | 10 | High | Multi-step with inline pickers. |
| P2 | Context visualization | 7 | Medium | Per-agent token usage. |
| P3 | Agent scopes | 1 | Low | managed + plugin scope dirs. |
| P3 | Interactive team mgmt | 7 | High | TeamView → interactive. |
| — | Color badge rendering | 6 | Low | `●` → actual ratatui Span color. |
