//! Ultragoal — GoalTracking workflow handler.
//!
//! Tier 5: State management. Tracks durable goals across turns.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct UltragoalHandler;

const DEFAULT_TOKEN_BUDGET: u32 = 100_000;

impl WorkflowHandler for UltragoalHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::Ultragoal
    }

    fn build_prompt(&self) -> String {
        "# $ultragoal — Goal Tracking Mode\n\n\
         You are in ultragoal mode. Track a durable goal across turns.\n\n\
         ## Features\n\
         - **Goal**: What we're trying to achieve\n\
         - **Budget**: Token budget for the goal\n\
         - **Progress**: Percentage complete\n\
         - **Status**: Active / Paused / Complete\n\n\
         ## Rules\n\
         - Report progress after each turn\n\
         - Track token usage\n\
         - Adjust strategy if progress stalls\n\
         - Signal completion when goal is achieved"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let goal = ctx
            .metadata
            .get("goal_description")
            .cloned()
            .unwrap_or_else(|| ctx.user_input.clone());

        let progress: f32 = ctx
            .metadata
            .get("goal_progress")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        let tokens_used: u32 = ctx
            .metadata
            .get("tokens_used")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let token_budget: u32 = ctx
            .metadata
            .get("token_budget")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_TOKEN_BUDGET);

        if tokens_used >= token_budget {
            return WorkflowAction::Complete(format!(
                "Goal tracking complete. Token budget exhausted.\nGoal: {}\nProgress: {:.0}%",
                goal, progress
            ));
        }

        if progress >= 100.0 {
            return WorkflowAction::Complete(format!(
                "Goal achieved!\nGoal: {}\nTokens used: {}/{}",
                goal, tokens_used, token_budget
            ));
        }

        let reminder = format!(
            "## Ultragoal — Tracking\n\n\
             **Goal**: {}\n\
             **Progress**: {:.0}%\n\
             **Budget**: {}/{} tokens\n\n\
             Continue working toward the goal. Report progress.",
            goal, progress, tokens_used, token_budget
        );

        let mut metadata = ctx.metadata.clone();
        if !metadata.contains_key("goal_description") {
            metadata.insert("goal_description".to_string(), goal);
        }
        metadata.insert(
            "tokens_used".to_string(),
            (tokens_used + 1000).to_string(),
        ); // Estimate
        metadata.insert("token_budget".to_string(), token_budget.to_string());

        WorkflowAction::ContinueWithMetadata {
            reminder,
            metadata,
        }
    }

    fn on_turn_complete(&self, response: &str, metadata: &HashMap<String, String>) -> WorkflowAction {
        // Try to extract progress from response
        let new_progress = extract_progress(response).unwrap_or(10.0);

        let mut updated_metadata = metadata.clone();
        updated_metadata.insert("goal_progress".to_string(), new_progress.to_string());

        if new_progress >= 100.0 {
            WorkflowAction::Complete("Goal achieved!".to_string())
        } else {
            WorkflowAction::ContinueWithMetadata {
                reminder: format!("Goal progress: {:.0}%", new_progress),
                metadata: updated_metadata,
            }
        }
    }
}

/// Extract progress percentage from LLM response.
fn extract_progress(response: &str) -> Option<f32> {
    let lower = response.to_lowercase();

    // Look for percentage patterns like "75%", "75 percent", "progress: 75"
    for line in lower.lines() {
        // Check for explicit percentage
        if let Some(pos) = line.find('%') {
            // Walk backwards from % to find the number
            let before = &line[..pos];
            let num_str: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if let Ok(num) = num_str.parse::<f32>() {
                if num <= 100.0 {
                    return Some(num);
                }
            }
        }

        // Check for "progress" or "complete" keywords with numbers
        if line.contains("progress") || line.contains("complete") || line.contains("done") {
            for word in line.split(|c: char| !c.is_ascii_digit() && c != '.') {
                if let Ok(num) = word.parse::<f32>() {
                    if num <= 100.0 {
                        return Some(num);
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_progress_from_response() {
        assert_eq!(
            extract_progress("Progress: 45% complete"),
            Some(45.0)
        );
        assert_eq!(
            extract_progress("We're 75% done"),
            Some(75.0)
        );
        assert_eq!(extract_progress("No progress here"), None);
    }
}
