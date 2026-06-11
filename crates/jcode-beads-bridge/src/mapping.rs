//! Type mappings between jcode types and beads_rust `Issue`.

use beads_rust::model::{Issue, IssueType, Priority, Status};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

// ─── Jcode types (stand-ins until dead crates are removed) ──────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    pub priority: String,
    pub id: String,
    pub group: Option<String>,
    pub confidence: Option<u8>,
    pub completion_confidence: Option<u8>,
    pub blocked_by: Vec<String>,
    pub assigned_to: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub title: String,
    pub scope: String,
    pub status: String,
    pub description: String,
    pub why: String,
    pub milestones: Vec<GoalMilestone>,
    pub next_steps: Vec<String>,
    pub blockers: Vec<String>,
    pub progress_percent: Option<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalMilestone {
    pub id: String,
    pub title: String,
    pub status: String,
    pub steps: Vec<GoalStep>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalStep {
    pub id: String,
    pub content: String,
    pub status: String,
}

// ─── Issue → TodoItem ──────────────────────────────────────────────────────

impl From<Issue> for TodoItem {
    fn from(issue: Issue) -> Self {
        TodoItem {
            id: issue.id,
            content: issue.title,
            status: issue.status.as_str().to_string(),
            priority: issue.priority.to_string(),
            group: issue.labels.first().cloned(),
            confidence: None,
            completion_confidence: None,
            blocked_by: Vec::new(),
            assigned_to: issue.assignee,
        }
    }
}

// ─── Issue → Goal (when issue_type == Epic) ────────────────────────────────

impl From<Issue> for Goal {
    fn from(issue: Issue) -> Self {
        let scope = if issue.labels.contains(&"global".to_string()) {
            "global".to_string()
        } else {
            "project".to_string()
        };
        Goal {
            id: issue.id,
            title: issue.title,
            scope,
            status: issue.status.as_str().to_string(),
            description: issue.description.unwrap_or_default(),
            why: String::new(),
            milestones: Vec::new(),
            next_steps: Vec::new(),
            blockers: Vec::new(),
            progress_percent: None,
            created_at: issue.created_at,
            updated_at: issue.updated_at,
        }
    }
}

// ─── TodoItem → Issue ──────────────────────────────────────────────────────

pub trait ToBeadsIssue {
    fn to_issue(&self) -> Issue;
}

impl ToBeadsIssue for TodoItem {
    fn to_issue(&self) -> Issue {
        let status = Status::from_str(&self.status).unwrap_or(Status::Open);
        let priority = match self.priority.to_lowercase().as_str() {
            "critical" | "p0" => Priority::CRITICAL,
            "high" | "p1" => Priority::HIGH,
            "medium" | "p2" => Priority::MEDIUM,
            "low" | "p3" => Priority::LOW,
            "backlog" | "p4" => Priority::BACKLOG,
            _ => Priority::MEDIUM,
        };
        let mut labels: Vec<String> = Vec::new();
        if let Some(g) = self.group.as_deref().filter(|g| !g.is_empty()) {
            labels.push(g.to_string());
        }
        Issue {
            id: self.id.clone(),
            content_hash: None,
            title: self.content.clone(),
            description: None,
            design: None,
            acceptance_criteria: None,
            notes: None,
            status,
            priority,
            issue_type: IssueType::Task,
            assignee: self.assigned_to.clone(),
            owner: None,
            estimated_minutes: None,
            created_at: Utc::now(),
            created_by: None,
            updated_at: Utc::now(),
            closed_at: None,
            close_reason: None,
            closed_by_session: None,
            due_at: None,
            defer_until: None,
            external_ref: None,
            source_system: Some("jcode".to_string()),
            source_repo: None,
            source_repo_path: None,
            agent_context: None,
            labels,
            deleted_at: None,
            deleted_by: None,
            delete_reason: None,
            original_type: None,
            compaction_level: None,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: None,
            sender: None,
            ephemeral: false,
            pinned: false,
            is_template: false,
            dependencies: Vec::new(),
            comments: Vec::new(),
        }
    }
}

/// Trait for constructing Epic-type issues from Goal data.
pub trait ToBeadsEpic {
    fn to_epic(&self) -> Issue;
}

impl ToBeadsEpic for Goal {
    fn to_epic(&self) -> Issue {
        let status = Status::from_str(&self.status).unwrap_or(Status::Open);
        let mut labels: Vec<String> = Vec::new();
        if self.scope == "global" {
            labels.push("global".to_string());
        }
        Issue {
            id: self.id.clone(),
            content_hash: None,
            title: self.title.clone(),
            description: Some(self.description.clone()),
            design: None,
            acceptance_criteria: None,
            notes: None,
            status,
            priority: Priority::MEDIUM,
            issue_type: IssueType::Epic,
            assignee: None,
            owner: None,
            estimated_minutes: None,
            created_at: Utc::now(),
            created_by: None,
            updated_at: Utc::now(),
            closed_at: None,
            close_reason: None,
            closed_by_session: None,
            due_at: None,
            defer_until: None,
            external_ref: None,
            source_system: Some("jcode".to_string()),
            source_repo: None,
            source_repo_path: None,
            agent_context: None,
            labels,
            deleted_at: None,
            deleted_by: None,
            delete_reason: None,
            original_type: None,
            compaction_level: None,
            compacted_at: None,
            compacted_at_commit: None,
            original_size: None,
            sender: None,
            ephemeral: false,
            pinned: false,
            is_template: false,
            dependencies: Vec::new(),
            comments: Vec::new(),
        }
    }
}

// ─── Reverse conversions ───────────────────────────────────────────────────

pub trait ToJcodeTodoItem {
    fn to_todo_item(&self) -> TodoItem;
}

impl ToJcodeTodoItem for Issue {
    fn to_todo_item(&self) -> TodoItem {
        self.clone().into()
    }
}

pub trait ToJcodeGoal {
    fn to_goal(&self) -> Goal;
}

impl ToJcodeGoal for Issue {
    fn to_goal(&self) -> Goal {
        self.clone().into()
    }
}
