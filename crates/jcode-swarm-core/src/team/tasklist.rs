//! Dependency-aware task board — port of `team-tasklist/*`.
//!
//! Tasks are JSON files under `runtime/{run}/tasks/`, ids come from a
//! high-watermark counter, and claims are guarded by per-task lockfiles.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::team::locks::{atomic_write, read_json, with_lock};
use crate::team::paths::{tasks_dir, validate_member_name};
use crate::team::spec::*;

pub struct NewTask {
    pub subject: String,
    pub description: String,
    pub blocks: Vec<String>,
    pub blocked_by: Vec<String>,
}

/// Create a task using a high-watermark counter under the tasks-dir lock
/// (port of store.ts).
pub fn create_task(run_id: &str, input: NewTask) -> TeamResult<TeamTask> {
    let dir = tasks_dir(run_id);
    fs::create_dir_all(dir.join("claims"))?;
    let lock = dir.join(".lock");
    with_lock(&lock, &format!("create-task:{run_id}"), || {
        let wm_path = dir.join(".highwatermark");
        let next = read_high_watermark(&wm_path)? + 1;
        atomic_write(&wm_path, &next.to_string())?;
        let now = now_millis();
        let task = TeamTask {
            version: 1,
            id: next.to_string(),
            subject: input.subject,
            description: input.description,
            active_form: None,
            status: TaskStatus::Pending,
            owner: None,
            blocks: input.blocks,
            blocked_by: input.blocked_by,
            created_at: now,
            updated_at: now,
            claimed_at: None,
        };
        atomic_write(
            &dir.join(format!("{}.json", task.id)),
            &format!("{}\n", serde_json::to_string_pretty(&task)?),
        )?;
        Ok(task)
    })
}

/// Read the high-watermark counter, distinguishing "missing" from "corrupt".
///
/// Missing → returns 0 (caller writes "1" on first task).
/// Corrupt (truncated, non-numeric, missing) → returns `Task` error so the
/// caller does NOT silently reuse task id 1 when `1.json` already exists.
/// The reference port's silent fallback to 0 was the source of a class of
/// "phantom task id" bugs we want to avoid in the Rust port.
fn read_high_watermark(path: &Path) -> TeamResult<u64> {
    match fs::read_to_string(path) {
        Ok(content) => content
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n < u64::MAX)
            .ok_or_else(|| {
                TeamError::Task(format!(
                    ".highwatermark at {} is corrupt (contents: {:?}); refusing to \
                     silently reuse task ids. Delete the file and re-create tasks, or \
                     run a migrator.",
                    path.display(),
                    content
                ))
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(TeamError::Io(e)),
    }
}

/// Atomically claim a pending task; fails if already claimed or blocked
/// (port of claim.ts).
pub fn claim_task(run_id: &str, task_id: &str, member: &str) -> TeamResult<TeamTask> {
    validate_member_name(member)?;
    let dir = tasks_dir(run_id);
    fs::create_dir_all(dir.join("claims"))?;
    let claim_lock = dir.join("claims").join(format!("{task_id}.lock"));
    with_lock(&claim_lock, &format!("claim:{task_id}"), || {
        let path = dir.join(format!("{task_id}.json"));
        let mut task: TeamTask = read_json(&path)?;
        if task.status != TaskStatus::Pending {
            return Err(TeamError::Task(format!("task {task_id} is not claimable")));
        }
        for dep in &task.blocked_by {
            let dep_task: TeamTask = read_json(&dir.join(format!("{dep}.json")))?;
            if dep_task.status != TaskStatus::Completed {
                return Err(TeamError::Task(format!(
                    "task {task_id} is blocked by incomplete task {dep}"
                )));
            }
        }
        let now = now_millis();
        task.status = TaskStatus::Claimed;
        task.owner = Some(member.to_string());
        task.claimed_at = Some(now);
        task.updated_at = now;
        atomic_write(
            &path,
            &format!("{}\n", serde_json::to_string_pretty(&task)?),
        )?;
        Ok(task)
    })
}

/// Apply a validated status transition (port of update.ts).
/// Uses the per-task claim lock for atomic read-modify-write.
pub fn update_status(run_id: &str, task_id: &str, next: TaskStatus) -> TeamResult<TeamTask> {
    let dir = tasks_dir(run_id);
    fs::create_dir_all(dir.join("claims"))?;
    let claim_lock = dir.join("claims").join(format!("{task_id}.lock"));
    with_lock(&claim_lock, &format!("update-status:{task_id}"), || {
        let path = dir.join(format!("{task_id}.json"));
        let mut task: TeamTask = read_json(&path)?;
        if !valid_transition(task.status, next) {
            return Err(TeamError::Task(format!(
                "invalid transition {:?} -> {:?}",
                task.status, next
            )));
        }
        task.status = next;
        task.updated_at = now_millis();
        atomic_write(
            &path,
            &format!("{}\n", serde_json::to_string_pretty(&task)?),
        )?;
        Ok(task)
    })
}

fn valid_transition(from: TaskStatus, to: TaskStatus) -> bool {
    use TaskStatus::*;
    matches!(
        (from, to),
        (Pending, Claimed)
            | (Claimed, InProgress)
            | (InProgress, Completed)
            | (Claimed, Pending) // release a claim
            | (Completed, Deleted)
            | (Pending, Deleted)
    )
}

/// List tasks, optionally filtered by status and/or owner; sorted by numeric id.
pub fn list_tasks(
    run_id: &str,
    status: Option<TaskStatus>,
    owner: Option<&str>,
) -> TeamResult<Vec<TeamTask>> {
    let dir = tasks_dir(run_id);
    let mut out = Vec::new();
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(TeamError::Io(e)),
    };
    for entry in rd.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.ends_with(".json") {
            continue;
        }
        if let Ok(task) = read_json::<TeamTask>(&entry.path()) {
            let status_ok = status.map(|s| s == task.status).unwrap_or(true);
            let owner_ok = owner
                .map(|o| task.owner.as_deref() == Some(o))
                .unwrap_or(true);
            if status_ok && owner_ok {
                out.push(task);
            }
        }
    }
    out.sort_by(|a, b| {
        a.id.parse::<u64>()
            .unwrap_or(0)
            .cmp(&b.id.parse::<u64>().unwrap_or(0))
    });
    Ok(out)
}

/// Transitive blockers of a task (port of dependencies.ts); cycle-safe.
pub fn transitive_blockers(run_id: &str, task_id: &str) -> TeamResult<Vec<String>> {
    let dir = tasks_dir(run_id);
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack = vec![task_id.to_string()];
    let mut order = Vec::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue; // cycle guard
        }
        let task: TeamTask = read_json(&dir.join(format!("{id}.json")))?;
        for dep in task.blocked_by {
            if dep != task_id {
                order.push(dep.clone());
            }
            stack.push(dep);
        }
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_task(subject: &str, blocked_by: Vec<String>) -> NewTask {
        NewTask {
            subject: subject.into(),
            description: String::new(),
            blocks: vec![],
            blocked_by,
        }
    }

    #[test]
    fn create_assigns_incrementing_ids() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let t1 = create_task(&run, new_task("a", vec![])).unwrap();
        let t2 = create_task(&run, new_task("b", vec![])).unwrap();
        assert_eq!(t1.id, "1");
        assert_eq!(t2.id, "2");
    }

    #[test]
    fn claim_blocked_task_fails_until_dependency_completed() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let dep = create_task(&run, new_task("dep", vec![])).unwrap();
        let blocked = create_task(&run, new_task("main", vec![dep.id.clone()])).unwrap();

        assert!(claim_task(&run, &blocked.id, "w").is_err());

        claim_task(&run, &dep.id, "w").unwrap();
        update_status(&run, &dep.id, TaskStatus::InProgress).unwrap();
        update_status(&run, &dep.id, TaskStatus::Completed).unwrap();

        let claimed = claim_task(&run, &blocked.id, "w").unwrap();
        assert_eq!(claimed.status, TaskStatus::Claimed);
        assert_eq!(claimed.owner.as_deref(), Some("w"));
    }

    #[test]
    fn double_claim_rejected() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let t = create_task(&run, new_task("x", vec![])).unwrap();
        claim_task(&run, &t.id, "a").unwrap();
        assert!(claim_task(&run, &t.id, "b").is_err());
    }

    #[test]
    fn invalid_transition_rejected() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let t = create_task(&run, new_task("x", vec![])).unwrap();
        // Pending -> Completed skips Claimed/InProgress.
        assert!(update_status(&run, &t.id, TaskStatus::Completed).is_err());
    }

    #[test]
    fn list_filters_by_status_and_owner() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        let a = create_task(&run, new_task("a", vec![])).unwrap();
        let _b = create_task(&run, new_task("b", vec![])).unwrap();
        claim_task(&run, &a.id, "alice").unwrap();
        assert_eq!(
            list_tasks(&run, Some(TaskStatus::Claimed), None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(list_tasks(&run, None, Some("alice")).unwrap().len(), 1);
        assert_eq!(
            list_tasks(&run, Some(TaskStatus::Pending), None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(list_tasks(&run, None, None).unwrap().len(), 2);
    }

    #[test]
    fn transitive_blockers_handles_cycle() {
        let base = crate::team::test_support::guarded_base();
        let run = base.run_id();
        // task 1 blocked_by 2, task 2 blocked_by 1 (cycle) — must terminate.
        let a = create_task(&run, new_task("a", vec!["2".into()])).unwrap();
        let _b = create_task(&run, new_task("b", vec!["1".into()])).unwrap();
        let deps = transitive_blockers(&run, &a.id).unwrap();
        assert!(deps.contains(&"2".to_string()));
    }
}
