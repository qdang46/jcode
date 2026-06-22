# Implementation Plan: Todo System Hardening cho jcode
> Generated từ research 8 reference repos + best-of-breed selection (skill: feature-planning)
> Goal: Bảo toàn todo state qua compaction, có UI feedback cho user, structural safety để model không skip verification, và reminder loop chống model quên update.

---

## 1. Executive Summary

Jcode hiện có `TodoTool` lưu file JSON `~/.jcode/todos/<session>.json` — đúng pattern upstream, nhưng có 4 gap lớn:
1. Model context mất todo khi compact dù file vẫn còn
2. Không có UI hiển thị
3. Không có safety check khi model close list mà chưa verify
4. Không có reminder khi model drift

Plan này thêm **4 modules mới + 2 patches**:
- Compaction-survival hook (lấy từ claude-code extractTodosFromTranscript)
- TodoUpdated event (lấy từ opencode + claude-code v2 signal)
- Sticky TUI panel (lấy từ oh-my-pi selectStickyTodoWindow)
- Verification nudge (lấy từ claude-code v1 verificationNudgeNeeded)
- Reminder loop (lấy từ oh-my-pi, tích hợp với jcode ambient/prompt system đã có)
- Patch: schema update — thêm activeForm (claude-code), giữ nguyên status enum 4

---

## 2. Files to change

### Modified (7)
- crates/jcode-base/src/todo.rs (+50 LOC) — add save_todos return bool (nudge), add needs_verification_nudge, add extract helper
- crates/jcode-base/src/bus.rs (+10 LOC) — add TodoUpdated variant + struct
- crates/jcode-base/Cargo.toml (+5 LOC) — criterion dev-dep for benchmarks
- crates/jcode-task-types/src/lib.rs (+5 LOC) — add activeForm field to TodoItem
- crates/jcode-app-core/src/tool/todo.rs (+15 LOC) — surface nudge text in tool output
- crates/jcode-app-core/src/compaction.rs (+5 LOC) — call restore_todos_after_compaction after compaction
- crates/jcode-tui/src/tui/app/mod.rs (+10 LOC) — subscribe BusEvent::TodoUpdated

### New (3 + 1 bench)
- crates/jcode-app-core/src/server/compaction_hooks.rs (~150 LOC) — extract_todos_from_transcript
- crates/jcode-tui/src/tui/components/todo_panel.rs (~120 LOC) — sticky TUI panel
- crates/jcode-tui/src/tui/app/todo_reminder.rs (~120 LOC) — reminder loop
- crates/jcode-base/benches/todo_bench.rs (~50 LOC) — criterion benchmarks

**Total: ~545 LOC across 12 files. Zero breaking changes.**

---

## 3. Architecture Decisions

| Component | Best Source | Why |
|-----------|-------------|-----|
| Compaction-survival | claude-code v1 extractTodosFromTranscript | Scan message log ngược. Zero state, fail-safe. Đơn giản hơn openagent hook. |
| Event signaling | opencode Event.Updated + claude-code v2 onTasksUpdated | Jcode đã có crate::bus::Bus. Thêm TodoUpdated. |
| Sticky panel | oh-my-pi selectStickyTodoWindow(tasks, maxVisible=5) | Slice open tasks + +N more hint. |
| Verification nudge | claude-code v1 verificationNudgeNeeded | Inject reminder khi close 3+ tasks không có /verif/i. |
| Reminder loop | oh-my-pi todo-reminder.ts | Detect many tool calls without todo update. |
| Schema | claude-code v1 + jcode existing | Add activeForm, keep priority/id/group/confidence/blocked_by/assigned_to. |

### Rejected alternatives
- Snapshot in-memory + restore hook (openagent) — rejected: requires plugin infra, bootstrap detection fragile
- Storage in tool result details (pi_agent_rust/oh-my-pi) — rejected: jcode already has file JSON
- SQLite per-session (opencode) — rejected: overkill
- Per-task file + high water mark + lock (claude-code v2) — rejected: complex for single-agent
- 3-status enum — rejected: breaks existing callers
- Per-task TaskUpdate ops — rejected: migration cost

---

## 4. Data Structures

```rust
// crates/jcode-task-types/src/lib.rs
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,  // NEW: present continuous for spinner
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_confidence: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoUpdateEvent {
    pub session_id: String,
    pub todos: Vec<TodoItem>,
    pub at: DateTime<Utc>,
}
```

```rust
// crates/jcode-base/src/bus.rs — additions
pub enum BusEvent {
    // ... existing variants ...
    TodoUpdated(TodoUpdated),
}

#[derive(Debug, Clone)]
pub struct TodoUpdated {
    pub session_id: String,
    pub todos: Vec<TodoItem>,
    pub at: chrono::DateTime<chrono::Utc>,
}
```

---

## 5. Verification nudge helper

```rust
// crates/jcode-base/src/todo.rs
pub fn needs_verification_nudge(previous: &[TodoItem], updated: &[TodoItem]) -> bool {
    let was_completed: HashSet<&str> = previous
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .map(|t| t.content.as_str())
        .collect();
    let newly_completed: Vec<&TodoItem> = updated
        .iter()
        .filter(|t| {
            t.status == TodoStatus::Completed && !was_completed.contains(t.content.as_str())
        })
        .collect();
    if newly_completed.len() < 3 { return false; }
    !newly_completed.iter().any(|t| {
        t.content.to_ascii_lowercase().contains("verif")
            || t.active_form.as_deref()
                .map(|s| s.to_ascii_lowercase().contains("verif"))
                .unwrap_or(false)
    })
}
```

---

## 6. Compaction-survival algorithm

Source: claude-code v1 extractTodosFromTranscript

```
FUNCTION extractTodosFromTranscript(messages: List[Message]) -> Vec<TodoItem>:
    FOR i = len(messages) - 1 DOWNTO 0:
        msg = messages[i]
        IF msg.role != "assistant": CONTINUE
        FOR block IN msg.content:
            IF block.type == "tool_use" AND block.name == "todo":
                INPUT = block.input
                IF INPUT has "todos":
                    RETURN normalize_and_parse(INPUT.todos)
    RETURN empty

normalize_and_parse: handle JSON-as-string quirk, skip malformed items.
```

---

## 7. Sticky panel algorithm

Source: oh-my-pi selectStickyTodoWindow + claude-code v1 all-done-clear

```
FUNCTION select_sticky_window(todos: Vec<TodoItem>) -> PanelData:
    open = FILTER(todos, status IN [pending, in_progress])
    IF len(open) > 0:
        visible = open[:MAX_VISIBLE]  # MAX_VISIBLE = 5
        hidden = len(open) - len(visible)
        RETURN PanelData { visible, hidden_open_count: hidden, mode: Active }
    ELSE:
        completed = FILTER(todos, status == completed)
        visible = completed[-MAX_VISIBLE:]  # last 5 completed as context
        RETURN PanelData { visible, hidden_open_count: 0, mode: AllCompletedClear }
```

---

## 8. Reminder loop algorithm

Source: oh-my-pi todo-reminder.ts

```
FUNCTION should_remind(state, todos) -> bool:
    IF todos is empty: RETURN false
    open_count = FILTER(todos, status IN [pending, in_progress]).len()
    IF open_count == 0: RETURN false
    IF state.reminded_at + 60s > now: RETURN false   # cooldown
    calls_since = state.tool_calls_count - state.tool_calls_at_last_update
    time_since = now - state.last_todo_update
    RETURN calls_since >= 5 OR time_since >= 10min
```

---

## 9. Configuration

```toml
# ~/.jcode/config.toml
[todo]
reminder_enabled = true                   # default true
reminder_tool_call_threshold = 5          # default
reminder_time_threshold_minutes = 10      # default
```

---

## 10. Wiring points

### Compaction hook
In crates/jcode-app-core/src/compaction.rs (existing), after do_compaction():
  restore_todos_after_compaction(session_id, &new_messages)

### Bus subscription
In crates/jcode-tui/src/tui/app/mod.rs, existing subscribe block:
  Bus::global().subscribe(|event| { if TodoUpdated => app.set_todos(todos); })

### Tool registration
tool/mod.rs — no change (TodoTool already registered)

---

## 11. Test cases

### Happy Path
- todo_save_load_roundtrip
- verification_nudge_triggers_at_3 (3 newly completed, no verif)
- verification_nudge_skipped_when_verif_present (3 completed, 1 contains "verif")
- verification_nudge_counts_only_newly_completed (already-completed don't count)
- select_sticky_window_active_mode
- select_sticky_window_all_done
- select_sticky_window_hidden_count_truncates

### Edge Cases
- transcript_scan_ignores_user_messages
- transcript_scan_ignores_tool_results
- sticky_panel_empty_list
- sticky_panel_with_only_blocked
- reminder_respects_cooldown (60s)
- reminder_no_open_tasks
- save_with_malformed_existing_file_recovers
- concurrent_save_uses_atomic_write

### Integration
- compaction_restores_todos (full integration)
- save_publishes_bus_event
- tool_execute_includes_nudge_text

---

## 12. Benchmarks (criterion)

| Metric | Target |
|--------|--------|
| extract_todos_from_transcript p50 (1000 msgs) | < 5ms |
| extract_todos_from_transcript p99 (10000 msgs) | < 50ms |
| save_todos (write + bus publish) | < 10ms |
| select_sticky_window (100 todos) | < 100µs |
| render_panel (5 todos, 80 cols) | < 50µs |
| Reminder check overhead per tool call | < 1µs |

---

## 13. Migration

Zero migration needed. Plan only:
- Adds new fields (activeForm, TodoUpdated event variant) — backward-compatible serde
- Adds new files — no edit to existing public API
- Patches tool/todo.rs execute() to include nudge text — additive
- Patches compaction.rs to call hook — fail-safe (tracing::warn on error)

Optional feature flag: [todo] reminder_enabled = false in config.toml to disable reminder.

---

## 14. Known Limitations & Future Work

- Per-task update tool (claude-code v2 style) — not in scope
- Dependency graph UI for blocked_by[] — schema ready, UI later
- Task high-water-mark + lock for swarm — not needed for single-agent
- Multi-phase (oh-my-pi) — not in scope
- Verification agent integration (sub-agent verifier) — future
- Transcript scan caching for >10k messages — future

---

## 15. Repo References

| Feature | Repo | File | Link |
|---------|------|------|------|
| Compaction-survival | claude-code | src/utils/sessionRestore.ts:extractTodosFromTranscript | https://github.com/claude-code-best/claude-code/blob/main/src/utils/sessionRestore.ts |
| Verification nudge | claude-code | TodoWriteTool.ts:call | https://github.com/claude-code-best/claude-code/blob/main/packages/builtin-tools/src/tools/TodoWriteTool/TodoWriteTool.ts |
| Schema + active_form | claude-code | src/utils/todo/types.ts | https://github.com/claude-code-best/claude-code/blob/main/src/utils/todo/types.ts |
| Event publishing | opencode | session/todo.ts:update | https://github.com/anomalyco/opencode/blob/main/packages/opencode/src/session/todo.ts |
| Sticky panel | oh-my-pi | tools/todo.ts:selectStickyTodoWindow | https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/tools/todo.ts |
| Reminder loop | oh-my-pi | todo-reminder.ts | https://github.com/can1357/oh-my-pi/blob/main/packages/coding-agent/src/modes/components/todo-reminder.ts |
| All-done clear | claude-code | TodoWriteTool.ts:call (allDone ? [] : todos) | (same as verification nudge link) |
| Compaction hook (alt) | openagent | compaction-todo-preserver/hook.ts | https://github.com/code-yeongyu/oh-my-openagent/blob/main/packages/omo-opencode/src/hooks/compaction-todo-preserver/hook.ts |

---

## 16. Success Criteria

- [ ] cargo check sạch
- [ ] All test cases in section 11 pass
- [ ] Benchmark p50 extract_todos_from_transcript (1000 msgs) < 5ms
- [ ] Manual: compact context preserves todos
- [ ] Manual: 3-task close without verif triggers NOTE reminder
- [ ] Manual: 3-task close with verif does NOT trigger reminder
- [ ] Sticky panel renders correctly with 0/3/5/10 todos
- [ ] Reminder fires after 5 tool calls without todo update
- [ ] Reminder respects 60s cooldown
- [ ] Bus event TodoUpdated published correctly
- [ ] No regression: existing goal.rs and todo.rs tests pass
