use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

/// Get the teams directory path (~/.jcode/teams/).
fn teams_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".jcode")
        .join("teams")
}

/// Team configuration stored as JSON on disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamConfig {
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub members: Vec<TeamMember>,
    pub tasks: Vec<TeamTask>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamMember {
    pub name: String,
    pub session_id: String,
    pub agent_type: String,
    pub status: String, // "active" | "idle" | "shutdown"
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamTask {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub status: String,        // "pending" | "in_progress" | "completed"
    pub owner: Option<String>, // member name
}

/// Validate that a team name is safe for use as a filename.
/// Rejects path traversal attempts and special characters.
fn validate_team_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Team name cannot be empty");
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        anyhow::bail!(
            "Team name '{}' is invalid: must not contain '..', '/', or '\\'",
            name
        );
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        anyhow::bail!(
            "Team name '{}' is invalid: only alphanumeric, hyphen, and underscore allowed",
            name
        );
    }
    Ok(())
}

impl TeamConfig {
    /// Load a team config from disk by name.
    pub fn load(name: &str) -> Result<Option<Self>> {
        validate_team_name(name)?;
        let path = teams_dir().join(format!("{name}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&text)?))
    }

    /// Save this team config to disk.
    pub fn save(&self) -> Result<()> {
        validate_team_name(&self.name)?;
        let dir = teams_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", self.name));
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Delete a team config from disk by name.
    pub fn delete(name: &str) -> Result<()> {
        validate_team_name(name)?;
        let path = teams_dir().join(format!("{name}.json"));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TeamCreateTool
// ---------------------------------------------------------------------------

pub struct TeamCreateTool;

impl TeamCreateTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamCreateInput {
    name: String,
    description: String,
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Create a new team for coordinating sub-agents. Stores a lightweight \
         team config file at ~/.jcode/teams/<name>.json that tracks members, \
         tasks, and status."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name", "description"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": {
                    "type": "string",
                    "description": "Unique team name (used as filename)."
                },
                "description": {
                    "type": "string",
                    "description": "What this team is for."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let params: TeamCreateInput = serde_json::from_value(input)?;

        if let Some(existing) = TeamConfig::load(&params.name)? {
            return Ok(ToolOutput::new(format!(
                "Team '{}' already exists.\n\n{}",
                params.name,
                serde_json::to_string_pretty(&existing)?
            ))
            .with_title(format!("Team '{}' already exists", params.name)));
        }

        let team = TeamConfig {
            name: params.name.clone(),
            description: params.description.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            members: Vec::new(),
            tasks: Vec::new(),
        };
        team.save()?;

        let output = serde_json::to_string_pretty(&team)?;
        Ok(
            ToolOutput::new(format!("Team '{}' created.\n\n{}", params.name, output))
                .with_title(format!("Team '{}' created", params.name)),
        )
    }
}

// ---------------------------------------------------------------------------
// TeamDeleteTool
// ---------------------------------------------------------------------------

pub struct TeamDeleteTool;

impl TeamDeleteTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct TeamDeleteInput {
    name: String,
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        "team_delete"
    }

    fn description(&self) -> &str {
        "Delete a team configuration. Removes the team config file from \
         ~/.jcode/teams/<name>.json."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": {
                    "type": "string",
                    "description": "Team name to delete."
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let params: TeamDeleteInput = serde_json::from_value(input)?;

        let existed = TeamConfig::load(&params.name)?.is_some();
        TeamConfig::delete(&params.name)?;

        if existed {
            Ok(ToolOutput::new(format!("Team '{}' deleted.", params.name))
                .with_title(format!("Team '{}' deleted", params.name)))
        } else {
            Ok(
                ToolOutput::new(format!("Team '{}' did not exist (no-op).", params.name))
                    .with_title(format!("Team '{}' not found", params.name)),
            )
        }
    }
}
