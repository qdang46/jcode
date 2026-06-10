//! Integration tests for the beads_rust bridge.
//!
//! Uses in-memory SQLite to test the facade without real filesystem I/O.
use crate::mapping::{TodoItem, Goal, ToBeadsIssue, ToBeadsEpic, ToJcodeGoal};

use crate::BeadsProject;
use beads_rust::model::{Status, Priority, IssueType};
use chrono::Utc;

fn temp_project(prefix: &str) -> BeadsProject {
    // Resolve to a real (non-symlink) path for beads_rust's strict path validation.
    let base = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join(format!(".beads-test-{}-{}", std::process::id(), rand_id()));
    let _ = std::fs::remove_dir_all(&dir);
    BeadsProject::init(&dir, prefix).expect("init should succeed")
}

fn rand_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64
}

#[test]
fn test_beads_project_init_open_flush() {
    let project = temp_project("test");
    assert!(project.beads_dir().join("beads.db").exists());
    assert!(project.beads_dir().join("config.yaml").exists());
    // flush may fail in test on clean schema; that is ok.
    let _ = project.flush();
    let _ = std::fs::remove_dir_all(project.beads_dir());
}

#[test]
fn test_beads_open_or_init() {
    let base = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join(format!(".beads-test-open-{}-{}", std::process::id(), rand_id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Should init since no project exists
    let p1 = BeadsProject::open_or_init(&dir, "test").expect("open_or_init should succeed");
    assert!(p1.beads_dir().exists());
    // Should open since project now exists
    let p2 = BeadsProject::open_or_init(&dir, "test").expect("second open_or_init should succeed");
    assert_eq!(p1.beads_dir(), p2.beads_dir());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_create_and_list_task() {
    let project = temp_project("test");
    let manager = crate::BeadsTaskManager::new(&project);

    let task = manager.create_task("Test task", Priority::HIGH, &["bug".to_string()])
        .expect("create should succeed");
    assert_eq!(task.title, "Test task");
    assert_eq!(task.status, Status::Open);
    assert!(task.labels.contains(&"bug".to_string()));
    assert_eq!(task.priority, Priority::HIGH);

    let tasks = manager.list_open_tasks().expect("list should succeed");
    assert!(tasks.iter().any(|t| t.id == task.id));
    let _ = std::fs::remove_dir_all(project.beads_dir());
}

#[test]
fn test_create_todo_from_item() {
    let project = temp_project("test");
    let manager = crate::BeadsTaskManager::new(&project);

    let item = TodoItem {
        id: "test-001".to_string(),
        content: "A todo item".to_string(),
        status: "open".to_string(),
        priority: "p1".to_string(),
        group: Some("backend".to_string()),
        confidence: None,
        completion_confidence: None,
        blocked_by: vec![],
        assigned_to: Some("agent".to_string()),
    };

    let issue = manager.create_todo(&item).expect("create_todo should succeed");
    assert_eq!(issue.title, "A todo item");
    assert_eq!(issue.status, Status::Open);
    assert_eq!(issue.priority, Priority::HIGH);
    assert!(issue.labels.contains(&"backend".to_string()));
    assert_eq!(issue.assignee, Some("agent".to_string()));

    let _ = std::fs::remove_dir_all(project.beads_dir());
}

#[test]
fn test_set_status_and_close() {
    let project = temp_project("test");
    let manager = crate::BeadsTaskManager::new(&project);

    let task = manager.create_task("Status test", Priority::MEDIUM, &[])
        .expect("create should succeed");

    let claimed = manager.set_status(&task.id, Status::InProgress, "tester")
        .expect("set_status should succeed");
    assert_eq!(claimed.status, Status::InProgress);

    let closed = manager.close_task(&task.id, "Done", "tester")
        .expect("close should succeed");
    assert_eq!(closed.status, Status::Closed);

    let _ = std::fs::remove_dir_all(project.beads_dir());
}

#[test]
fn test_ready_tasks() {
    let project = temp_project("test");
    let manager = crate::BeadsTaskManager::new(&project);

    let _t1 = manager.create_task("Ready task", Priority::HIGH, &[])
        .expect("create should succeed");
    // Ready tasks need no blockers → empty graph = all ready
    let ready = manager.ready_tasks(10).expect("ready_tasks should succeed");
    assert!(!ready.is_empty(), "should have at least one ready task");
    assert!(ready.iter().any(|t| t.title == "Ready task"));

    let _ = std::fs::remove_dir_all(project.beads_dir());
}

#[test]
fn test_mapping_todo_to_issue_roundtrip() {
    let item = TodoItem {
        id: "rt-001".to_string(),
        content: "Roundtrip".to_string(),
        status: "in_progress".to_string(),
        priority: "p2".to_string(),
        group: None,
        confidence: None,
        completion_confidence: None,
        blocked_by: vec![],
        assigned_to: None,
    };

    let issue = item.to_issue();
    assert_eq!(issue.title, "Roundtrip");
    assert_eq!(issue.status, Status::InProgress);
    assert_eq!(issue.priority, Priority::MEDIUM);

    let back: TodoItem = issue.into();
    assert_eq!(back.content, "Roundtrip");
    assert_eq!(back.status, "in_progress");
}

#[test]
fn test_mapping_goal_to_epic_roundtrip() {
    let goal = Goal {
        id: "epic-001".to_string(),
        title: "Big feature".to_string(),
        scope: "project".to_string(),
        status: "active".to_string(),
        description: "Build the thing".to_string(),
        why: String::new(),
        milestones: vec![],
        next_steps: vec![],
        blockers: vec![],
        progress_percent: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let issue = goal.to_epic();
    assert_eq!(issue.title, "Big feature");
    assert_eq!(issue.issue_type, IssueType::Epic);
    assert_eq!(issue.description, Some("Build the thing".to_string()));

    let back = issue.to_goal();
    assert_eq!(back.title, "Big feature");
    assert_eq!(back.description, "Build the thing");
}

#[test]
fn test_dependency_cycle_detection() {
    let project = temp_project("test");
    let manager = crate::BeadsTaskManager::new(&project);

    let a = manager.create_task("Task A", Priority::MEDIUM, &[])
        .expect("create A");
    let b = manager.create_task("Task B", Priority::MEDIUM, &[])
        .expect("create B");

    // A blocks on B
    manager.add_dependency(&a.id, &b.id, "tester")
        .expect("add dep A->B should succeed");

    // B blocking on A would create a cycle
    let err = manager.add_dependency(&b.id, &a.id, "tester");
    assert!(err.is_err(), "cycle should be rejected");
    assert!(err.unwrap_err().to_string().contains("cycle"));

    let blockers = manager.blockers(&a.id).expect("blockers should succeed");
    assert!(blockers.contains(&b.id));

    let _ = std::fs::remove_dir_all(project.beads_dir());
}
