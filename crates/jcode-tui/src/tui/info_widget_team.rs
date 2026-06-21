//! Team info widget — live roster + compact task list for the active team run.
//!
//! Mirrors the rendering style of `info_widget_swarm_background.rs` (status icons
//! + color per member) but adds a task DAG section. Wired into `WidgetKind::TeamView`.

use super::{InfoWidgetData, truncate_smart};
use crate::tui::color_support::rgb;
use ratatui::prelude::*;

/// Snapshot of team state fed into `InfoWidgetData.team_info`.
#[derive(Debug, Default, Clone)]
pub struct TeamInfo {
    pub team_name: String,
    pub member_total: usize,
    pub members: Vec<TeamMemberView>,
    pub tasks: Vec<TeamTaskView>,
}

#[derive(Debug, Clone)]
pub struct TeamMemberView {
    pub name: String,
    pub is_lead: bool,
    pub status: String,
    pub task_count: usize,
    pub message_count: usize,
    pub color: Option<String>,
}

/// Interactive team view with keyboard selection.
/// Allows selecting tasks/members and performing actions (claim, close, view).
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(super) struct TeamViewInteraction {
    /// Currently selected index (0 = header, 1+ = members then tasks).
    #[allow(dead_code)]
    pub selected: usize,
    /// Whether selection mode is active.
    pub active: bool,
}

impl TeamViewInteraction {
    /// Total interactive slots in the current view.
    #[allow(dead_code)]
    pub fn slot_count(member_count: usize, task_count: usize) -> usize {
        1 + member_count.min(5) + task_count.min(3) // header + members + tasks
    }
}
#[derive(Debug, Clone)]
pub struct TeamTaskView {
    pub id: String,
    pub subject: String,
    pub status: String,
    pub owner: Option<String>,
    pub blocked_by: Vec<String>,
}

fn member_status_glyph(status: &str) -> (Color, &'static str) {
    match status {
        "pending" => (rgb(140, 140, 150), "○"),
        "running" => (rgb(255, 200, 100), "▶"),
        "idle" => (rgb(120, 180, 120), "●"),
        "errored" => (rgb(255, 100, 100), "✗"),
        "completed" => (rgb(100, 200, 100), "✓"),
        _ => (rgb(140, 140, 150), "·"),
    }
}

fn task_status_badge(status: &str) -> (Color, &'static str) {
    match status {
        "completed" => (rgb(100, 200, 100), "[✓]"),
        "in_progress" => (rgb(255, 200, 100), "[▶]"),
        "claimed" => (rgb(140, 180, 255), "[◑]"),
        _ => (rgb(140, 140, 150), "[○]"),
    }
}

pub(super) fn render_team_widget(data: &InfoWidgetData, inner: Rect) -> Vec<Line<'static>> {
    let Some(info) = &data.team_info else {
        return Vec::new();
    };
    let mut lines = Vec::new();

    // Header: team name + member/task counts
    let active = info
        .members
        .iter()
        .filter(|m| m.status == "running")
        .count();
    lines.push(Line::from(vec![
        Span::styled("👥 ", Style::default().fg(rgb(255, 200, 100))),
        Span::styled(
            truncate_smart(&info.team_name, inner.width.saturating_sub(20) as usize),
            Style::default().fg(rgb(220, 220, 230)).bold(),
        ),
        Span::styled(
            format!(
                " {}/{} · {} active · {} tasks",
                info.members.len(),
                info.member_total,
                active,
                info.tasks.len()
            ),
            Style::default().fg(rgb(140, 140, 150)),
        ),
    ]));

    // Member rows (cap to fit height, reserve room for tasks)
    let max_members = ((inner.height as usize).saturating_sub(2))
        .min(info.members.len())
        .min(5);
    for m in info.members.iter().take(max_members) {
        let (color, glyph) = member_status_glyph(&m.status);
        let role = if m.is_lead { "★ " } else { "  " };
        let detail = format!("{} · {}t · {}m", m.status, m.task_count, m.message_count);
        lines.push(Line::from(vec![
            Span::styled(role.to_string(), Style::default().fg(rgb(255, 200, 100))),
            Span::styled(format!("{glyph} "), Style::default().fg(color)),
            Span::styled(
                truncate_smart(&m.name, 14),
                Style::default().fg(rgb(200, 200, 210)),
            ),
            Span::styled(
                format!("  {detail}"),
                Style::default().fg(rgb(140, 140, 150)),
            ),
        ]));
    }

    // Task DAG (compact): show up to 3, with dependency arrows
    let remaining = (inner.height as usize).saturating_sub(lines.len());
    if remaining > 1 && !info.tasks.is_empty() {
        lines.push(Line::from(Span::styled(
            "Tasks",
            Style::default().fg(rgb(140, 140, 150)).bold(),
        )));
        for t in info.tasks.iter().take(remaining.saturating_sub(1)).take(3) {
            let (color, badge) = task_status_badge(&t.status);
            let mut spans = vec![
                Span::styled(format!("{badge} "), Style::default().fg(color)),
                Span::styled(
                    truncate_smart(&t.subject, 22),
                    Style::default().fg(rgb(190, 190, 200)),
                ),
            ];
            if let Some(owner) = &t.owner {
                spans.push(Span::styled(
                    format!(" ({owner})"),
                    Style::default().fg(rgb(120, 120, 130)),
                ));
            }
            if !t.blocked_by.is_empty() {
                spans.push(Span::styled(
                    format!(" ←{}", t.blocked_by.join(",")),
                    Style::default().fg(rgb(255, 170, 80)),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    lines
}
