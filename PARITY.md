# jcode ↔ Claude Code Parity

> Feature-by-feature comparison between jcode (`quangdang46/jcode`) and reference tools (Claude Code, opencode, codebuff, pi-agent-rust, oh-my-openagent, codex, oh-my-pi).  
> Organized by domain area for extensibility — new features should be added to the appropriate section.

---

## Legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Complete — matches reference UX |
| ⚠️ | Partial — works but missing depth |
| ❌ | Not implemented |
| — | Not applicable / different approach |

---

## 1. Core Terminal UI

*Status bar, input area, message rendering, overlays, layout zones.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Status bar** | Mode icon, model name, provider, context usage bar, token count, cost, custom shell command (3 layers). Configurable segments. | CCB: `src/commands/statusline/index.ts` | `ui_input.rs`: `draw_status()` at line 537. Layer 1 (basic) + Layer 3 (custom command) done. | ⚠️ | Layer 2 (configurable segments via `status_line.segments`) NOT wired. `draw_status` always hardcodes mode→model→provider→context. |
| **Running items list** | Interactive list below status bar: subagents, shell commands, background tasks. ↓/↑ navigate, Enter detail, Esc close. | CCB: `src/hooks/useBackgroundAgentTasks.ts`, `src/hooks/useTasksV2.ts` | `ui_running_items.rs`, `ui.rs` (chunks[8]), `input.rs` (Ctrl+O toggle). | ✅ | — |
| **Detail overlay** | Rounded border popup with live item status. Real-time update (rebuilt per frame). Shows status/kind/ID/session/elapsed. Cancel action via Backspace/Ctrl+C. | CCB: `src/components/agents/AgentDetail.tsx` | `ui_running_items.rs`: `draw_running_item_detail()`. | ✅ | — |
| **Input area** | Multi-line text input, prompt history, autocomplete, slash commands. | CCB: `src/components/PromptInput/`, opencode `packages/tui/` | `ui_input.rs`, `input.rs`: text input, history, autocomplete. | ✅ | — |
| **Message/transcript** | Render conversation with user/assistant/tool messages. Stream text, tool calls, formatted output. | CCB: message rendering, opencode TUI | `ui_messages.rs`, `ui_tools.rs`: message rendering pipeline. | ✅ | — |
| **Overlays** | Notifications, toasts, queued messages, inline interactive pickers. | CCB: notification system | `ui_overlays.rs`: notification, queued, inline interactive. | ✅ | — |
| **Donut animation** | Idle animation at bottom of screen while waiting for model response. | CCB: spinner system | `animations.rs`: `draw_idle_animation()`. | ✅ | — |
| **Info widgets** | Floating/minimap panels in transcript margin: Overview, Todos, ContextUsage, ModelInfo, Memory, Git, Team, SwarmBackground, Tips. | CCB: `src/components/PromptInput/useSwarmBanner.ts` | `info_widget.rs`: comprehensive widget system with compact/expanded modes. | ✅ | — |
| **Context window visualization** | Per-subagent token usage visualization. Context pressure indicators. | CCB: `src/commands/context/index.ts`, opencode context widget | — | ❌ | Track per-subagent tokens. Render in detail overlay or info widget. |

---

## 2. Agent Management

*Agent definitions, lifecycle, spawning, background tasks, /agents command, /tasks command.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Agent file format** | TOML-based definition. Fields: id, display_name, model_override, tool_names, disallowed_tools, spawnable_agents, system_prompt, instructions_prompt, step_prompt, spawner_prompt, inherit_parent_system_prompt, include_message_history, permission_mode, max_turns, output_mode, output_schema, color. | CCB: `.claude/agents/*.md` YAML frontmatter. pi-agent-rust: agent config format. | `definition.rs`: `AgentDefinition` struct (TOML). Serialize/Deserialize. | ✅ | — |
| **Agent registry** | 3-tier priority: Builtin < UserGlobal < ProjectLocal. `load_directory()`, `register_builtin()`, `load_file()`, `iter_sorted()`, `get()`, conflict resolution, load errors. | CCB: 4 scopes (managed/project/user/plugin). | `registry.rs`: `AgentRegistry` with full API. | ✅ | — |
| **Agent storage scopes** | Directory locations for agent files. | CCB: 4 scopes: managed (read-only), project (`.claude/agents/`), user (`~/.claude/agents/`), plugin. | User: `~/.jcode/agents/`. Project: `.jcode/agents/`. | ⚠️ | Add managed scope (read-only builtin dir) and plugin scope. Currently 2/4 scopes. |
| **Agent colors** | 8 named colors: red/blue/green/yellow/purple/orange/pink/cyan. Color badge in agent list. | CCB: `agentColorManager.ts`, `src/components/agents/ColorPicker.tsx`. | `definition.rs`: `color: Option<String>`. Badge `●` in agent list. Built-in agents assigned colors. | ✅ | Proper ratatui Span colored rendering (currently plain `●`). Need `PickerEntry.color` field. |
| **Color picker UI** | Interactive 8-swatch picker with live preview. Set agent color from UI. | CCB: `src/components/agents/ColorPicker.tsx`. | — | ❌ | Add `PickerKind::ColorPicker`. 8 color entries + "Automatic". |
| **`/agents` command** | Tabbed interface: Running tab (live agents) + Library tab (saved agents). Tab/BackTab switch. | CCB: `src/commands/agents/index.ts`, `src/commands/agents/agents.tsx`. | `openers.rs`: `open_agents_picker()`. `inline_interactive.rs`: Tab/BackTab/Left/Right. | ✅ | — |
| **`/agents` — Running tab** | Live subagents, background tasks, batch tools, swarm members. Enter opens detail/running items. | CCB: `/agents` → Running tab. | `openers.rs`: `build_running_tab_entries()`. | ✅ | — |
| **`/agents` — Library tab** | Agent definitions from disk. `+ Create new agent` (manual TOML). `+ Generate via AI` (prompt→model). Enter edits agent file. | CCB: `src/components/agents/AgentsList.tsx`, `src/components/agents/AgentEditor.tsx`. | `openers.rs`: loads via `AgentRegistry`. `run_agent_creation_flow()`. | ✅ | — |
| **Agent edit menu** | Change model/tools/color without editing raw file. Inline pickers per field. | CCB: `src/components/agents/AgentEditor.tsx` (model/tools/color options). | Opens $EDITOR with raw TOML file. | ❌ | Add model picker, tools list, color picker to edit flow. |
| **Agent lifecycle** | Start → running → completed/failed/stopped. Visible in running items and `/agents` Running tab. | CCB: `src/tasks/LocalAgentTask/LocalAgentTask.tsx`. | `running_items.rs`: status icons. `SwarmMemberStatus` from server events. | ✅ | — |
| **Background agents** | Agents running in background (non-blocking). Progress tracking, notifications, wake. | CCB: `src/hooks/useBackgroundAgentTasks.ts`. pi-agent-rust: background task scheduling. | `background::global()`, `BackgroundTaskManager`. `info_widget_swarm_background.rs`. | ✅ | — |
| **`/tasks` command** | List running/completed background tasks. Attach to task, stop/kill. | CCB: `src/commands/tasks/index.ts`, `src/commands/tasks/tasks.tsx`. | — | ❌ | Add `PickerKind::Tasks`, `open_tasks_picker()`. Data exists via `background::global().running_snapshot()`. |
| **`/agents save`** | Save generated agent TOML from last model response. | CCB: auto-saves after AI generation. | — | ❌ | Parse ` ```toml ` from last assistant message. Write to `~/.jcode/agents/`. |
| **AI generation auto-save** | Model generates definition → auto-parse → auto-save. No user copy-paste. | CCB: `src/components/agents/generateAgent.ts` (Claude API → programmatic save). | User queues message, response appears in chat. | ❌ | Hook into turn completion. Detect TOML in response. Save automatically. |
| **Agent creation wizard** | Multi-step guided wizard: location → method → type → prompt → tools → model → color → memory → confirm. | CCB: `src/components/agents/new-agent-creation/CreateAgentWizard.tsx` (10+ steps). | Manual TOML via $EDITOR (1 step). | ❌ | Multi-step wizard with inline pickers. |
| **Agent validation** | Validate agent file on load. Report errors/warnings. | CCB: `AgentValidationResult` (isValid, warnings, errors). | `definition.rs`: `AgentDefinition::validate()` returns `Result<(), DefinitionError>`. | ✅ | — |

---

## 3. Model & Provider

*Model selection, provider routing, model resolution, streaming, failover.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Model picker** | Inline interactive picker listing all available models from all providers. Search/filter, favorites, recent. | opencode: model picker UI. oh-my-pi: 40+ provider model list. | `inline_interactive.rs`: `open_model_picker()`. Full picker with favorites, usage scoring, fuzzy search. | ✅ | — |
| **Provider management** | Login/logout providers. Account status. Provider catalog. | opencode: provider abstraction. oh-my-pi: 40+ providers. | `openers.rs`: login/logout pickers. `provider_catalog.rs`: catalog. | ✅ | — |
| **Model routing** | Tier-based routing (routine, thinking, quality). Fallback chain. | oh-my-pi: model routing. pi-agent-rust: model resolution. | `model_routing.rs`, `tier.rs`: tier resolution. | ✅ | — |
| **Model failover** | Automatic failover on model error/rate-limit. | CCB: failover system. | `model_failover.rs`: failover logic. | ✅ | — |
| **Agent model override** | Per-agent-type model override (Swarm/Review/Judge/Memory/Ambient). Stored in `model_prefs.json`. | CCB: agent `model:` field. | `inline_interactive/helpers.rs`: `save_agent_model_override()`, `load_agent_model_override()`. | ✅ | — |
| **Reasoning/effort** | Per-agent reasoning effort setting (minimal/low/medium/high). | CCB: reasoning effort. oh-my-openagent: model-variant routing. | `definition.rs`: `reasoning: Option<ReasoningEffort>`. | ✅ | — |

---

## 4. Session & History

*Session management, conversation history, replay, resume.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Session resume** | Resume previous session. `/resume` command with session picker. | CCB: session resume. pi-agent-rust: SQLite session store. | `session_picker.rs`: full session picker with preview. `workspace_client.queue_resume_session()`. | ✅ | — |
| **Session switching** | Switch between sessions. Enter → switch to subagent session via `queue_resume_session(sid)`. | CCB: session switching. | `input.rs`, `key_handling.rs`: Enter on subagent → resume session. | ✅ | — |
| **Transcript viewer** | View session transcript. `/transcript` command opens in viewer. | CCB: transcript viewing. | Session picker shows preview (20 messages). `/transcript` opens file in OS viewer. | ✅ | — |
| **Compact/compress** | Compress long sessions to save context window. | CCB: compaction system. pi-agent-rust: session compaction. | `compact.rs`: session memory compaction. | ✅ | — |
| **History search** | Search across session history. | CCB: history search. | History search via session files. | ✅ | — |

---

## 5. Tools & Permissions

*Tool system, tool restrictions, permission modes, sandboxing.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Tool registry** | Registered tool list. Tool metadata, arguments, allowed scopes. | CCB: `src/Tool.ts`. opencode: tool abstraction. | `Tool.rs`: tool trait + registry. 30+ tools. | ✅ | — |
| **Tool whitelist** | `tool_names` field: only these tools available to agent. | CCB: tools field in agent file. | `definition.rs`: `tool_names: Vec<String>`. | ✅ | — |
| **Tool denylist** | `disallowed_tools` field: block specific tools. | CCB: tool deny system. | `definition.rs`: `disallowed_tools: Vec<String>`. | ✅ | — |
| **Spawnable agents** | `spawnable_agents` field: which sub-agents this agent can spawn. | CCB: spawn control. | `definition.rs`: `spawnable_agents: Vec<String>`. | ✅ | — |
| **Permission modes** | Plan mode (read-only), AcceptEdits (batch auto-approve), etc. Per-agent override. | CCB: permission modes. codex: sandbox execution. | `permission.rs`: `PermissionMode` enum. `definition.rs`: `permission_mode: Option<PermissionMode>`. | ✅ | — |
| **Sandbox/Isolation** | Sandbox execution for untrusted code. Network isolation, filesystem isolation. | codex: firewall init script, container execution. pi-agent-rust: capability gates, hostcall security. | DCG (Dangerous Command Guard), `extension_policy.rs`. | ⚠️ | No network/filesystem sandbox (codex-style). DCG only covers dangerous shell commands. |
| **Context sharing** | Share parent context with spawned agent. Cache sharing via `inherit_parent_system_prompt`. | CCB: prompt cache prefix sharing. | `definition.rs`: `inherit_parent_system_prompt`, `include_message_history`. Built-in agents use cache sharing (editor, code-reviewer). | ✅ | — |
| **Max turns** | Limit agent turns to prevent runaway loops. | CCB: maxTurns field. | `definition.rs`: `max_turns: Option<u32>`. | ✅ | — |

---

## 6. Multi-Agent & Swarm

*Agent teams, coordination, inter-agent communication, swarm UI.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Agent teams** | Multi-agent coordination with task DAG. TeamView widget. | oh-my-openagent: Atlas/delegate-task orchestration. codebuff: 4-agent pipeline. CCB: swarm coordination. | `info_widget_swarm_background.rs`: TeamView widget. | ⚠️ | TeamView is informational only. No interactive team management. |
| **Swarm members** | Remote swarm member lifecycle. Status updates via `ServerEvent::SwarmStatus`. | CCB: swarm backends (InProcess, Tmux, Pane). | `remote_swarm_members: Vec<SwarmMemberStatus>`. `server_events.rs`: SwarmStatus handler. | ✅ | — |
| **Swarm plan** | Swarm plan synchronization. Plan proposals, coordinator mode. | CCB: `src/coordinator/coordinatorMode.ts`. | `swarm_plan_core.rs`, `ServerEvent::SwarmPlan`. | ✅ | — |
| **Inter-agent comm** | Agents communicate via mailboxes, shared context, notifications. | CCB: `src/utils/teammateMailbox.ts`, `src/utils/udsMessaging.ts`. | `ServerEvent::Notification`, `CommReadContext`, `CommContextHistory`. | ✅ | — |
| **Swarm info widget** | Show swarm member status in margin. Status icons, member names, roles. | CCB: teammate banner. | `info_widget_swarm_background.rs`: `render_swarm_widget()`. | ✅ | — |
| **Forked agents** | Fork agent with full context inheritance. In-process spawning. | CCB: `src/utils/forkedAgent.ts`, `src/utils/swarm/inProcessRunner.ts`. | Spawning via `spawnInProcess` via agent runtime. | ✅ | — |

---

## 7. Extensions & Plugins

*Plugin system, MCP, skills, hooks.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Plugin system** | Load plugins from disk. Plugin registry, lifecycle management. | CCB: `src/utils/plugins/pluginLoader.ts`, opencode: extension system. | Plugin system exists (`PluginTuiBridge` in status bar). | ⚠️ | Basic plugin bridge. Full plugin lifecycle (install/list/remove) missing. |
| **Plugin agents** | Load agent definitions from plugins. Plugin-defined agent files with scoped availability. | CCB: `src/utils/plugins/loadPluginAgents.ts`. | — | ❌ | Plugin loader integration with AgentRegistry. |
| **MCP servers** | Model Context Protocol server integration. | CCB: MCP client (`src/services/mcp/`). | MCP support via hashline/ffs tools. | ✅ | — |
| **Skills** | Bundled skills system. Load skills from directory. | CCB: `src/skills/`. oh-my-openagent: prompt variants per model. | Skills system in `.claude/skills/`. Skill loading, bundled skills. | ✅ | — |
| **Agent hooks** | Pre/post lifecycle hooks (onSpawn, onComplete, onError). Defined in agent file. | CCB: `src/utils/hooks/execAgentHook.ts`, `src/utils/hooks/registerFrontmatterHooks.ts`. | Hooks system exists (`HOOKS.md`, `SPAWN_HOOK.md`). | ✅ | — |

---

## 8. IDE Integration

*LSP, DAP, editor integration, ACP protocol.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **LSP operations** | 13 LSP operations: diagnostics, definition, references, hover, rename, code actions, symbols, etc. | oh-my-pi: 13 LSP ops. | `lsp` tool: full LSP integration. | ✅ | — |
| **DAP operations** | 27 DAP operations: launch, attach, breakpoints, step, evaluate, threads, etc. | oh-my-pi: 27 DAP ops. | `debug` tool: full DAP integration. | ✅ | — |
| **External editor** | Open file in external `$EDITOR`. Edit prompt in editor. | CCB: `$EDITOR` integration. | `edit_text_in_external_editor()` in `input.rs`. | ✅ | — |
| **ACP protocol** | Agent Communication Protocol for Zed/Cursor IDE bridge. | CCB: `src/services/acp/agent.ts`, `src/services/acp/bridge.ts`. | — | ❌ | Cross-IDE agent protocol. Separate feature. |

---

## 9. Deployment & Infrastructure

*Remote servers, CI/CD, release, installation.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Remote session** | Connect to remote jcode server. Subscribe to remote session events. | CCB: remote control, Docker deployment. | `backend.rs`: `RemoteConnection`. Full remote session support. | ✅ | — |
| **Release workflow** | GitHub Actions CI/CD, release automation, cross-platform builds, installer scripts. | oh-my-pi: CI/CD. pi-agent-rust: release pipeline. | `.github/workflows/ci.yml`, `release.yml`. `install.sh`, `install.ps1`. | ✅ | — |
| **Installation** | curl | sh installer, brew, cargo, binary releases. | CCB: multiple install paths. | `install.sh` (curl pipe). Binary releases on GitHub. | ✅ | — |
| **Auto-update** | Automatic update check. Notify user of new version. | CCB: auto-update. | `setup_hints.rs`: update hints. | ⚠️ | Manual update via re-install. No auto-update daemon. |

---

## 10. Configuration & UX

*Settings, themes, keybindings, onboarding, help.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Config file** | TOML-based config. Config reference documentation. | CCB: config system. | `config.toml`. `CONFIG_REFERENCE.md`. | ✅ | — |
| **Keybindings** | Customizable keybindings. Default binding set. | CCB: `src/keybindings/defaultBindings.ts`. | Keybinding system. Default bindings. | ✅ | — |
| **Theme** | Dark/light theme. ANSI color theme file. | CCB: `src/utils/theme.ts`. | Theme system. | ✅ | — |
| **Onboarding** | First-run setup wizard. Provider authorization flow. | CCB: onboarding. | Onboarding flow. | ✅ | — |
| **Doctor command** | `jcode doctor` diagnostics. Check agent files, config, providers. | CCB: doctor command. | Doctor command. | ✅ | — |
| **Help system** | `/help` command. Built-in help for commands. | CCB: help system. | Help command. | ✅ | — |

---

## 11. Performance & Optimizations

*Caching, streaming, benchmarks, prompt optimization.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Prompt cache** | Prompt caching for Anthropic models. Cache hit rate display. | CCB: prompt caching. | `prefix_cache_stable.rs`: prompt cache with adaptive strategy. Cache stats in status bar. | ✅ | — |
| **Streaming** | SSE streaming parser. Token-by-token rendering. | pi-agent-rust: SSE streaming parser with UTF-8 tail handling. | Streaming with token-by-token render. | ✅ | — |
| **Benchmark harness** | Edit benchmark suite. Task definitions, runner, scoring. | oh-my-pi: `typescript-edit-benchmark/`. pi-agent-rust: `benches/`. | `examples/bench_anthropic_essay_tps.rs`. | ⚠️ | Basic TPS benchmark. No full edit benchmark suite. |
| **Context optimization** | Compaction, micro-compact, time-based MC config, post-compact cleanup. | CCB: compaction system. | `compact.rs`: full compaction pipeline. | ✅ | — |
| **Memory management** | `/dream` command for memory consolidation. Auto-extraction. | CCB: `src/services/extractMemories/extractMemories.ts`. | Memory system. | ✅ | — |

---

## 12. Platform Support

*Cross-platform compatibility, terminal emulators, accessibility.*

| Name | Features | References | jcode Impl | Progress | Remaining |
|------|----------|------------|------------|----------|-----------|
| **Unix support** | macOS, Linux support. | All refs | macOS (native). Linux via binary. | ✅ | — |
| **Windows support** | Windows native support. PowerShell installer. | oh-my-pi: Windows support. | `install.ps1`. Windows binary. | ✅ | — |
| **Terminal emulators** | Kitty keyboard protocol. WezTerm, Ghostty, iTerm2 support. | CCB: terminal detection. | Kitty protocol via crossterm. WezTerm, Ghostty integration. | ✅ | — |
| **Desktop app** | Native GUI wrapper via wgpu/winit. Workspace mode (tiled sessions). | opencode: `packages/desktop/` (Electron-style). | `jcode-desktop`: wgpu/winit/ glyphon. Animated viewport transitions. | ✅ | — |

---

## Summary

### By Section

| # | Section | Features | ✅ Complete | ⚠️ Partial | ❌ Missing |
|---|---------|----------|-------------|-------------|-----------|
| 1 | Core Terminal UI | 9 | 8 | 1 | 0 |
| 2 | Agent Management | 16 | 9 | 2 | 5 |
| 3 | Model & Provider | 6 | 6 | 0 | 0 |
| 4 | Session & History | 5 | 5 | 0 | 0 |
| 5 | Tools & Permissions | 8 | 7 | 1 | 0 |
| 6 | Multi-Agent & Swarm | 6 | 5 | 1 | 0 |
| 7 | Extensions & Plugins | 5 | 3 | 1 | 1 |
| 8 | IDE Integration | 4 | 3 | 0 | 1 |
| 9 | Deployment & Infrastructure | 4 | 3 | 1 | 0 |
| 10 | Configuration & UX | 6 | 6 | 0 | 0 |
| 11 | Performance & Optimizations | 5 | 3 | 2 | 0 |
| 12 | Platform Support | 4 | 4 | 0 | 0 |
| | **Total** | **78** | **62 (79%)** | **9 (12%)** | **7 (9%)** |

### Missing Features (Priority Order)

| Priority | Feature | Section | Effort | Dependencies |
|----------|---------|---------|--------|-------------|
| P0 | `/tasks` command | 2 | Low | `background::global().running_snapshot()` already exists |
| P0 | `/agents save` | 2 | Low | Parse ```toml from last assistant message |
| P1 | AI generation auto-save | 2 | Medium | Hook into turn completion |
| P1 | Color picker UI | 2 | Medium | 8 color swatches + ratatui Span rendering |
| P1 | Agent edit menu | 2 | Medium | Model/tools/color inline pickers |
| P1 | Agent storage scopes | 2 | Low | Add managed (read-only) scope directory |
| P2 | Context window visualization | 1 | Medium | Per-agent token tracking + rendering |
| P2 | Sandbox/Isolation | 5 | High | Network/filesystem sandbox (codex-style) |
| P2 | Plugin agents | 7 | High | Plugin loader + AgentRegistry integration |
| P3 | ACP protocol | 8 | High | Cross-IDE agent protocol |
| P3 | Auto-update daemon | 9 | Medium | Background update checker |
| P3 | Edit benchmark suite | 11 | Medium | Full task catalog + runner + scoring |

### Steps to Add a New Feature

1. Pick the right section (1-12). If none fits, add a new section.
2. Add a row matching the table format above.
3. Fill: Name, Features, References, jcode Impl, Progress, Remaining.
4. Update the summary table counts.
