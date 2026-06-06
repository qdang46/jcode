//! Coordination tools for multi-agent shared memory.
//!
//! Bridges mempalace's coordination module into jcode's tool system.
//! Provides 8 tools for signals, actions, file reservations, and saturation checks.

use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use jcode_mempalace_adapter::coordination::CoordinationAdapter;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared coordination adapter for all coordination tools.
pub type SharedCoordination = Arc<RwLock<CoordinationAdapter>>;

// ============================================================================
// shared_memory_write
// ============================================================================

pub struct SharedMemoryWriteTool {
    coord: SharedCoordination,
}

impl SharedMemoryWriteTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for SharedMemoryWriteTool {
    fn name(&self) -> &str {
        "shared_memory_write"
    }

    fn description(&self) -> &str {
        "Write a memory entry to the shared memory pool. All agents in the swarm can read this memory. Use for: decisions, findings, blockers, learnings, requirements."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The memory content to store"
                },
                "category": {
                    "type": "string",
                    "enum": ["fact", "preference", "entity", "correction", "decision", "finding", "blocker", "learning", "requirement"],
                    "description": "Category of the memory"
                },
                "tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags for categorization"
                }
            },
            "required": ["content", "category"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'content'"))?;
        let category = input["category"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'category'"))?;

        let coord = self.coord.read().await;
        let agent_id = &ctx.session_id;

        // Store as a signal (inter-agent message)
        let signal_id = coord.send_signal(
            agent_id,
            "shared", // broadcast to all
            content,
            jcode_mempalace_adapter::SignalType::Info,
        )?;

        Ok(ToolOutput::new(format!(
            "Memory {} written to shared pool (category: {})",
            signal_id, category
        )))
    }
}

// ============================================================================
// shared_memory_read
// ============================================================================

pub struct SharedMemoryReadTool {
    coord: SharedCoordination,
}

impl SharedMemoryReadTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for SharedMemoryReadTool {
    fn name(&self) -> &str {
        "shared_memory_read"
    }

    fn description(&self) -> &str {
        "Read memories from the shared memory pool. Can read by signal ID or list recent signals."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unread_only": {
                    "type": "boolean",
                    "default": true,
                    "description": "Only return unread signals"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let unread_only = input["unread_only"].as_bool().unwrap_or(true);

        let coord = self.coord.read().await;
        let signals = coord.read_signals(&ctx.session_id, unread_only)?;

        if signals.is_empty() {
            return Ok(ToolOutput::new("No shared memories found.".to_string()));
        }

        let mut result = format!("Found {} shared memories:\n\n", signals.len());
        for signal in &signals {
            result.push_str(&format!(
                "- [{}] from={}: {}\n",
                signal.id, signal.from, signal.content
            ));
        }

        Ok(ToolOutput::new(result))
    }
}

// ============================================================================
// shared_memory_list
// ============================================================================

pub struct SharedMemoryListTool {
    coord: SharedCoordination,
}

impl SharedMemoryListTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for SharedMemoryListTool {
    fn name(&self) -> &str {
        "shared_memory_list"
    }

    fn description(&self) -> &str {
        "List all active actions and their statuses in the shared pool."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "failed", "blocked"],
                    "description": "Filter by status"
                },
                "project": {
                    "type": "string",
                    "description": "Filter by project"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let status = input["status"].as_str().map(|s| match s {
            "pending" => jcode_mempalace_adapter::ActionStatus::Pending,
            "in_progress" => jcode_mempalace_adapter::ActionStatus::InProgress,
            "completed" => jcode_mempalace_adapter::ActionStatus::Completed,
            "failed" => jcode_mempalace_adapter::ActionStatus::Failed,
            "blocked" => jcode_mempalace_adapter::ActionStatus::Blocked,
            _ => jcode_mempalace_adapter::ActionStatus::Pending,
        });

        let project = input["project"].as_str();
        let coord = self.coord.read().await;
        let actions = coord.actions.list_actions(project, status)?;

        if actions.is_empty() {
            return Ok(ToolOutput::new("No actions found.".to_string()));
        }

        let mut result = format!("Found {} actions:\n\n", actions.len());
        for action in &actions {
            result.push_str(&format!(
                "- [{}] {} (status={:?}, priority={})\n",
                action.id, action.title, action.status, action.priority
            ));
        }

        Ok(ToolOutput::new(result))
    }
}

// ============================================================================
// shared_memory_delete
// ============================================================================

pub struct SharedMemoryDeleteTool {
    coord: SharedCoordination,
}

impl SharedMemoryDeleteTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for SharedMemoryDeleteTool {
    fn name(&self) -> &str {
        "shared_memory_delete"
    }

    fn description(&self) -> &str {
        "Release a file reservation or update an action status."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reservation_id": {
                    "type": "string",
                    "description": "File reservation ID to release"
                },
                "action_id": {
                    "type": "string",
                    "description": "Action ID to update"
                },
                "action_status": {
                    "type": "string",
                    "enum": ["completed", "failed", "cancelled"],
                    "description": "New action status"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let coord = self.coord.read().await;

        if let Some(reservation_id) = input["reservation_id"].as_str() {
            coord.release_file(reservation_id)?;
            return Ok(ToolOutput::new(format!(
                "Reservation {} released",
                reservation_id
            )));
        }

        if let Some(action_id) = input["action_id"].as_str() {
            if let Some(status_str) = input["action_status"].as_str() {
                let status = match status_str {
                    "completed" => jcode_mempalace_adapter::ActionStatus::Completed,
                    "failed" => jcode_mempalace_adapter::ActionStatus::Failed,
                    "cancelled" => jcode_mempalace_adapter::ActionStatus::Cancelled,
                    _ => return Ok(ToolOutput::new("Invalid action_status")),
                };
                coord.update_action_status(action_id, status)?;
                return Ok(ToolOutput::new(format!(
                    "Action {} updated to {:?}",
                    action_id, status
                )));
            }
        }

        Ok(ToolOutput::new(
            "Provide reservation_id or action_id with action_status",
        ))
    }
}

// ============================================================================
// shared_memory_cleanup
// ============================================================================

pub struct SharedMemoryCleanupTool {
    coord: SharedCoordination,
}

impl SharedMemoryCleanupTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for SharedMemoryCleanupTool {
    fn name(&self) -> &str {
        "shared_memory_cleanup"
    }

    fn description(&self) -> &str {
        "Cleanup expired file reservations."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let coord = self.coord.read().await;
        let cleaned = coord.reservations.cleanup()?;
        Ok(ToolOutput::new(format!(
            "Cleaned up {} expired reservations",
            cleaned
        )))
    }
}

// ============================================================================
// file_reserve
// ============================================================================

pub struct FileReserveTool {
    coord: SharedCoordination,
}

impl FileReserveTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for FileReserveTool {
    fn name(&self) -> &str {
        "file_reserve"
    }

    fn description(&self) -> &str {
        "Reserve a file for exclusive or shared access. Prevents other agents from conflicting edits."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path_pattern": {
                    "type": "string",
                    "description": "File path or glob pattern (e.g., 'src/auth/*.rs')"
                },
                "mode": {
                    "type": "string",
                    "enum": ["exclusive", "shared"],
                    "default": "exclusive",
                    "description": "Reservation mode"
                },
                "reason": {
                    "type": "string",
                    "description": "Reason for reservation"
                },
                "ttl_minutes": {
                    "type": "integer",
                    "default": 10,
                    "description": "Time to live in minutes (max 60)"
                }
            },
            "required": ["path_pattern"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let path_pattern = input["path_pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path_pattern'"))?;
        let mode = match input["mode"].as_str().unwrap_or("exclusive") {
            "shared" => jcode_mempalace_adapter::ReservationMode::Shared,
            _ => jcode_mempalace_adapter::ReservationMode::Exclusive,
        };
        let reason = input["reason"].as_str();
        let ttl = input["ttl_minutes"].as_i64().unwrap_or(10);

        let coord = self.coord.read().await;
        let reservation = coord.reserve_file(path_pattern, &ctx.session_id, mode, reason, ttl)?;

        Ok(ToolOutput::new(format!(
            "File reserved: {} (id={}, mode={:?}, expires={})",
            reservation.path_pattern, reservation.id, reservation.mode, reservation.expires_at
        )))
    }
}

// ============================================================================
// file_conflicts
// ============================================================================

pub struct FileConflictsTool {
    coord: SharedCoordination,
}

impl FileConflictsTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for FileConflictsTool {
    fn name(&self) -> &str {
        "file_conflicts"
    }

    fn description(&self) -> &str {
        "Check for file reservation conflicts before editing."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path_pattern": {
                    "type": "string",
                    "description": "File path to check"
                },
                "mode": {
                    "type": "string",
                    "enum": ["exclusive", "shared"],
                    "default": "exclusive"
                }
            },
            "required": ["path_pattern"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let path_pattern = input["path_pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path_pattern'"))?;
        let mode = match input["mode"].as_str().unwrap_or("exclusive") {
            "shared" => jcode_mempalace_adapter::ReservationMode::Shared,
            _ => jcode_mempalace_adapter::ReservationMode::Exclusive,
        };

        let coord = self.coord.read().await;
        let conflict = coord.check_file_conflict(path_pattern, &ctx.session_id, mode)?;

        match conflict {
            jcode_mempalace_adapter::ReservationConflict::None => {
                Ok(ToolOutput::new("No conflict. Safe to edit.".to_string()))
            }
            jcode_mempalace_adapter::ReservationConflict::SameAgent => Ok(ToolOutput::new(
                "You already hold a reservation for this file.".to_string(),
            )),
            other => Ok(ToolOutput::new(format!("Conflict detected: {:?}", other))),
        }
    }
}

// ============================================================================
// saturation_check
// ============================================================================

pub struct SaturationCheckTool {
    coord: SharedCoordination,
}

impl SaturationCheckTool {
    pub fn new(coord: SharedCoordination) -> Self {
        Self { coord }
    }
}

#[async_trait]
impl Tool for SaturationCheckTool {
    fn name(&self) -> &str {
        "saturation_check"
    }

    fn description(&self) -> &str {
        "Check for coordination saturation signals (duplicate work, stale threads, repeated blockers)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value, _ctx: ToolContext) -> Result<ToolOutput> {
        let coord = self.coord.read().await;
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;

        // Create sample events from recent signals
        // In a real implementation, this would read from the event log
        let events = vec![];
        let report = coord.check_saturation(&events, now_ms);

        if !report.saturated {
            return Ok(ToolOutput::new(
                "No saturation signals detected. Coordination is healthy.".to_string(),
            ));
        }

        let mut result = format!("Saturation detected! {} signals:\n\n", report.signals.len());
        for evidence in &report.signals {
            result.push_str(&format!(
                "- {:?}: {} occurrences (threshold: {})\n",
                evidence.signal, evidence.count, evidence.threshold
            ));
            for detail in &evidence.details {
                result.push_str(&format!("  {}\n", detail));
            }
        }

        if !report.recommendations.is_empty() {
            result.push_str("\nRecommendations:\n");
            for rec in &report.recommendations {
                result.push_str(&format!(
                    "- {:?}: switch to {} (confidence: {})\n",
                    rec.signal, rec.recommended_skill, rec.confidence
                ));
            }
        }

        Ok(ToolOutput::new(result))
    }
}

// ============================================================================
// Tool registration
// ============================================================================

/// Register all coordination tools.
pub async fn register_coordination_tools(
    registry: &crate::tool::Registry,
    coord: SharedCoordination,
) {
    registry
        .register(
            "shared_memory_write".to_string(),
            Arc::new(SharedMemoryWriteTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;

    registry
        .register(
            "shared_memory_read".to_string(),
            Arc::new(SharedMemoryReadTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;

    registry
        .register(
            "shared_memory_list".to_string(),
            Arc::new(SharedMemoryListTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;

    registry
        .register(
            "shared_memory_delete".to_string(),
            Arc::new(SharedMemoryDeleteTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;

    registry
        .register(
            "shared_memory_cleanup".to_string(),
            Arc::new(SharedMemoryCleanupTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;

    registry
        .register(
            "file_reserve".to_string(),
            Arc::new(FileReserveTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;

    registry
        .register(
            "file_conflicts".to_string(),
            Arc::new(FileConflictsTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;

    registry
        .register(
            "saturation_check".to_string(),
            Arc::new(SaturationCheckTool::new(coord.clone())) as Arc<dyn Tool>,
        )
        .await;
}
