//! Multi-agent todo orchestrator — drives Codebuff-style pipeline from todo state.
use super::*;
use anyhow::Result;
use jcode_task_types::TodoItem;

/// Classify a todo into an agent type (planner, file-picker, editor, code-reviewer, basher).
pub(super) fn classify_todo(todo: &TodoItem) -> String {
    let content = todo.content.to_ascii_lowercase();
    let group = todo.group.as_deref().unwrap_or("").to_ascii_lowercase();
    if group.contains("plan") || group.contains("foundation") { return "planner".into(); }
    if group.contains("test") || group.contains("verify") || group.contains("qa") { return "basher".into(); }
    if group.contains("review") { return "code-reviewer".into(); }
    if group.contains("search") || group.contains("find") { return "file-picker".into(); }
    if content.contains("plan") || content.contains("analyz") || content.contains("design") { return "planner".into(); }
    if content.contains("test") || content.contains("verif") || content.contains("check") { return "basher".into(); }
    if content.contains("review") || content.contains("audit") { return "code-reviewer".into(); }
    if content.contains("search") || content.contains("find") || content.starts_with("read") { return "file-picker".into(); }
    "editor".into()
}

/// Build the prompt for a sub-agent based on its type and the todo.
fn build_prompt(todo: &TodoItem) -> String {
    match classify_todo(todo).as_str() {
        "planner" => format!("Analyze this task and produce a step-by-step plan:\n\n{}", todo.content),
        "file-picker" => format!("Find relevant files in the codebase for this task:\n\n{}", todo.content),
        "editor" => format!("Task: {}\nGroup: {}\nPriority: {}", todo.content, todo.group.as_deref().unwrap_or("default"), if todo.priority.is_empty() { "medium" } else { &todo.priority }),
        "code-reviewer" => format!("Review the code changes for this task:\n\n{}", todo.content),
        "basher" => format!("Run relevant tests for this task:\n\n{}", todo.content),
        _ => todo.content.clone(),
    }
}

/// Allowed-tool set matching each agent's `.toml` definition.
pub(crate) fn build_allowed_tools(agent_type: &str) -> HashSet<String> {
    let tools: Vec<&str> = match agent_type {
        "planner" => vec!["read","glob","grep","codesearch","session_search","ls"],
        "file-picker" => vec!["ls","glob","read"],
        "editor" => vec!["read","write","edit","hashline_edit","propose_edit","glob","grep","codesearch","ls","bash"],
        "code-reviewer" => vec!["read","glob","grep","codesearch","ls"],
        "basher" => vec!["bash","read","glob","ls"],
        _ => vec!["read","bash"],
    };
    tools.into_iter().map(String::from).collect()
}

impl Agent {
    /// Enable/disable the todo orchestrator (post-turn sub-agent pipeline).
    pub fn set_todo_orchestrator_enabled(&mut self, enabled: bool) { self.todo_orchestrator_enabled = enabled; }
    pub fn todo_orchestrator_enabled(&self) -> bool { self.todo_orchestrator_enabled }

    /// Run the todo pipeline: spawn sub-agents for all incomplete todos.
    pub async fn poll_todo_pipeline(&mut self) -> Result<usize> {
        let session_id = self.session.id.clone();
        let todos = crate::todo::load_todos(&session_id).unwrap_or_default();
        let incomplete: Vec<TodoItem> = todos.into_iter().filter(|t| !matches!(t.status.as_str(), "completed" | "cancelled")).collect();
        if incomplete.is_empty() { return Ok(0); }

        let provider = Arc::clone(&self.provider);
        let registry = self.registry.clone();
        let mut processed = 0usize;

        for todo in &incomplete {
            let child_session = Session::create(Some(self.session.id.clone()), Some(format!("orchestrator-{}", todo.id)));
            let mut child = Agent::new_with_session(provider.clone(), registry.clone(), child_session, Some(build_allowed_tools(&classify_todo(todo))));
            match child.run_once_capture_inner(&build_prompt(todo)).await {
                Ok(output) => { crate::logging::info(&format!("[orchestrator] '{}' done ({} chars)", classify_todo(&todo), output.len())); processed += 1; }
                Err(e) => { crate::logging::warn(&format!("[orchestrator] '{}' failed: {e}", classify_todo(&todo))); }
            }
        }
        if processed > 0 { crate::logging::info(&format!("[orchestrator] processed {processed} todos")); }
        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn td(c: &str, g: Option<&str>) -> TodoItem { TodoItem { content: c.into(), group: g.map(String::from), ..Default::default() } }
    fn check(c: &str, g: Option<&str>, expected: &str) { assert_eq!(classify_todo(&td(c, g)), expected); }
    #[test] fn t_planner() { check("Design the auth", None, "planner"); }
    #[test] fn t_editor() { check("Implement button", None, "editor"); }
    #[test] fn t_basher() { check("Fix test", Some("qa"), "basher"); }
    #[test] fn t_reviewer() { check("Review PR", None, "code-reviewer"); }
    #[test] fn t_filepicker() { check("Find files", Some("search"), "file-picker"); }
    #[test] fn t_tools_readonly() { let t = build_allowed_tools("planner"); assert!(t.contains("read")); assert!(!t.contains("write")); }
    #[test] fn t_tools_editor() { let t = build_allowed_tools("editor"); assert!(t.contains("write")); assert!(t.contains("bash")); }
}
