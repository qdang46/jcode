# Plan: Expand PARITY.md Coverage & Close Reference Repo Gaps

> Generated from research across jcode codebase + feature-planning reference repos
> Goal: Identify untracked features, prioritize additions to PARITY.md, and flag implementation gaps vs 9 reference repos

---

## Phase 1 — Add All Tools (33 untracked)

**Why:** Tools are what agents use every interaction. 42 tools in code, only 9 references in PARITY.md. This is the single biggest gap.

### Core File Tools (6)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 1 | **read** | `tool/read.rs` | CCB, codebuff | P0 |
| 2 | **edit** | `tool/edit.rs` | CCB, oh-my-pi | P0 |
| 3 | **write** | `tool/write.rs` | CCB, codebuff | P0 |
| 4 | **apply_patch** | `tool/apply_patch.rs` | CCB | P1 |
| 5 | **patch** | `tool/patch.rs` | oh-my-pi | P1 |
| 6 | **multiedit** | `tool/multiedit.rs` | oh-my-pi | P1 |

### Search & Code Understanding (5)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 7 | **codesearch** | `tool/codesearch.rs` | oh-my-openagent (ripgrep-cli) | P0 |
| 8 | **agentgrep** | `tool/agentgrep.rs` | — | P1 |
| 9 | **lsp** | `tool/lsp.rs` | oh-my-pi (13 LSP ops) | P1 |
| 10 | **websearch** | `tool/websearch.rs` | CCB | P1 |
| 11 | **webfetch** | `tool/webfetch.rs` | CCB | P1 |

### Shell & Execution (3)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 12 | **bash** | `tool/bash.rs` | CCB, codex (hardened) | P0 |
| 13 | **bg** | `tool/bg.rs` | CCB | P1 |
| 14 | **batch** | `tool/batch.rs` | — | P2 |

### Multi-Agent & Coordination (5)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 15 | **team** | `tool/team.rs` | oh-my-openagent (delegate-task) | P1 |
| 16 | **task** (SubagentTool) | `tool/task.rs` | CCB, codebuff | P0 |
| 17 | **communicate** | `tool/communicate.rs` | CCB (teammateMailbox) | P1 |
| 18 | **coordination** | `tool/coordination.rs` | oh-my-openagent | P1 |
| 19 | **todo** | `tool/todo.rs` | CCB | P1 |

### Memory & Knowledge (5)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 20 | **memory** | `tool/memory.rs` | CCB, pi-agent-rust | P0 |
| 21 | **conversation_search** | `tool/conversation_search.rs` | — | P1 |
| 22 | **session_search** | `tool/session_search.rs` | — | P1 |
| 23 | **notepad** | `tool/notepad.rs` | jcode native | P1 |
| 24 | **beads** | `tool/beads.rs` | — | P1 |

### Agent Lifecycle (5)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 25 | **goal** (InitiativeTool) | `tool/goal.rs` | CCB | P1 |
| 26 | **best_of_n** | `tool/best_of_n.rs` | oh-my-pi | P1 |
| 27 | **propose_edit** | `tool/propose_edit.rs` | codebuff | P1 |
| 28 | **propose_hashline_edit** | `tool/propose_hashline_edit.rs` | oh-my-pi (hashline_edit) | P2 |
| 29 | **propose_write** | `tool/propose_write.rs` | codebuff | P2 |

### Infrastructure (6)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 30 | **mcp** | `tool/mcp.rs` | — | P1 |
| 31 | **skill** | `tool/skill.rs` | jcode native | P1 |
| 32 | **side_panel** | `tool/side_panel.rs` | CCB | P2 |
| 33 | **dcp_compress** | `tool/dcp_compress.rs` | — | P2 |
| 34 | **debug_socket** | `tool/debug_socket.rs` | — | P2 |
| 35 | **browser** | `tool/browser.rs` | CCB (Chrome Use) | P1 |

### File Navigation (3)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 36 | **ls** | `tool/ls.rs` | CCB | P1 |
| 37 | **open** | `tool/open.rs` | CCB | P1 |
| 38 | **ambient** | `tool/ambient.rs` | — | P2 |

### Domain-Specific (4)

| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 39 | **gmail** | `tool/gmail.rs` | — | P2 |
| 40 | **hashline_edit** | `tool/hashline_edit.rs` | oh-my-pi | P1 |
| 41 | **invalid** | `tool/invalid.rs` | — | P2 |
| 42 | **task_management** | `tool/task_management.rs` | CCB | P2 |

### Hashline edit (special)
| # | Tool | File | Known From | Priority |
|---|------|------|------------|----------|
| 43 | **hashline_edit** | `tool/hashline_edit.rs` | oh-my-pi (hashline_edit) | P1 |

*(Note: `edit` and `hashline_edit` are separate — hashline uses hash-anchored editing)*

---

## Phase 2 — Add Missing Feature Domains

After tools, add the major subsystems as new PARITY.md sections.

### P0 — Add as sections immediately

| Domain | Crates | Why Now |
|--------|--------|---------|
| **VIII — Provider System** | `jcode-provider-*` (10 crates) | Every agent depends on providers. Model override is tracked but provider abstraction, failover, pricing, auth are not. |
| **IX — Plugin System** | `jcode-plugin-core/`, `jcode-plugin-runtime/` | Full plugin runtime with security sandbox, manifest, dispatcher, TUI host. Unique jcode advantage. |
| **X — Tools Registry** | All 42 tools | Each tool should be a row in a new Tools section, with its tool name, description, source, and status. |

### P1 — Add as sections

| Domain | Crates | Notes |
|--------|--------|-------|
| **XI — Desktop App** | `jcode-desktop/` | Rich text, IPC, animations, gallery, issue browser |
| **XII — Embedding/Memory Pipeline** | `jcode-embedding/`, `jcode-mempalace-adapter/`, `jcode-memory-types/` | Memory system is partially tracked in VI. Full pipeline includes ONNX model, embedding, memory palace adapter. |
| **XIII — Auth & Secrets** | `jcode-auth-types/`, `jcode-azure-auth/`, `jcode-keyring-store/`, `jcode-secrets/` | Provider auth, OAuth flows, OS keyring |
| **XIV — Config System** | `jcode-config-types/` | Schema, keybindings, model prefs, hooks config |

### P2 — Add as sections when convenient

| Domain | Crates | Notes |
|--------|--------|-------|
| **XV — TUI Framework** | 14 `jcode-tui-*` crates | Individual rendering crates. Low user impact if app-level TUI already tracked. |
| **XVI — Protocol** | `jcode-protocol/` | Wire protocol, message types. Low impact if session system covers it. |
| **XVII — Overnight** | `jcode-overnight-core/` | Background overnight processing |
| **XVIII — Mobile** | `jcode-mobile-core/`, `jcode-mobile-sim/` | Mobile agent runtime |
| **XIX — Build & Release** | `jcode-build-meta/`, `jcode-build-support/`, `jcode-selfdev-types/` | Dev infrastructure |

---

## Phase 3 — Reference Repo Feature Gaps

Features from reference repos that jcode doesn't have yet. Decision needed: implement or defer?

### High Impact

| Feature | Source Repo | Effort | jcode Counterpart |
|---------|-------------|--------|-------------------|
| **DAP (27 ops)** | oh-my-pi | Medium | jcode has LSP (9 ops). DAP would add debugger integration. |
| **Tree-sitter code map** | codebuff (10+ languages) | Medium | jcode uses tree-sitter only in edit-bench. A general code-map tool would improve code understanding. |
| **Prompt variants per model** | oh-my-openagent | Small | Same agent, different prompt per provider (Claude vs GPT vs Gemini). Could be added to agent definition. |

### Medium Impact

| Feature | Source Repo | Effort | Notes |
|---------|-------------|--------|-------|
| **Computer Use** | CCB | High | Screen capture + vision model. Complex infrastructure. |
| **Pipe IPC** | CCB | High | Cross-instance communication. Requires protocol extension. |
| **ACP Protocol** | CCB | High | Agent Communication Protocol for IDE integration (Zed/Cursor). |
| **Voice Mode** | CCB | High | Speech-to-text + text-to-speech. Whisper already partially wired in protocol tests. |
| **SSE streaming** | pi-agent-rust | Medium | Server-Sent Events for streaming responses. May already exist in protocol layer. |
| **Tmux integration** | oh-my-openagent | Small | Multi-pane agent workflows. `team.rs` already mentions tmux layout. |

### Low Impact / Defer

| Feature | Source Repo | Notes |
|---------|-------------|-------|
| **WASM extension runtime** | pi-agent-rust | jcode has native plugin system already. WASM adds security isolation but may not be needed. |
| **io_uring fast lane** | pi-agent-rust | Linux-specific. Not portable. |
| **Shadow dual execution** | pi-agent-rust | Complex. Runs two models and compares. |
| **Langfuse monitoring** | CCB | jcode has telemetry-core. Langfuse is a specific external platform. |
| **Sandbox execution** | codex | Already marked ❌ by design decision. |
| **IDE wiring** (VS Code) | oh-my-pi | jcode is terminal-first. Different philosophy. |
| **Remote Control** (Docker UI) | CCB | Docker self-hosted remote UI. Niche. |

---

## Execution Roadmap

```
Week 1: Phase 1 — Add 42 tools to PARITY.md (can batch by category)
         → New Tools section with 8-10 sub-sections
         → Review each tool's doc comment for accurate description

Week 2: Phase 2 P0 — Provider System + Plugin System sections
         → Provider: 10 crates, model selection, failover, auth, pricing
         → Plugin: manifest, security, dispatcher, TUI host, native

Week 3: Phase 2 P1 — Desktop + Embedding + Auth + Config sections
         → Desktop: rich text, IPC, animations
         → Embedding: ONNX model, memory pipeline

Week 4: Phase 3 — Evaluate high-impact gaps (DAP, code map, prompt variants)
         → Decide implement vs defer
         → Quick wins: prompt variants per model (< 1 day)
```

---

## Success Criteria

- [ ] All 42 tools listed in PARITY.md with description, source, status
- [ ] Provider system section with all 10 crates covered
- [ ] Plugin system section with capabilities and security model
- [ ] Desktop app features documented
- [ ] Gap analysis documented for each reference repo feature
- [ ] Summary table updated with correct totals
- [ ] Phase 3 features evaluated with implement/defer decision
