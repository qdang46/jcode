//! Full Codebuff-style multi-agent orchestrator pipeline.
//!
//! Pipeline per todo:
//!   planner (child) → [parallel sub-agents] → basher (child) → coordinator (child)
//!
//! NEVER calls self.run_once_capture_inner — all work done by child agents.
//! Parent only: save_todos(status) to persist + broadcast.

use super::*;
use anyhow::Result;
use futures::future::try_join_all;
use jcode_task_types::TodoItem;
use std::collections::HashSet;

const MAX_RETRIES: u32 = 2;

/// A subtask produced by the planner agent.
struct SwarmTaskSpec {
    description: String,
    prompt: String,
    subagent_type: String,
}

/// Result of orchestrating one todo through the full pipeline.
pub(super) struct PipelineResult {
    pub all_tests_pass: bool,
    pub subtask_count: usize,
}

/// Classify a todo into an agent type.
pub(super) fn classify_todo(todo: &TodoItem) -> String {
    let c = todo.content.to_ascii_lowercase();
    let g = todo.group.as_deref().unwrap_or("").to_ascii_lowercase();
    if g.contains("plan") || g.contains("foundation") {
        return "planner".into();
    }
    if g.contains("test") || g.contains("verify") || g.contains("qa") {
        return "basher".into();
    }
    if g.contains("review") {
        return "code-reviewer".into();
    }
    if g.contains("search") || g.contains("find") {
        return "file-picker".into();
    }
    if c.contains("plan") || c.contains("analyz") || c.contains("design") {
        return "planner".into();
    }
    if c.contains("test") || c.contains("verif") || c.contains("check") {
        return "basher".into();
    }
    if c.contains("review") || c.contains("audit") {
        return "code-reviewer".into();
    }
    if c.contains("search") || c.contains("find") || c.starts_with("read") {
        return "file-picker".into();
    }
    "editor".into()
}

/// Build allowed-tool set matching each agent type.
pub(crate) fn build_allowed_tools(tp: &str) -> HashSet<String> {
    let tools: Vec<&str> = match tp {
        "planner" => vec!["read", "glob", "grep", "codesearch", "session_search", "ls"],
        "file-picker" => vec!["ls", "glob", "read"],
        "editor" => vec![
            "read",
            "write",
            "edit",
            "hashline_edit",
            "propose_edit",
            "glob",
            "grep",
            "codesearch",
            "ls",
            "bash",
        ],
        "code-reviewer" => vec!["read", "glob", "grep", "codesearch", "ls"],
        "basher" => vec!["bash", "read", "glob", "ls"],
        _ => vec!["read", "bash"],
    };
    tools.into_iter().map(String::from).collect()
}

impl Agent {
    pub fn set_todo_orchestrator_enabled(&mut self, v: bool) {
        self.todo_orchestrator_enabled = v;
    }
    pub fn todo_orchestrator_enabled(&self) -> bool {
        self.todo_orchestrator_enabled
    }

    /// Run the full Codebuff pipeline for all incomplete todos.
    pub async fn poll_todo_pipeline(&mut self) -> Result<usize> {
        let sid = self.session.id.clone();
        let todos = crate::todo::load_todos(&sid).unwrap_or_default();
        let incomplete: Vec<TodoItem> = todos
            .into_iter()
            .filter(|t| !matches!(t.status.as_str(), "completed" | "cancelled"))
            .collect();
        if incomplete.is_empty() {
            return Ok(0);
        }

        let provider = Arc::clone(&self.provider);
        let registry = self.registry.clone();
        let parent_sid = sid.clone();
        let mut processed = 0usize;

        for todo in &incomplete {
            let result = orchestrate_one_todo(&provider, &registry, &parent_sid, todo).await;
            match result {
                Ok(r) => {
                    if r.all_tests_pass {
                        processed += 1;
                    }
                    crate::logging::info(&format!(
                        "[orchestrator] '{}': {} subtasks, pass={}",
                        todo.content, r.subtask_count, r.all_tests_pass,
                    ));
                }
                Err(e) => {
                    crate::logging::warn(&format!("[orchestrator] '{}' failed: {e}", todo.content))
                }
            }
        }
        if processed > 0 {
            crate::logging::info(&format!("[orchestrator] processed {processed} todos"));
        }
        Ok(processed)
    }
}

// ─── Pipeline free functions (no &self, all via child agents) ────────────

/// Orchestrate one todo through full Codebuff pipeline.
/// All sub-agents are spawned as children — NEVER runs on the parent agent.
async fn orchestrate_one_todo(
    provider: &Arc<dyn Provider>,
    registry: &Registry,
    parent_sid: &str,
    todo: &TodoItem,
) -> Result<PipelineResult> {
    // 1. Planner child → decompose into subtasks
    let plan_prompt = format!(
        "Break this task into 2-4 subtasks. Return ONLY a JSON array of \
         objects with keys: description, prompt, subagent_type. \
         No extra text.\n\nTask:\n{}",
        todo.content,
    );
    let plan_text = spawn_child(provider, registry, parent_sid, "planner", &plan_prompt).await?;
    let mut subtasks = parse_swarm_tasks(&plan_text);
    if subtasks.is_empty() {
        subtasks.push(SwarmTaskSpec {
            description: todo.content.clone(),
            prompt: todo.content.clone(),
            subagent_type: classify_todo(todo),
        });
    }

    let mut attempts = 0u32;
    let mut all_pass = false;
    while attempts < MAX_RETRIES && !all_pass {
        // 2. Run subtasks in PARALLEL (try_join_all)
        let futures: Vec<_> = subtasks
            .iter()
            .map(|st| {
                let p = Arc::clone(provider);
                let r = registry.clone();
                let sid = parent_sid.to_string();
                let prompt = st.prompt.clone();
                let atype = st.subagent_type.clone();
                async move { spawn_child(&p, &r, &sid, &atype, &prompt).await }
            })
            .collect();
        let outputs = try_join_all(futures).await?;

        // 3. Basher child → run tests
        let test_prompt = format!(
            "Run relevant tests for this task AND REPORT pass/fail:\n\n{}",
            todo.content
        );
        let test_out = spawn_child(provider, registry, parent_sid, "basher", &test_prompt).await?;
        all_pass = !test_out.to_ascii_lowercase().contains("fail");
        attempts += 1;
    }

    // 4. Coordinator child → integrate all results
    let integration_prompt = format!(
        "Integrate the completed subtask results and produce a final summary.\n\nTask:\n{}",
        todo.content,
    );
    let _final_out = spawn_child(
        provider,
        registry,
        parent_sid,
        "editor",
        &integration_prompt,
    )
    .await?;

    // 5. Persist and broadcast: load ALL todos, update the one just processed.
    // save_todos replaces the full list (whole-list replace pattern).
    let mut all_todos = crate::todo::load_todos(parent_sid).unwrap_or_default();
    for t in &mut all_todos {
        if t.content == todo.content && t.id == todo.id {
            t.status = if all_pass {
                "completed".into()
            } else {
                "blocked".into()
            };
            break;
        }
    }
    crate::todo::save_todos(parent_sid, &all_todos)?;
    // save_todos internally broadcasts BusEvent::TodoUpdated.

    Ok(PipelineResult {
        all_tests_pass: all_pass,
        subtask_count: subtasks.len(),
    })
}

/// Spawn a single child agent with given type and prompt.
/// NEVER persists to parent session. Returns child's text output.
async fn spawn_child(
    provider: &Arc<dyn Provider>,
    registry: &Registry,
    parent_sid: &str,
    agent_type: &str,
    prompt: &str,
) -> Result<String> {
    let session = Session::create(
        Some(parent_sid.to_string()),
        Some(format!("orchestrator-{agent_type}")),
    );
    let allowed = build_allowed_tools(agent_type);
    let mut child = Agent::new_with_session(
        Arc::clone(provider),
        registry.clone(),
        session,
        Some(allowed),
    );
    child.run_once_capture_inner(prompt).await
}

/// Parse the planner's JSON array response into SwarmTaskSpecs.
/// Accepts wrapped or unwrapped JSON (Codebuff pattern).
fn parse_swarm_tasks(text: &str) -> Vec<SwarmTaskSpec> {
    let trimmed = text.trim();
    // Try direct parse
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
        return arr.into_iter().filter_map(parse_one_task).collect();
    }
    // Try wrapping in array (sometimes model wraps in ```json ... ```)
    if let Some(inner) = trimmed.strip_prefix("```json") {
        if let Some(end) = inner.rfind("```") {
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(inner[..end].trim()) {
                return arr.into_iter().filter_map(parse_one_task).collect();
            }
        }
    }
    Vec::new()
}

fn parse_one_task(v: serde_json::Value) -> Option<SwarmTaskSpec> {
    let desc = v.get("description")?.as_str()?.to_string();
    let prompt = v.get("prompt")?.as_str()?.to_string();
    let subagent_type = v
        .get("subagent_type")
        .and_then(|s| s.as_str())
        .unwrap_or("editor")
        .to_string();
    Some(SwarmTaskSpec {
        description: desc,
        prompt,
        subagent_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn td(c: &str, g: Option<&str>) -> TodoItem {
        TodoItem {
            content: c.into(),
            group: g.map(String::from),
            ..Default::default()
        }
    }
    fn check(c: &str, g: Option<&str>, e: &str) {
        assert_eq!(
            classify_todo(&td(c, g)),
            e,
            "mismatch for {c:?} group={g:?}"
        );
    }
    #[test]
    fn t_pl() {
        check("Design auth", None, "planner");
    }
    #[test]
    fn t_ed() {
        check("Implement btn", None, "editor");
    }
    #[test]
    fn t_ba() {
        check("Fix test", Some("qa"), "basher");
    }
    #[test]
    fn t_rv() {
        check("Review PR", None, "code-reviewer");
    }
    #[test]
    fn t_fp() {
        check("Find files", Some("search"), "file-picker");
    }
    #[test]
    fn t_tools() {
        let t = build_allowed_tools("planner");
        assert!(t.contains("read"));
        assert!(!t.contains("write"));
    }

    fn parse(s: &str) -> Vec<SwarmTaskSpec> {
        parse_swarm_tasks(s)
    }

    #[test]
    fn parse_json_array() {
        let json =
            r#"[{"description":"Fix auth","prompt":"Update login.ts","subagent_type":"editor"}]"#;
        assert_eq!(parse(json).len(), 1);
    }

    #[test]
    fn parse_wrapped_json() {
        let wrapped = "```json\n[{\"description\":\"Fix db\",\"prompt\":\"Update db.ts\",\"subagent_type\":\"editor\"}]\n```";
        assert_eq!(parse(wrapped).len(), 1);
    }

    #[test]
    fn parse_fallback_empty() {
        assert!(parse("Just do it").is_empty());
    }

    #[test]
    fn parse_multiple_tasks() {
        let json = r#"[
            {"description":"A","prompt":"a","subagent_type":"editor"},
            {"description":"B","prompt":"b","subagent_type":"file-picker"}
        ]"#;
        let tasks = parse(json);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1].subagent_type, "file-picker");
    }

    #[test]
    fn parse_swarm_tasks_skips_malformed() {
        let json = r#"[
            {"description":"good","prompt":"ok","subagent_type":"editor"},
            {"description":"bad"}  // missing prompt
        ]"#;
        assert_eq!(parse(json).len(), 1);
    }
}
