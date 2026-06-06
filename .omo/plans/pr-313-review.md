# PR #313 Review: jcode Multi-Agent Foundation vs 9 Reference Repos

> **Date**: 2026-06-05
> **Reviewer**: Claude Opus 4.8 (feature-planning skill)
> **PR**: #313 — `experimental/multi-agent-foundation` → `master`
> **Scope**: +5775 / -94 lines, 28 files, 7 commits

---

## 1. Per-Dimension Comparison Tables

### 1A. Agent Definition Schema

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Format** | TOML | TS imperative + `handleSteps` | N/A (TUI) | Markdown + YAML frontmatter | Markdown + YAML | TS imperative | Markdown + YAML | Markdown + YAML | Rust runtime | N/A |
| **Schema validation** | `serde(deny_unknown_fields)` | Zod runtime | TS types | Zod (lazy) | Effect `Schema.Class` | TS types | YAML parse | YAML parse | serde derive | N/A |
| **`model` field** | optional (`model_override` + `prefer_tier`) | **required** | N/A | optional (`inherit`) | optional | **required** | optional | optional | N/A | env var stack |
| **`reasoning`/`effort`** | `ReasoningEffort` enum (4 levels) | `reasoningOptions.effort` (5 levels) + `max_tokens` | N/A | `effort` enum + integer | `variant` per-model | `Effort` enum | `ModelV2.VariantID` | N/A | N/A | N/A |
| **`outputMode`** | `last_message`/`all_messages`/`structured_output` | identical | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| **`tool_names`** | whitelist (deny-by-default) | whitelist + MCP servers | built-in list | `tools` + `disallowedTools` | optional from registry | `loadMode` + `tier` | tool registry | tool allowlist | optional | N/A |
| **`spawnable_agents`** | whitelist | `publisher/agent@version` | N/A | N/A (model drives) | N/A | N/A | N/A | N/A | N/A | N/A |
| **`inherit_parent_system_prompt`** | ✅ | ✅ | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| **`include_message_history`** | ✅ | ✅ | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| **`handleSteps`** | N/A (Phase 2) | ✅ Generator | N/A | N/A | `steps: PositiveInt` | N/A | N/A | N/A | N/A | N/A |
| **`permissionMode`** | N/A | N/A | N/A | ✅ per-agent | ✅ per-agent | `ToolTier` per-tool | N/A | N/A | N/A | N/A |
| **`maxTurns`** | N/A | N/A | N/A | ✅ per-agent | `steps: PositiveInt` | N/A | N/A | N/A | N/A | N/A |
| **`isolation`** | N/A | N/A | N/A | `worktree`/`remote` | N/A | N/A | N/A | `worktree` (git) | N/A | N/A |
| **`mcpServers`** | N/A | ✅ per-agent | N/A | ✅ per-agent | N/A | N/A | N/A | ✅ MCP server | N/A | N/A |
| **`hooks`** | N/A | N/A | N/A | ✅ per-agent | N/A | N/A | N/A | N/A | N/A | N/A |
| **`memory` scope** | N/A | N/A | N/A | `user`/`project`/`local` | N/A | N/A | N/A | N/A | N/A | N/A |

---

### 1B. Agent Registry / Discovery

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Discovery paths** | 3-tier: project > user > builtin | `.agents/` local | N/A | `.claude/agents/*.md` + settings | `.opencode/agents/*.md` + `modes/` | N/A | N/A | N/A | N/A | N/A |
| **Priority order** | project > user > builtin | built-in first | N/A | built-in first | primary source glob | N/A | N/A | N/A | N/A | N/A |
| **Filename == id check** | ✅ enforced | ❌ | N/A | ❌ | ❌ | N/A | N/A | N/A | N/A | N/A |
| **Non-fatal errors** | ✅ collected for `doctor` | throws | N/A | log + skip | throws | N/A | N/A | N/A | N/A | N/A |
| **On-disk format** | TOML | TS | N/A | Markdown | Markdown | N/A | N/A | N/A | N/A | N/A |
| **Reload at runtime** | not yet | no | N/A | cache + plugin invalidation | `update` API | N/A | N/A | N/A | N/A | N/A |

---

### 1C. Model Routing / Tier

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Approach** | env-var slots + session inherit | OpenRouter catalog | `JCODE_ROUTING_*` env vars | `inherit` | `ModelV2.parse` | dynamic `ModelV2` | `ModelResolutionPipeline` (5 stages) | via Claude session | direct | env var stack |
| **Slot/tier concept** | `Routine`/`Thinking` | no (literal model id) | `ROUTINE`+`THINKING`+`THRESHOLD` | no | variant per-provider | model string | catalog aliases | no | no | default + fallback |
| **Fallback chain** | 3-level: override > env > session | OpenRouter routing | N/A | N/A | provider fallback | `resolveModelWithFallback` | 5-stage pipeline | N/A | per-provider | 2-tier fallback |
| **Predefined catalog** | **no** (intentional) | yes (100+ models) | no | no | yes (`models-dev.ts`) | no | yes (60+ models) | no | no | no |
| **Provider abstraction** | no (single OAuth) | OpenRouter | multi-provider | Anthropic | multi-provider | 40+ providers | multi-provider | Anthropic | 15+ providers | Codex only |

---

### 1D. Agent Lifecycle / Spawn

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Agent tree** | N/A | N/A | ✅ `AgentPath` + `ThreadSpawnEdgeStatus` | `team_name` (1:1 TaskList) | `mode: subagent/primary/all` | runtime | `boulder-state` (worktrees) | `team jobs` | session tree | N/A |
| **Spawn tool** | N/A (schema only) | `spawn_agents` | `SpawnAgent`/`WaitAgent`/`CloseAgent`/`SendMessage`/`AssignAgentTask` | `Agent` tool + `TeamCreate` | delegation via tools | N/A | `delegate_task` | `omc_team_start` CLI | N/A | N/A |
| **Message bus** | N/A | output return | `InterAgentCommunication` + delivery edges | `SendMessage` tool | N/A | N/A | `shared-state.ts` | `omc-team-state.ts` | N/A | N/A |
| **Parallel execution** | N/A | `Promise.all` | DAG traversal | concurrent teammates | concurrent | DAG wave | sequential | sequential | N/A | N/A |
| **Worktree isolation** | N/A | N/A | N/A | ✅ `isolation: worktree/remote` | N/A | N/A | N/A | ✅ git worktree cleanup | N/A | N/A |
| **`maxTurns`** | N/A | N/A | N/A | ✅ per-agent | `steps: PositiveInt` | N/A | N/A | N/A | N/A | N/A |
| **Job persistence** | N/A | N/A | ✅ SQLite `agent_jobs` | team config JSON | N/A | N/A | `boulder-state` file | `OMC_JOBS_DIR` artifacts | session JSONL | N/A |

---

### 1E. Permission / Safety

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Permission system** | **existing** `SafetySystem` + `ActionTier` | none | sandbox | `PermissionMode` per-agent (default/auto/ask/deny) | `PermissionV2.Ruleset` (allow/deny/ask) per-agent | `ToolTier` (read/write/exec) + approval modes | MCP allowlist | plugin/team scopes | none | `OMX_*` env controls |
| **Per-agent policy** | **gap** — tool whitelist only | tool whitelist | N/A | ✅ `permissionMode` field | ✅ `permissions` array | ✅ `tier` on each tool | N/A | N/A | N/A | N/A |
| **Classification levels** | 2 (auto/permission) | N/A | N/A | 4 (default/auto/ask/deny) | 3 (allow/deny/ask) | 3 (read/write/exec) | N/A | N/A | N/A | N/A |
| **Auto-approve for sub-agents** | **not wired** | via `handleSteps` | N/A | via `permissionMode` | N/A | tool-tier-based | N/A | N/A | N/A | N/A |
| **TUI permission flow** | ✅ `PermissionsApp` (existing) | none | none | none (CLI only) | N/A | N/A | N/A | N/A | N/A | N/A |
| **`disallowedTools`** | N/A | N/A | N/A | ✅ | N/A | `hidden` field | N/A | N/A | N/A | N/A |

---

### 1F. Tool Execution

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Tool registry** | whitelist strings in TOML | typed `ToolName` union | hard-coded | `getTools()` config | `ToolsProvider` | `AgentTool<TParams>` interface | tool discovery | MCP servers | typed `Tool` trait | sparkshell bridge |
| **Concurrency control** | N/A | N/A | N/A | N/A | N/A | ✅ `shared`/`exclusive` | N/A | N/A | N/A | N/A |
| **`loadMode`** | N/A | N/A | N/A | N/A | N/A | ✅ `essential`/`discoverable` | N/A | N/A | N/A | N/A |
| **`deferrable`** | N/A | ✅ | N/A | N/A | N/A | ✅ | N/A | N/A | N/A | N/A |
| **`nonAbortable`** | N/A | N/A | N/A | N/A | N/A | ✅ | N/A | N/A | N/A | N/A |
| **Validation** | runtime (registry) | Zod args | sandbox | Zod | Effect Schema | Zod (`zodToWireSchema`) | Zod | Zod | typed Rust | typed Rust |
| **`beforeToolCall` hook** | N/A | N/A | N/A | N/A | N/A | ✅ (block/transform) | N/A | N/A | N/A | N/A |
| **`afterToolCall` hook** | N/A | N/A | N/A | N/A | N/A | ✅ (override) | N/A | N/A | N/A | N/A |
| **Structured output** | ✅ `OutputMode::StructuredOutput` | ✅ `set_output` + `outputSchema` | N/A | N/A | N/A | `set_output` | N/A | N/A | N/A | N/A |

---

### 1G. Eval / Benchmark

| Aspect | **jcode PR #313** | codebuff (BuffBench) | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|---------------------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Approach** | git-commit reconstruction (scaffold) | git-commit reconstruction (production) | e2e + bench scripts | N/A | N/A | LSP+DAP benchmarks | smoke tests | integration tests | N/A | sparkshell benchmark |
| **Multi-judge** | ✅ 3 judges + per-model timeout | 2 judges (20 min shared) | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| **Median scoring** | ✅ | ✅ | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| **Lessons extractor** | ✅ scaffold | ✅ production | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| **`meta-analyze`** | ✅ implemented | ✅ | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| **Feature flag** | ✅ `agent-runner` gate | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |

---

### 1H. Prompt Utilities

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Placeholder substitution** | ✅ `prompt_placeholders.rs` (pure utility) | `PLACEHOLDER` constants | N/A | prompt templates | mode prompts | atlas prompts | `prompts-core` package | `atlas-prompts.ts` | N/A | `build_summary_prompt()` |
| **Supported tokens** | 7 tokens with length caps | `PLACEHOLDER` enum | N/A | env vars + dynamic | template engine | context-based | variant resolver | markdown | N/A | shell output |
| **Length caps** | ✅ 2500/10k/30k/100k chars | `FILE_TREE_PROMPT` only | N/A | N/A | N/A | provider-specific | model caps | N/A | N/A | N/A |
| **System reminder wrap** | ✅ `wrap_as_system_reminder()` | `<system_reminder>` tags | N/A | injection | N/A | N/A | prompt-injection.ts | prompt-injection.ts | N/A | N/A |
| **Frontmatter parse** | N/A (TOML) | N/A | N/A | ✅ `parseAgentToolsFromFrontmatter` | ✅ `ConfigMarkdown.parseOption` | N/A | `shared/frontmatter.ts` | N/A | N/A | N/A |

---

### 1I. Session / Persistence

| Aspect | **jcode PR #313** | codebuff | codex | claude-code | opencode | oh-my-pi | oh-my-openagent | oh-my-claudecode | pi-agent-rust | oh-my-codex |
|--------|-------------------|----------|-------|-------------|----------|----------|-----------------|------------------|---------------|-------------|
| **Session format** | N/A (existing) | in-memory | SQLite + JSONL | config JSON | SQLite (Effect) | runtime state | `boulder-state` file | `OMC_JOBS_DIR` JSON | **JSONL + SHA-256 chain** | N/A |
| **Branching/history** | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | ✅ tree structure | N/A |
| **Indexed search** | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | ✅ `SessionIndex` | N/A |
| **Chain integrity** | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | ✅ SHA-256 per-entry | N/A |

---

## 2. Top 5 Gaps (ROI-ranked)

| Rank | Gap | Effort | Impact | Source repos | Concrete action |
|------|-----|--------|--------|--------------|-----------------|
| **1** | `permissionMode` per-agent — wire `SafetySystem` into `AgentDefinition` | 2-3 days | 🔴 Critical (security) | claude-code (`PermissionMode`), opencode (`allow/deny/ask` per action+resource) | ✅ DONE (commit f84cc127 + 795242b6) — `permission_mode` enum + field added, dcg_bridge wired |
| **2** | `Agent` tool — model-driven spawn | 1-2 weeks | 🔴 Critical (core feature) | codex (`SpawnAgent`/`WaitAgent`), claude-code (`AgentTool` + `TeamCreateTool`), codebuff (`spawn_agents`) | Phase 2: add `agent` tool that LLM calls; wire `spawnable_agents` whitelist; implement `AgentPath` tree from codex |
| **3** | `maxTurns` per-agent | 1 day | 🟡 Important (runaway prevention) | claude-code, opencode | ✅ DONE (commit 844fc412) — `max_turns` field added to `AgentDefinition` |
| **4** | `handleSteps` — programmatic agents | 1 week | 🟡 Important (flexibility) | codebuff (`handleSteps` Generator), oh-my-pi (`beforeToolCall`/`afterToolCall`) | Phase 2: add optional `handle_steps` field with Rust async generator or callback approach |
| **5** | Tool concurrency (`shared`/`exclusive`) | 2-3 days | 🟢 Nice-to-have (perf) | oh-my-pi (`AgentTool.concurrency`) | Add `concurrency` field to tool definition; runtime scheduler respects exclusive locks |

---

## 3. Wire-up Plan: SafetySystem + AgentDefinition.permissionMode

### Current state
- `SafetySystem` (crates/jcode-base/src/safety.rs): `ActionTier` = `AutoAllowed | RequiresPermission`
- `AgentDefinition` (crates/jcode-agent-runtime/src/definition.rs): `tool_names` whitelist only
- `PermissionsApp` (crates/jcode-tui/src/tui/permissions.rs): TUI approval flow exists

### Proposed addition

```rust
// crates/jcode-agent-runtime/src/definition.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Inherit approval from parent agent (default for sub-agents).
    Inherit,
    /// Auto-approve all tool calls for this agent.
    AutoApprove,
    /// Always ask user for permission.
    Ask,
    /// Deny all tool calls (read-only agent).
    Deny,
}

impl Default for PermissionMode {
    fn default() -> Self { PermissionMode::Inherit }
}

// Add to AgentDefinition:
// pub permission_mode: Option<PermissionMode>,
```

### Resolution algorithm (runtime)

```
fn resolve_permission(action, tool_name, agent_def, parent_approval):
    mode = agent_def.permission_mode.unwrap_or(Inherit)
    match mode:
        Deny → block
        AutoApprove → approve
        Ask → prompt user via PermissionsApp
        Inherit → use parent_approval (or session-level classify)
```

### Migration path
- Default `None` = `Inherit` = existing behavior unchanged
- TOML agents opt-in: `permission_mode = "auto_approve"` for leaf agents
- Phase 2: auto-wire `bash` tool in `basher.toml` with `permission_mode = "auto_approve"`

---

## 4. Roadmap: Phases After PR #313

| Phase | Scope | Dependencies | Estimated |
|-------|-------|--------------|-----------|
| **Phase 1** (this PR) | AgentDefinition + tier + registry + JBench scaffold | — | ✅ Done |
| **Phase 1.5** | `permissionMode` wire-up (SafetySystem + AgentDefinition) | Phase 1 | ✅ Done |
| **Phase 2** | Agent runtime engine: spawn, parent-child tree, `Agent` tool, `AgentPath` | Phase 1 | 2-3 weeks |
| **Phase 2.5** | `handleSteps` (programmatic agents), tool concurrency | Phase 2 | 1-2 weeks |
| **Phase 3** | Team pipeline (claude-code-style `TeamCreateTool`) | Phase 2 | 1 week |
| **Phase 4** | JBench production (full `pick-commits` → `gen-evals` → `run` → `judge` → `lessons` pipeline) | Phase 2 | 1-2 weeks |
| **Phase 5** | Multi-provider support (extend tier to per-provider catalogs) | Phase 2 | 1 week |

---

## 5. PR #313 Strengths

1. **Best-in-class agent discovery** — 3-tier priority, filename==id enforcement, non-fatal error collection
2. **Correct model routing philosophy** — slots not catalog, matches single-OAuth reality
3. **JBench exceeds BuffBench** — 3 judges with per-model timeout (vs BuffBench's shared 20-min timeout)
4. **Rust-idiomatic crate structure** — feature gates, clean separation, `serde(deny_unknown_fields)`
5. **Comprehensive documentation** — every module has a doc comment explaining WHY, not just WHAT

---

## 6. PR #313 Actionable Issues

| # | Issue | Severity | File | Fix |
|---|-------|----------|------|-----|
| 1 | `extract_diff_from_repo` uses sync `std::process::Command` in async fn | Medium | evals/jbench/src/agent_runner.rs:195 | ✅ FIXED (commit 2d7a020c) |
| 2 | `todo_step` calls `std::process::exit(0)` for unimplemented commands | Low | evals/jbench/src/bin/jbench.rs | ✅ FIXED (commit 2d7a020c) |
| 3 | `file-picker.toml` missing explicit `inherit_parent_system_prompt = false` | Low | .jcode/agents/file-picker.toml | Add for consistency with `basher.toml` |
| 4 | `edition = "2024"` in jbench may cause toolchain issues if workspace uses 2021 | Low | evals/jbench/Cargo.toml | Verify workspace edition consistency |
| 5 | `meta_analyze_impl` reads all `.run.json` files into memory | Low | evals/jbench/src/bin/jbench.rs:268 | Streaming deserializer for large runs |

---

## 7. Implementation Status (2026-06-05)

| Item | Status | Commit |
|------|--------|--------|
| Merge master into branch | ✅ Done | 25d3f21e |
| Reconcile src/lib.rs with master | ✅ Done | 60a61f0b |
| Review document (9 repos) | ✅ Done | d2942498 |
| `permissionMode` enum + field | ✅ Done | f84cc127 |
| `permissionMode` wire-up (dcg_bridge) | ✅ Done | 795242b6 |
| `maxTurns` field | ✅ Done | 844fc412 |
| TOML agents max_turns | ✅ Done | 6d8ecbc6 |
| Fix jbench warnings | ✅ Done | 2d7a020c |
| `Agent` tool (model-driven spawn) | 🔲 Phase 2 | — |
| `handleSteps` (programmatic agents) | 🔲 Phase 2 | — |
| Tool concurrency (shared/exclusive) | 🔲 Phase 2 | — |
| Team pipeline (TeamCreateTool) | 🔲 Phase 3 | — |
| JBench production | 🔲 Phase 4 | — |
