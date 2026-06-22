//! Goal management backed by local JSON files under `~/.jcode/goals/`.
//!
//! Goals live either in the global store (`~/.jcode/goals/`) or in a
//! project-local store (`<working_dir>/.jcode/goals/`). Per-session
//! attachments are tracked in `~/.jcode/goals/attachments/<session>.json`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::side_panel::{SidePanelSnapshot, focus_page, snapshot_for_session, write_markdown_page};
use crate::storage::{self, read_json, write_json_fast};

// Re-export task-types so existing callers can keep referring to the same
// type names (matches upstream `jcode-base::goal` surface).
pub use jcode_task_types::{Goal, GoalMilestone, GoalScope, GoalStatus, GoalStep, GoalUpdate};

/// Goal display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalDisplayMode {
    Auto,
    Focus,
    UpdateOnly,
    None,
}

impl GoalDisplayMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "focus" => Some(Self::Focus),
            "update_only" => Some(Self::UpdateOnly),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// Goal creation input.
#[derive(Debug, Clone, Default)]
pub struct GoalCreateInput {
    pub id: Option<String>,
    pub title: String,
    pub scope: GoalScope,
    pub description: Option<String>,
    pub why: Option<String>,
    pub success_criteria: Vec<String>,
    pub milestones: Vec<GoalMilestone>,
    pub next_steps: Vec<String>,
    pub blockers: Vec<String>,
    pub current_milestone_id: Option<String>,
    pub progress_percent: Option<u8>,
}

/// Goal update input.
#[derive(Debug, Clone, Default)]
pub struct GoalUpdateInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub why: Option<String>,
    pub status: Option<GoalStatus>,
    pub success_criteria: Option<Vec<String>>,
    pub milestones: Option<Vec<GoalMilestone>>,
    pub next_steps: Option<Vec<String>>,
    pub blockers: Option<Vec<String>>,
    pub current_milestone_id: Option<Option<String>>,
    pub progress_percent: Option<Option<u8>>,
    pub checkpoint_summary: Option<String>,
}

/// Display result for a goal.
#[derive(Debug, Clone)]
pub struct GoalDisplayResult {
    pub goal: Goal,
    pub snapshot: SidePanelSnapshot,
}

// ─── File-backed operations (replaces beads-backed storage) ───────────────

fn global_goals_dir() -> Result<PathBuf> {
    Ok(storage::jcode_dir()?.join("goals"))
}

fn project_goals_dir(working_dir: Option<&Path>) -> Result<Option<PathBuf>> {
    let Some(dir) = working_dir else {
        return Ok(None);
    };
    Ok(Some(dir.join(".jcode").join("goals")))
}

fn ensure_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create goals dir {}", dir.display()))?;
    }
    Ok(())
}

fn goal_file_in_dir(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{}.json", jcode_task_types::sanitize_goal_id(id)))
}

fn load_goals_in_dir(dir: &Path) -> Result<Vec<Goal>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut goals = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match read_json::<Goal>(&path) {
            Ok(goal) => goals.push(goal),
            Err(err) => {
                crate::logging::warn(&format!(
                    "skip unreadable goal {}: {err}",
                    path.display()
                ));
            }
        }
    }
    sort_goals(&mut goals);
    Ok(goals)
}

fn save_goal(goal: &Goal, working_dir: Option<&Path>) -> Result<()> {
    let dir = match goal.scope {
        GoalScope::Global => global_goals_dir()?,
        GoalScope::Project => project_goals_dir(working_dir)?
            .ok_or_else(|| anyhow::anyhow!("working_dir required for project goals"))?,
    };
    ensure_dir(&dir)?;
    let path = goal_file_in_dir(&dir, &goal.id);
    write_json_fast(&path, goal)
}

fn sort_goals(goals: &mut [Goal]) {
    goals.sort_by(|a, b| {
        a.status
            .sort_rank()
            .cmp(&b.status.sort_rank())
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn session_attachment_path(session_id: &str) -> Result<PathBuf> {
    Ok(storage::jcode_dir()?
        .join("goals")
        .join("attachments")
        .join(format!("{}.json", session_id)))
}

fn project_hash(working_dir: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    working_dir.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn next_available_goal_id(seed: &str, scope: GoalScope, working_dir: Option<&Path>) -> Result<String> {
    let taken: HashSet<String> = match scope {
        GoalScope::Global => load_goals_in_dir(&global_goals_dir()?)?
            .into_iter()
            .map(|g| g.id)
            .collect(),
        GoalScope::Project => match project_goals_dir(working_dir)? {
            Some(dir) => load_goals_in_dir(&dir)?.into_iter().map(|g| g.id).collect(),
            None => HashSet::new(),
        },
    };
    let base = jcode_task_types::sanitize_goal_id(seed);
    if !taken.contains(&base) {
        return Ok(base);
    }
    for n in 2..=9999 {
        let candidate = format!("{base}-{n}");
        if !taken.contains(&candidate) {
            return Ok(candidate);
        }
    }
    anyhow::bail!("could not allocate goal id for seed {seed}");
}

/// Create a new goal and persist it.
pub fn create_goal(input: GoalCreateInput, working_dir: Option<&Path>) -> Result<Goal> {
    if input.title.trim().is_empty() {
        anyhow::bail!("goal title cannot be empty");
    }
    let now = Utc::now();
    let scope = input.scope;
    let mut goal = Goal {
        id: input
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(jcode_task_types::sanitize_goal_id)
            .unwrap_or_else(|| jcode_task_types::sanitize_goal_id(&input.title)),
        title: input.title.trim().to_string(),
        scope,
        status: GoalStatus::default(),
        description: input.description.unwrap_or_default().trim().to_string(),
        why: input.why.unwrap_or_default().trim().to_string(),
        success_criteria: input.success_criteria,
        milestones: input.milestones,
        next_steps: input.next_steps,
        blockers: input.blockers,
        current_milestone_id: input.current_milestone_id,
        progress_percent: input.progress_percent.map(|p| p.min(100)),
        created_at: now,
        updated_at: now,
        updates: Vec::new(),
    };
    goal.id = next_available_goal_id(&goal.id, scope, working_dir)?;
    save_goal(&goal, working_dir)?;
    Ok(goal)
}

/// Update a goal and persist it.
pub fn update_goal(
    id: &str,
    scope_hint: Option<GoalScope>,
    working_dir: Option<&Path>,
    input: GoalUpdateInput,
) -> Result<Option<Goal>> {
    let Some(mut goal) = load_goal(id, scope_hint, working_dir)? else {
        return Ok(None);
    };
    if let Some(title) = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        goal.title = title.to_string();
    }
    if let Some(description) = input.description {
        goal.description = description.trim().to_string();
    }
    if let Some(why) = input.why {
        goal.why = why.trim().to_string();
    }
    if let Some(status) = input.status {
        goal.status = status;
    }
    if let Some(criteria) = input.success_criteria {
        goal.success_criteria = criteria;
    }
    if let Some(milestones) = input.milestones {
        goal.milestones = milestones;
    }
    if let Some(next_steps) = input.next_steps {
        goal.next_steps = next_steps;
    }
    if let Some(blockers) = input.blockers {
        goal.blockers = blockers;
    }
    if let Some(current_milestone_id) = input.current_milestone_id {
        goal.current_milestone_id = current_milestone_id.map(|s| s.trim().to_string());
    }
    if let Some(progress) = input.progress_percent {
        goal.progress_percent = progress.map(|p| p.min(100));
    }
    if let Some(summary) = input
        .checkpoint_summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        goal.updates.push(GoalUpdate {
            at: Utc::now(),
            summary: summary.to_string(),
        });
    }
    goal.updated_at = Utc::now();
    save_goal(&goal, working_dir)?;
    Ok(Some(goal))
}

/// Load a single goal by id, looking in project then global stores.
pub fn load_goal(
    id: &str,
    scope_hint: Option<GoalScope>,
    working_dir: Option<&Path>,
) -> Result<Option<Goal>> {
    let id = jcode_task_types::sanitize_goal_id(id);
    let mut candidates = Vec::new();
    match scope_hint {
        Some(GoalScope::Global) => candidates.push(goal_file_in_dir(&global_goals_dir()?, &id)),
        Some(GoalScope::Project) => {
            if let Some(dir) = project_goals_dir(working_dir)? {
                candidates.push(goal_file_in_dir(&dir, &id));
            }
        }
        None => {
            if let Some(dir) = project_goals_dir(working_dir)? {
                candidates.push(goal_file_in_dir(&dir, &id));
            }
            candidates.push(goal_file_in_dir(&global_goals_dir()?, &id));
        }
    }
    for path in candidates {
        if path.exists() {
            let goal: Goal = read_json(&path)
                .with_context(|| format!("failed to read goal {}", path.display()))?;
            return Ok(Some(goal));
        }
    }
    Ok(None)
}

/// List goals from both global and project stores.
pub fn list_relevant_goals(working_dir: Option<&Path>) -> Result<Vec<Goal>> {
    let mut goals = load_goals_in_dir(&global_goals_dir()?)?
        .into_iter()
        .collect::<Vec<_>>();
    if let Some(dir) = project_goals_dir(working_dir)? {
        goals.extend(load_goals_in_dir(&dir)?);
    }
    sort_goals(&mut goals);
    Ok(goals)
}

/// Resume a goal for a session: prefer an explicit attachment, otherwise the
/// first resumable goal from the relevant list.
pub fn resume_goal(session_id: &str, working_dir: Option<&Path>) -> Result<Option<Goal>> {
    if let Some(goal) = load_attached_goal(session_id, working_dir)?
        && goal.status.is_resumable()
    {
        return Ok(Some(goal));
    }
    let mut goals = list_relevant_goals(working_dir)?;
    goals.retain(|g| g.status.is_resumable());
    Ok(goals.into_iter().next())
}

/// Persist which goal is attached to a given session.
pub fn attach_goal_to_session(
    session_id: &str,
    goal: &Goal,
    working_dir: Option<&Path>,
) -> Result<()> {
    let path = session_attachment_path(session_id)?;
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    #[derive(Serialize, Deserialize)]
    struct Attachment {
        goal_id: String,
        scope: GoalScope,
        project_hash: Option<String>,
        attached_at: DateTime<Utc>,
    }
    let attachment = Attachment {
        goal_id: goal.id.clone(),
        scope: goal.scope,
        project_hash: if goal.scope == GoalScope::Project {
            working_dir.map(project_hash)
        } else {
            None
        },
        attached_at: Utc::now(),
    };
    write_json_fast(&path, &attachment)
}

/// Load the goal attached to a given session, if any.
pub fn load_attached_goal(session_id: &str, working_dir: Option<&Path>) -> Result<Option<Goal>> {
    let path = session_attachment_path(session_id)?;
    if !path.exists() {
        return Ok(None);
    }
    #[derive(Deserialize)]
    struct Attachment {
        goal_id: String,
        scope: GoalScope,
        project_hash: Option<String>,
    }
    let attachment: Attachment = read_json(&path)?;
    if attachment.scope == GoalScope::Project {
        let Some(dir) = working_dir else {
            return Ok(None);
        };
        if attachment.project_hash.as_deref() != Some(project_hash(dir).as_str()) {
            return Ok(None);
        }
    }
    load_goal(&attachment.goal_id, Some(attachment.scope), working_dir)
}

// ─── Side-panel / UI helpers (beads-aware) ─────────────────────────────────

/// Open goals overview in side panel.
pub fn open_goals_overview_for_session(
    session_id: &str,
    working_dir: Option<&Path>,
    focus: bool,
) -> Result<SidePanelSnapshot> {
    let goals = list_relevant_goals(working_dir)?;
    let content = format_goals_overview(&goals);
    let page_id = "goals";
    write_markdown_page(session_id, page_id, Some("Goals"), &content, focus)?;
    if focus {
        focus_page(session_id, page_id)
    } else {
        snapshot_for_session(session_id)
    }
}

/// Refresh goals overview in side panel.
pub fn refresh_goals_overview_for_session(
    session_id: &str,
    working_dir: Option<&Path>,
) -> Result<Option<SidePanelSnapshot>> {
    let snapshot = snapshot_for_session(session_id)?;
    if snapshot.pages.iter().any(|page| page.id == "goals") {
        open_goals_overview_for_session(session_id, working_dir, false).map(Some)
    } else {
        Ok(None)
    }
}

/// Write goal detail page to side panel.
pub fn write_goal_page(
    session_id: &str,
    working_dir: Option<&Path>,
    goal: &Goal,
    display: GoalDisplayMode,
) -> Result<SidePanelSnapshot> {
    let _ = (working_dir, display);
    let content = format_goal_detail(goal);
    let page_id = goal_page_id(&goal.id);
    write_markdown_page(
        session_id,
        &page_id,
        Some(&format!("Epic: {}", goal.title)),
        &content,
        true,
    )?;
    focus_page(session_id, &page_id)
}

/// Open a goal detail in the side panel.
pub fn open_goal_for_session(
    session_id: &str,
    working_dir: Option<&Path>,
    id: &str,
    explicit_focus: bool,
) -> Result<Option<GoalDisplayResult>> {
    let goal = match load_goal(id, None, working_dir)? {
        Some(g) => g,
        None => return Ok(None),
    };
    let snapshot = write_goal_page(
        session_id,
        working_dir,
        &goal,
        if explicit_focus {
            GoalDisplayMode::Focus
        } else {
            GoalDisplayMode::Auto
        },
    )?;
    Ok(Some(GoalDisplayResult { goal, snapshot }))
}

/// Resume a goal and show it in the side panel.
pub fn resume_goal_for_session(
    session_id: &str,
    working_dir: Option<&Path>,
    explicit_focus: bool,
) -> Result<Option<GoalDisplayResult>> {
    let goal = match resume_goal(session_id, working_dir)? {
        Some(g) => g,
        None => return Ok(None),
    };
    let snapshot = write_goal_page(
        session_id,
        working_dir,
        &goal,
        if explicit_focus {
            GoalDisplayMode::Focus
        } else {
            GoalDisplayMode::Auto
        },
    )?;
    Ok(Some(GoalDisplayResult { goal, snapshot }))
}

pub fn goal_page_id(id: &str) -> String {
    format!("goal-{id}")
}

pub fn header_badge(
    working_dir: Option<&Path>,
    snapshot: &crate::side_panel::SidePanelSnapshot,
) -> Option<String> {
    if let Some(page) = snapshot.focused_page()
        && page.id.starts_with("goal-")
    {
        return Some(format!("🎯 {}", page.title));
    }
    let goals = list_relevant_goals(working_dir).ok()?;
    if goals.is_empty() {
        return None;
    }
    let active = goals.iter().filter(|g| g.status.is_resumable()).count();
    if active > 0 {
        Some(format!("🎯 {} active", active))
    } else {
        Some(format!("🎯 {} goals", goals.len()))
    }
}

pub fn render_goals_overview(goals: &[Goal]) -> String {
    format_goals_overview(goals)
}

pub fn render_goal_detail(goal: &Goal) -> String {
    format_goal_detail(goal)
}

// ─── Rendering helpers ─────────────────────────────────────────────────────

fn format_goals_overview(goals: &[Goal]) -> String {
    if goals.is_empty() {
        return "# Goals\n\nNo goals found. Use `initiative` tool to create one.".to_string();
    }
    let mut html = "# Goals\n\n".to_string();
    for goal in goals {
        html.push_str(&format!(
            "- **{}** [{}] — {}\n",
            goal.title,
            goal.status.as_str(),
            goal.description
        ));
    }
    html
}

fn format_goal_detail(goal: &Goal) -> String {
    format!(
        "# {}\n\n**Status:** {} | **Scope:** {}\n\n{}\n\n---\n*goal: {}*",
        goal.title, goal.status.as_str(), goal.scope.as_str(), goal.description, goal.id
    )
}
