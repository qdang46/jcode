//! Ultragoal — GoalTracking workflow handler.
//!
//! Tier 5: State management. Tracks durable goals across turns.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct UltragoalHandler;

impl WorkflowHandler for UltragoalHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultragoal
    }

    fn build_prompt(&self) -> String {
        "# $ultragoal — Goal Tracking Mode

MANDATORY: Say \"GOAL MODE ENABLED!\" as your first response.

## Goal Tracking Protocol
- Goal: What we are achieving (defined by user)
- Progress: Report percentage after each turn
- Budget: Track token usage against limit
- Status: On-track / Blocked / Needs-redefinition

## Report Format
Report after each turn:
Progress: N%
Status: On-track
Next step: ...

## Completion
Goal achieved: [GOAL:ACHIEVED]
Cannot achieve: [GOAL:STUCK] - explain why"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let goal = ctx
            .metadata
            .get("goal_description")
            .cloned()
            .unwrap_or_else(|| ctx.user_input.to_string());

        let progress: f32 = ctx
            .metadata
            .get("goal_progress")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        if progress >= 100.0 {
            return WorkflowAction::Complete(format!("Goal achieved: {}", goal));
        }

        let reminder = format!(
            "## Ultragoal — Tracking\n\n\
             **Goal**: {}\n\
             **Progress**: {:.0}%\n\n\
             Continue working. Report as: `Progress: N%`",
            goal, progress
        );

        let mut metadata = ctx.metadata.clone();
        if !metadata.contains_key("goal_description") {
            metadata.insert("goal_description".to_string(), goal);
        }
        if !metadata.contains_key("goal_progress") {
            metadata.insert("goal_progress".to_string(), "0".to_string());
        }

        WorkflowAction::ContinueWithMetadata { reminder, metadata }
    }

    fn on_turn_complete(
        &self,
        response: &str,
        metadata: &HashMap<String, String>,
    ) -> WorkflowAction {
        // Check for explicit completion
        if response.contains("[GOAL:ACHIEVED]") {
            return WorkflowAction::Complete("Goal achieved!".to_string());
        }

        let mut updated = metadata.clone();

        // Only update progress if LLM actually reported it
        if let Some(new_progress) = extract_progress(response) {
            updated.insert("goal_progress".to_string(), new_progress.to_string());
            if new_progress >= 100.0 {
                return WorkflowAction::Complete("Goal achieved!".to_string());
            }
        }

        let current_progress: f32 = updated
            .get("goal_progress")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        WorkflowAction::ContinueWithMetadata {
            reminder: format!("Progress: {:.0}%", current_progress),
            metadata: updated,
        }
    }
}

/// Extract progress percentage from LLM response.
/// Requires "progress" keyword on the same line as a percentage.
fn extract_progress(response: &str) -> Option<f32> {
    let lower = response.to_lowercase();

    for line in lower.lines() {
        // Only match lines with "progress" keyword
        if !line.contains("progress") {
            continue;
        }

        // Look for N% pattern
        if let Some(pos) = line.find('%') {
            let before = &line[..pos];
            let num_str: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if let Ok(num) = num_str.parse::<f32>()
                && num.is_finite()
                && num <= 100.0
            {
                return Some(num);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_progress_with_keyword() {
        assert_eq!(extract_progress("Progress: 45%"), Some(45.0));
        assert_eq!(extract_progress("Overall progress is 75%"), Some(75.0));
    }

    #[test]
    fn extract_progress_requires_keyword() {
        // Should NOT match % without "progress"
        assert_eq!(extract_progress("The code has 10% test coverage"), None);
        assert_eq!(extract_progress("We're 75% done"), None);
    }

    #[test]
    fn extract_progress_no_match() {
        assert_eq!(extract_progress("No progress here"), None);
        assert_eq!(extract_progress(""), None);
    }

    #[test]
    fn extract_progress_rejects_infinite() {
        // Very large numbers should not match
        assert_eq!(extract_progress("Progress: 9999999999%"), None);
    }
}
