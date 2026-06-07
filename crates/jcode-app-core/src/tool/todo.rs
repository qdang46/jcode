use super::{Tool, ToolContext, ToolOutput};
use crate::bus::{Bus, BusEvent, TodoEvent};
use crate::todo::{TodoItem, load_todos, save_todos};
use anyhow::Result;
use async_trait::async_trait;
use jcode_hooks::{
    DispatchConfig, HookContext, HookEvent, HookInputBuilder, HookRegistry, load_hooks_config,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub struct TodoTool;

impl TodoTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TodoInput {
    todos: Option<Vec<TodoItem>>,
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn description(&self) -> &str {
        "Read or update the todo list. Include confidence for each item and completion_confidence when marking an item completed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "todos": {
                    "type": "array",
                    "description": "Todo list to save.",
                    "items": {
                        "type": "object",
                        "required": ["content", "status", "priority", "id", "confidence"],
                        "properties": {
                            "content": {
                                "type": "string",
                                "description": "Task."
                            },
                            "status": {
                                "type": "string",
                                "description": "Status."
                            },
                            "priority": {
                                "type": "string",
                                "description": "Priority."
                            },
                            "id": {
                                "type": "string",
                                "description": "ID."
                            },
                            "group": {
                                "type": "string",
                                "description": "Optional group label. Todos sharing a group render together under one header. Use one group per coherent goal (e.g. 'optimize rendering'). When the user steers into new work, start a new group instead of renaming the existing one. Omit for an ungrouped flat list."
                            },
                            "confidence": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 100,
                                "description": "Forward-looking confidence, 0-100, that this todo can be completed correctly. Set when creating or substantially revising a todo."
                            },
                            "completion_confidence": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 100,
                                "description": "Confidence, 0-100, that this todo is correctly completed. Set when marking the todo completed; omit until then."
                            }
                        }
                    }
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: TodoInput = serde_json::from_value(input)?;
        let operation = if params.todos.is_some() {
            "write"
        } else {
            "read"
        };
        match params.todos {
            Some(todos) => {
                let existing = load_todos(&ctx.session_id).unwrap_or_default();
                let existing_ids: std::collections::HashSet<&str> =
                    existing.iter().map(|t| t.id.as_str()).collect();
                let completed_ids: std::collections::HashSet<&str> = existing
                    .iter()
                    .filter(|t| t.status == "completed")
                    .map(|t| t.id.as_str())
                    .collect();

                save_todos(&ctx.session_id, &todos)?;

                Bus::global().publish(BusEvent::TodoUpdated(TodoEvent {
                    session_id: ctx.session_id.clone(),
                    todos: todos.clone(),
                }));

                // Fire TaskCreated / TaskCompleted hooks (fire-and-forget, observational)
                let session_id = ctx.session_id.clone();
                let cwd = ctx
                    .working_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                let new_todos: Vec<TodoItem> = todos
                    .iter()
                    .filter(|t| !existing_ids.contains(t.id.as_str()))
                    .cloned()
                    .collect();
                let newly_completed: Vec<TodoItem> = todos
                    .iter()
                    .filter(|t| t.status == "completed" && !completed_ids.contains(t.id.as_str()))
                    .cloned()
                    .collect();
                tokio::spawn(async move {
                    let hook_config = load_hooks_config();
                    let hook_registry = HookRegistry::from_config(hook_config.clone());
                    let dispatch_config = DispatchConfig::from_settings(&hook_config.settings);

                    for todo in &new_todos {
                        let mut hook_ctx = HookContext::new(&session_id, "", &cwd, "TaskCreated");
                        hook_ctx.task_id = Some(todo.id.clone());
                        let handlers =
                            hook_registry.get_matching(&HookEvent::TaskCreated, &hook_ctx);
                        if !handlers.is_empty() {
                            let hook_input = HookInputBuilder::new()
                                .session(&session_id, &cwd)
                                .event("TaskCreated")
                                .build();
                            let _ = jcode_hooks::dispatch_hooks(
                                &HookEvent::TaskCreated,
                                &hook_input,
                                &handlers,
                                &dispatch_config,
                            )
                            .await;
                        }
                    }

                    for todo in &newly_completed {
                        let mut hook_ctx = HookContext::new(&session_id, "", &cwd, "TaskCompleted");
                        hook_ctx.task_id = Some(todo.id.clone());
                        let handlers =
                            hook_registry.get_matching(&HookEvent::TaskCompleted, &hook_ctx);
                        if !handlers.is_empty() {
                            let hook_input = HookInputBuilder::new()
                                .session(&session_id, &cwd)
                                .event("TaskCompleted")
                                .build();
                            let _ = jcode_hooks::dispatch_hooks(
                                &HookEvent::TaskCompleted,
                                &hook_input,
                                &handlers,
                                &dispatch_config,
                            )
                            .await;
                        }
                    }
                });

                let remaining = todos.iter().filter(|t| t.status != "completed").count();
                Ok(ToolOutput::new(serde_json::to_string_pretty(&todos)?)
                    .with_title(format!("{} todos", remaining))
                    .with_metadata(json!({"todos": todos})))
            }
            None => {
                let todos = load_todos(&ctx.session_id)?;
                let remaining = todos.iter().filter(|t| t.status != "completed").count();
                Ok(ToolOutput::new(serde_json::to_string_pretty(&todos)?)
                    .with_title(format!("{} todos", remaining))
                    .with_metadata(json!({"todos": todos})))
            }
        }
        .map_err(|err| {
            crate::logging::warn(&format!(
                "[tool:todo] operation failed operation={} session_id={} error={}",
                operation, ctx.session_id, err
            ));
            err
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_is_named_todo() {
        assert_eq!(TodoTool::new().name(), "todo");
    }

    #[test]
    fn schema_advertises_intent_and_todos() {
        let schema = TodoTool::new().parameters_schema();
        let props = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("todo schema should have properties");
        assert_eq!(props.len(), 2);
        assert!(props.contains_key("intent"));
        assert!(props.contains_key("todos"));

        let item = props["todos"]
            .get("items")
            .and_then(|v| v.as_object())
            .expect("todos should describe item objects");
        let required = item
            .get("required")
            .and_then(|v| v.as_array())
            .expect("todo item should advertise required fields");
        assert!(required.iter().any(|v| v == "confidence"));
        let item_props = item
            .get("properties")
            .and_then(|v| v.as_object())
            .expect("todo item should advertise properties");
        assert!(item_props.contains_key("confidence"));
        assert!(item_props.contains_key("completion_confidence"));
    }
}
