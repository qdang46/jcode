# Agent Foundation Audit — jcode

## What's already there

### Agent definitions (5 files in `.jcode/agents/`)
- `planner.toml` — Codebuff planner, prefer_tier=thinking, permission_mode=plan
- `file-picker.toml` — Codebuff Fletcher, prefer_tier=routine, tools=[ls,glob,read]
- `editor.toml` — Codebuff editor, hashline_edit-based
- `code-reviewer.toml` — Codebuff reviewer
- `basher.toml` — shell commands, safe-mode

### Loading infrastructure (already wired)
- `jcode-agent-runtime::AgentRegistry` — loads from `.jcode/agents/*.toml`
  - Resolution order: project-local > user-global > builtin
  - Used by `openers.rs:128-137` for picker
- `jcode-keywords::workflow::spawn_agent()` — STUB (returns placeholder text)
  - Comment: "Stub implementation — real wiring happens in app-core"

### Manual invocation paths (working)
- `/agents` slash command → picker
- `SubagentTool` (in `jcode-app-core/src/tool/task.rs`) — agent tool for LLM
- `communicate` tool — for agent-to-agent coordination
- Inline openers — picker UI to launch agents

## What's MISSING

### Orchestrator pipeline (the gap)
- **planner → file-picker → editor → code-reviewer → basher** chain
- This is the Codebuff-style "decomposed implementation" flow
- Currently NO automatic pipeline that runs these in sequence
- The 5 agents exist as definitions but are not auto-driven by todo state

### Todo → Orchestrator integration
- Todo system has `incomplete_poke_todos()` + `auto_poke_incomplete_todos: bool`
- `schedule_auto_poke_followup_if_needed()` re-queues the SAME prompt
- It does NOT spawn agents to work through the todo list

## Architectural gap

The user added Codebuff's 5-agent pipeline to replace auto-implement behavior,
but the orchestrator (the thing that runs the pipeline) is not yet wired.
The pipeline is defined but not driven.

## What needs to happen

1. **Orchestrator module**: takes a TodoItem or task description
2. **Sequential spawn chain**:
   - planner: produces plan
   - file-picker: finds files
   - editor: applies edits
   - code-reviewer: reviews
   - basher: runs tests
3. **BusEvent integration**: emit TodoUpdated on each step completion
4. **Hook into `schedule_auto_poke_followup_if_needed`**: when auto-poke fires,
   trigger the orchestrator instead of just re-queuing the raw prompt
5. **Error handling**: if any step fails, leave todo as in_progress with error

## Estimated scope
- New module: `crates/jcode-app-core/src/agent/orchestrator.rs` (~300 LOC)
- Wire into: turn.rs (auto-poke path)
- Tests: 5-10 unit tests for state machine
