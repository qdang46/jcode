# Implementation Plan: Complete Codebuff-Style Orchestrator Pipeline

> Generated from research + gap audit against jcode current state
> Goal: Close remaining 60% gap between current jcode orchestrator and full Codebuff pipeline.

## 1. Executive Summary

Jcode hiện tại có 2 implementation song song của cùng một pattern:
- `crates/jcode-app-core/src/server/swarm.rs` — **Codebuff pipeline đầy đủ** (planner → parallel sub-agents → coordinator integration), nhưng **KHÔNG được wire với todo system**.
- `crates/jcode-app-core/src/agent/orchestrator.rs` (vừa commit) — **stub**: 1 todo = 1 sub-agent, không có planner decomposition, không chain.

Plan này:
1. **Refactor orchestrator.rs** để gọi `swarm.rs` thay vì duplicate spawn logic
2. **Wire swarm.rs vào todo pipeline** thông qua `Agent::poll_todo_pipeline`
3. **Add feedback loop** (editor → reviewer fail → retry)
4. **Wire `spawn_agent` stub** trong `jcode-keywords/src/workflow/spawn.rs`

Sau khi xong: orchestrator = full Codebuff pipeline, /poke sẽ chạy planner → parallel sub-agents → coordinator → loop until tests pass.

---

## 2. Architecture Decision

### Chosen Approach
**Refactor orchestrator.rs to delegate to swarm.rs.** Không duplicate logic. swarm.rs đã có:
- `run_swarm_task()` (line 937) — spawn 1 sub-agent với allowed_tools
- `parse_swarm_tasks()` (line 1120) — parse JSON array thành SwarmTaskSpec
- `run_swarm_message()` (line 1027) — full pipeline: planner → parallel sub-agents → integration

### Rejected Alternatives

| Approach | Pros | Cons | Decision |
|----------|------|------|----------|
| Keep orchestrator.rs as-is | Không phải refactor | Duplicate logic, 60% gap còn lại | ❌ |
| Replace orchestrator.rs entirely với swarm.rs | Clean | swarm.rs không biết về todos | ❌ |
| Bridge: orchestrator.rs → swarm.rs | Tận dụng cả 2 | Cần wrapper layer | ✅ |
| Build parallel pipeline from scratch | Tối ưu | Re-invent Codebuff | ❌ |

---

## 3. Data Structures & Types

```rust
// crates/jcode-app-core/src/agent/orchestrator.rs — additions

/// Result of orchestrating one todo through full pipeline.
pub struct PipelineResult {
    pub todo_id: String,
    pub subtasks: Vec<SubtaskResult>,
    pub integration_output: String,
    pub all_tests_pass: bool,
    pub retries: u32,
}

pub struct SubtaskResult {
    pub description: String,
    pub subagent_type: String,
    pub output: String,
    pub success: bool,
}

/// Configuration for pipeline retries.
pub struct PipelineConfig {
    pub max_retries: u32,           // default 2
    pub require_tests: bool,        // default true
    pub parallel: bool,             // default true (Codebuff-style)
    pub allowed_tools_override: Option<HashSet<String>>,
}
```

---

## 4. Pseudocode — Core Algorithm

```
FUNCTION orchestrate_todo_via_swarm(agent, todo, config):
    # Step 1: Planner decomposes todo into subtasks (Codebuff pattern)
    planner_prompt = "Break this task into 2-4 subtasks. Return JSON array with \
        keys: description, prompt, subagent_type.\n\nTask: " + todo.content
    plan_output = spawn_subagent_with_type(agent, "planner", planner_prompt)
    subtasks = parse_swarm_tasks(plan_output)
    subtasks = parse_swarm_tasks(plan_text)  # JSON → Vec<SwarmTaskSpec>

    IF subtasks is empty:
        subtasks = [single task with todo.content]

    # Step 2: Run subtasks in parallel (Codebuff pattern)
    attempts = 0
    all_pass = false
    WHILE attempts < config.max_retries AND NOT all_pass:
        task_futures = subtasks.map(|task| spawn_subagent(agent, task))
        outputs = try_join_all(task_futures)  # parallel
        all_pass = run_tests(agent, todo)     # basher agent
        attempts += 1

    # Step 3: Coordinator integrates results (Codebuff pattern)
    integration_prompt = build_integration_prompt(todo, outputs)
    final = spawn_subagent_with_type(agent, "editor", integration_prompt)

    # Step 4: Update todo state
    IF all_pass:
        todo.status = "completed"
    ELSE:
        todo.status = "blocked"
    save_todos(...)  # broadcasts BusEvent::TodoUpdated

    RETURN PipelineResult
```

---

## 5. Implementation Code

### File: `crates/jcode-app-core/src/agent/orchestrator.rs` (refactor)

**Replace current `poll_todo_pipeline` with:**

```rust
impl Agent {
    pub async fn poll_todo_pipeline(&mut self) -> Result<usize> {
        let session_id = self.session.id.clone();
        let todos = crate::todo::load_todos(&session_id).unwrap_or_default();
        let incomplete: Vec<TodoItem> = todos
            .into_iter()
            .filter(|t| !matches!(t.status.as_str(), "completed" | "cancelled"))
            .collect();
        if incomplete.is_empty() { return Ok(0); }

        let config = PipelineConfig::default();
        let mut processed = 0usize;
        for todo in &incomplete {
            match self.orchestrate_todo_via_swarm(todo, &config).await {
                Ok(result) => {
                    if result.all_tests_pass { processed += 1; }
                    crate::logging::info(&format!(
                        "[orchestrator] '{}' done: {} subtasks, all_pass={}",
                        todo.content, result.subtasks.len(), result.all_tests_pass,
                    ));
                }
                Err(e) => {
                    crate::logging::warn(&format!(
                        "[orchestrator] '{}' failed: {e}", todo.content,
                    ));
                }
            }
        }
        Ok(processed)
    }

    async fn orchestrate_todo_via_swarm(
        &mut self,
        todo: &TodoItem,
        config: &PipelineConfig,
    ) -> Result<PipelineResult> {
        use crate::server::swarm::{run_swarm_task, parse_swarm_tasks};
        let provider = Arc::clone(&self.provider);
        let registry = self.registry.clone();

        // Step 1: Planner decomposes (Codebuff pattern from swarm.rs:1038)
        let planner_prompt = format!(
            "Break this task into 2-4 subtasks. Return ONLY a JSON array of \
             objects with keys: description, prompt, subagent_type.\n\nTask:\n{}",
            todo.content,
        );
        let plan_text = spawn_planner_as_child(self, let plan_text = self.run_once_capture_inner(&planner_prompt).await?;planner_prompt).await?;
        let mut subtasks = parse_swarm_tasks(&plan_text);
        if subtasks.is_empty() {
            subtasks.push(SwarmTaskSpec {
                description: todo.content.clone(),
                prompt: todo.content.clone(),
                subagent_type: Some(classify_todo(todo)),
            });
        }

        // Step 2: Run subtasks in parallel with feedback loop
        let mut attempts = 0u32;
        let mut all_pass = false;
        let mut outputs = Vec::new();
        while attempts < config.max_retries && !all_pass {
            let task_futures = subtasks.iter().map(|task| {
                let provider = provider.clone();
                let registry = registry.clone();
                let desc = task.description.clone();
                let prompt = task.prompt.clone();
                let agent_type = task.subagent_type.clone()
                    .unwrap_or_else(|| classify_todo(todo));
                async move {
                    run_swarm_task_with_type(
                        provider, registry, &desc, &agent_type, &prompt,
                    ).await
                }
            });
            outputs = futures::future::try_join_all(task_futures).await?;
            all_pass = self.run_tests_via_basher(todo).await?;
            attempts += 1;
        }

        // Step 3: Coordinator integrates
        let integration_prompt = build_integration_prompt(todo, &subtasks, &outputs);
        let final_output = spawn_coordinator_child(self, let final_output = self.run_once_capture_inner(&integration_prompt).await?;integration_prompt).await?;

        // Step 4: Update todo state via save_todos (broadcasts BusEvent::TodoUpdated)
        let mut updated = todo.clone();
        updated.status = if all_pass { "completed" } else { "blocked" }.into();
        crate::todo::save_todos(&self.session.id, &[updated])?;

        Ok(PipelineResult {
            todo_id: todo.id.clone().unwrap_or_default(),
            subtasks: outputs.iter().enumerate().map(|(i, out)| SubtaskResult {
                description: subtasks.get(i).map(|s| s.description.clone()).unwrap_or_default(),
                subagent_type: subtasks.get(i).and_then(|s| s.subagent_type.clone()).unwrap_or_default(),
                output: out.clone(),
                success: true,
            }).collect(),
            integration_output: final_output,
            all_tests_pass: all_pass,
            retries: attempts,
        })
    }

    async fn run_tests_via_basher(&mut self, todo: &TodoItem) -> Result<bool> {
        let prompt = format!("Run tests for this task:\n\n{}", todo.content);
        let output = self.run_once_capture_inner(&prompt).await?;
        Ok(!output.to_lowercase().contains("failed"))
    }
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            require_tests: true,
            parallel: true,
            allowed_tools_override: None,
        }
    }
}
```

### File: `crates/jcode-keywords/src/workflow/spawn.rs` (wire stub)

```rust
pub async fn spawn_agent(spec: &SpawnSpec) -> SpawnResult {
    // New: wire to swarm.rs run_swarm_task
    use crate::server::swarm::run_swarm_task;
    // ... existing code but call run_swarm_task instead of placeholder
    todo!("wire to run_swarm_task — needs Agent context")
}
```

### File: `crates/jcode-app-core/src/server/compaction_hooks.rs` (add pipeline state save)

After compaction: ALSO re-trigger orchestrator with todo state to restore pipeline progress.

---

## 6. Configuration & Wiring

```toml
# ~/.jcode/config.toml
[todo.orchestrator]
# Pipeline retries (default: 2)
max_retries = 2

# Require tests to pass before marking todo completed (default: true)
require_tests = true

# Run subtasks in parallel via try_join_all (default: true)
parallel = true

# Override allowed tools for sub-agents (default: use classify_todo)
allowed_tools_override = []

# Skip subtasks that user explicitly marked as low-confidence
# (default: false — run all)
skip_low_confidence = false
```

---

## 7. Repo References

| Feature | Repo | File | Link |
|---------|------|------|------|
| **Planner decomposition** | opencode | packages/opencode/src/server/swarm.ts (swarm.rs mirror) | https://github.com/anomalyco/opencode/blob/main/packages/opencode/src/server/swarm.ts |
| **JSON parse_swarm_tasks** | opencode | server/swarm.ts:parse_swarm_tasks | (same repo) |
| **Parallel sub-agent** | opencode | server/swarm.ts:tryJoinAll | (same repo) |
| **Coordinator integration** | opencode | server/swarm.ts:integration_prompt | (same repo) |
| **Feedback loop (retries)** | codebuff | file-picker / code-reviewer | https://github.com/CodebuffAI/codebuff |
| **5-agent pipeline** | codebuff | agents/registry | https://github.com/CodebuffAI/codebuff |

Jcode internal:
- `crates/jcode-app-core/src/server/swarm.rs` — already has 80% of Codebuff pipeline
- `crates/jcode-app-core/src/agent/orchestrator.rs` — needs refactor to use swarm.rs
- `crates/jcode-keywords/src/workflow/spawn.rs` — stub, needs wiring

---

## 8. Test Cases

### Happy Path Tests
```rust
#[test]
fn poll_todo_pipeline_runs_planner() {
    let mut agent = test_agent();
    agent.session.id = "test-1".into();
    save_todos(&agent.session.id, &vec![
        TodoItem { content: "Refactor auth".into(), status: "pending".into(), ..Default::default() },
    ]).unwrap();
    let result = agent.poll_todo_pipeline().await.unwrap();
    // Planner should have run, subtasks created
    assert!(result > 0);
}

#[test]
fn orchestrator_calls_swarm_run_swarm_task() {
    // Mock agent, verify run_swarm_task is called for each subtask
}

#[test]
fn all_subtasks_succeed_marks_todo_completed() {
    // After pipeline + tests pass → todo.status = "completed"
    // Verify BusEvent::TodoUpdated fired
}

#[test]
fn test_failure_triggers_retry_loop() {
    // Basher reports "tests failed"
    // → attempts++, sub-agents re-run
    // → after max_retries, status = "blocked"
}
```

### Edge Cases
```rust
#[test]
fn planner_returns_no_subtasks_falls_back_to_single_subagent() { ... }
#[test]
fn planner_returns_invalid_json_uses_whole_task_as_subtask() { ... }
#[test]
fn todo_already_completed_skipped() { ... }
#[test]
fn todo_with_group_eq_review_routes_to_code_reviewer_only() { ... }
#[test]
fn parallel_true_runs_subtasks_concurrently() { ... }
#[test]
fn parallel_false_runs_subtasks_sequentially() { ... }
#[test]
fn basher_failure_blocks_todo_with_error_message() { ... }
#[test]
fn integration_output_saved_to_todo_completion_note() { ... }
```

### Integration Tests
```rust
#[tokio::test]
async fn end_to_end_pipeline_with_real_planner() {
    // Setup test session with provider mock
    // Create todo: "Build a hello world CLI in Rust"
    // Run poll_todo_pipeline
    // Assert: at least 1 subtask created, file written, todos completed
}

#[tokio::test]
async fn pipeline_preserves_compaction_state() {
    // After compaction, pipeline state should restore from disk
}
```

---

## 9. Benchmarks

### What to Measure
| Metric | Baseline | Target | How to Measure |
|--------|----------|--------|----------------|
| Planner decomposition latency | - | < 5s | measure around parse_swarm_tasks |
| Parallel subtask throughput | - | 2x sequential | 3 subtasks parallel vs sequential wall time |
| Pipeline retries (tests fail) | - | 90% catch on retry 1 | run failing-test scenarios |
| BusEvent::TodoUpdated latency | - | < 100ms | measure between save_todos and event publish |
| orchestrator_total_time_for_5_todos | - | < 60s | 5 todos × 10s each (parallel) |

### Benchmark Code
```rust
// crates/jcode-app-core/benches/orchestrator_bench.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_poll_todo_pipeline(c: &mut Criterion) {
    let mut agent = test_agent();
    setup_todos(&mut agent, 5);
    c.bench_function("poll_5_todos", |b| {
        b.iter(|| agent.poll_todo_pipeline());
    });
}

fn bench_parallel_vs_sequential(c: &mut Criterion) {
    // ... benchmark parallel=true vs parallel=false
}
```

---

## 10. Migration / Rollout

**Phased rollout to avoid breaking existing auto-poke:**

**Phase 1** (1 day): Add `PipelineConfig::default()` and `orchestrate_todo_via_swarm` as **opt-in via flag**:
```rust
if !self.use_legacy_orchestrator {
    self.orchestrate_todo_via_swarm(todo, &config).await?
} else {
    // existing classify → single sub-agent path
}
```

**Phase 2** (2 days): Wire `spawn_agent` stub to call `run_swarm_task`. Test on real tasks.

**Phase 3** (2 days): Enable parallel mode by default, set max_retries=2.

**Phase 4** (1 day): Enable feedback loop (basher → retry on test fail).

**Phase 5**: Deprecate legacy path. Force pipeline mode.

No feature flag needed since each phase is backward compatible.

---

## 11. Known Limitations & Future Work

- [ ] **Parallel conflict detection**: When 2 sub-agents edit same file, last-write-wins. Need file reservation system like Agent Mail.
- [ ] **Dependency graph between subtasks**: Codebuff allows subtasks to depend on each other. Current implementation runs in parallel unconditionally.
- [ ] **Cost control**: Each subtask is a full LLM call. Add cost cap or budget.
- [ ] **Cancellation**: User can't cancel a running pipeline mid-execution.
- [ ] **Persistent logs**: Subagent outputs only saved if todo is completed. Failed retries don't keep history.

---

## 12. Success Criteria Checklist

- [ ] `orchestrate_todo_via_swarm` calls `run_swarm_task` for each subtask
- [ ] Planner decomposes 1 todo → 2-4 subtasks (JSON array parse works)
- [ ] Subtasks run in parallel via `try_join_all`
- [ ] Basher agent runs tests; failure triggers retry (up to max_retries=2)
- [ ] Coordinator agent integrates outputs into final response
- [ ] Todo status updated: "completed" if all pass, "blocked" if retries exhausted
- [ ] BusEvent::TodoUpdated fired on each todo state change
- [ ] `spawn_agent()` stub in jcode-keywords wired to run_swarm_task
- [ ] 5 integration tests pass
- [ ] `cargo check` clean for jcode-app-core + jcode-base + jcode-keywords
- [ ] Existing orchestrator.rs tests still pass (no regression)

---

## Estimated scope
- **Modified files**: 3 (orchestrator.rs, swarm.rs, spawn.rs)
- **New code**: ~300 LOC
- **New tests**: ~10 unit + 3 integration
- **Migration time**: ~1 week
