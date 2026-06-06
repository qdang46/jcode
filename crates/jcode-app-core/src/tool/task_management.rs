use super::team::{TeamConfig, TeamTask};
use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// TaskCreateTool
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct TaskCreateTool;

impl TaskCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TaskCreateInput {
    team_name: String,
    subject: String,
    description: String,
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "task_create"
    }

    fn description(&self) -> &str {
        "Create a new task within a team. The task starts with status 'pending' \
         and no owner assigned."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_name", "subject", "description"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_name": {
                    "type": "string",
                    "description": "Team to add the task to."
                },
                "subject": {
                    "type": "string",
                    "description": "Short task title."
                },
                "description": {
                    "type": "string",
                    "description": "Detailed task description."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let params: TaskCreateInput = serde_json::from_value(input)?;

        let mut team = match TeamConfig::load(&params.team_name)? {
            Some(t) => t,
            None => {
                return Err(anyhow::anyhow!(
                    "Team '{}' not found. Create it first with team_create.",
                    params.team_name
                ));
            }
        };

        let task_id = format!("task-{}", uuid::Uuid::new_v4().as_simple());
        let task = TeamTask {
            id: task_id.clone(),
            subject: params.subject,
            description: params.description,
            status: "pending".to_string(),
            owner: None,
        };
        team.tasks.push(task);
        team.save()?;

        Ok(ToolOutput::new(format!(
            "Task '{}' created in team '{}'.",
            task_id, params.team_name
        ))
        .with_title(format!("Task created: {}", task_id)))
    }
}

// ---------------------------------------------------------------------------
// TaskUpdateTool
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct TaskUpdateTool;

impl TaskUpdateTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TaskUpdateInput {
    team_name: String,
    task_id: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    owner: Option<String>,
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "task_update"
    }

    fn description(&self) -> &str {
        "Update a task's status or owner within a team."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_name", "task_id"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_name": {
                    "type": "string",
                    "description": "Team containing the task."
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID to update."
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed"],
                    "description": "New status for the task."
                },
                "owner": {
                    "type": "string",
                    "description": "Assign or reassign the task to a team member name."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let params: TaskUpdateInput = serde_json::from_value(input)?;

        let mut team = match TeamConfig::load(&params.team_name)? {
            Some(t) => t,
            None => {
                return Err(anyhow::anyhow!("Team '{}' not found.", params.team_name));
            }
        };

        let task = team
            .tasks
            .iter_mut()
            .find(|t| t.id == params.task_id)
            .ok_or_else(|| anyhow::anyhow!("Task '{}' not found.", params.task_id))?;

        if let Some(status) = params.status {
            task.status = status;
        }
        if let Some(owner) = params.owner {
            task.owner = Some(owner);
        }

        let updated = task.clone();
        team.save()?;

        Ok(ToolOutput::new(format!(
            "Task '{}' updated.\n\n{}",
            params.task_id,
            serde_json::to_string_pretty(&updated)?
        ))
        .with_title(format!("Task '{}' updated", params.task_id)))
    }
}

// ---------------------------------------------------------------------------
// TaskListTool
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct TaskListTool;

impl TaskListTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TaskListInput {
    team_name: String,
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "task_list"
    }

    fn description(&self) -> &str {
        "List all tasks in a team, showing their status and owner."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_name"],
            "properties": {
                "intent": super::intent_schema_property(),
                "team_name": {
                    "type": "string",
                    "description": "Team to list tasks for."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let params: TaskListInput = serde_json::from_value(input)?;

        let team = match TeamConfig::load(&params.team_name)? {
            Some(t) => t,
            None => {
                return Err(anyhow::anyhow!("Team '{}' not found.", params.team_name));
            }
        };

        let output = serde_json::to_string_pretty(&team.tasks)?;
        let summary = format!(
            "Team '{}': {} task(s) total, {} pending, {} in_progress, {} completed.",
            params.team_name,
            team.tasks.len(),
            team.tasks.iter().filter(|t| t.status == "pending").count(),
            team.tasks
                .iter()
                .filter(|t| t.status == "in_progress")
                .count(),
            team.tasks
                .iter()
                .filter(|t| t.status == "completed")
                .count(),
        );

        Ok(
            ToolOutput::new(format!("{}\n\n{}", summary, output)).with_title(format!(
                "{} tasks in '{}'",
                team.tasks.len(),
                params.team_name
            )),
        )
    }
}
