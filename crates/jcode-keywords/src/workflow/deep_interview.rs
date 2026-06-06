//! DeepInterview — RequirementsGathering workflow handler.
//!
//! Tier 4: Interactive. Asks clarifying questions, tracks ambiguity score.

use super::{WorkflowAction, WorkflowContext, WorkflowHandler};
use crate::registry::WorkflowKind;
use std::collections::HashMap;

pub struct DeepInterviewHandler;

const MAX_ROUNDS: u32 = 5;
const AMBIGUITY_THRESHOLD: u32 = 3;

impl WorkflowHandler for DeepInterviewHandler {
    fn kind(&self) -> WorkflowKind {
        WorkflowKind::DeepInterview
    }

    fn build_prompt(&self) -> String {
        "# $deep-interview — Requirements Gathering Mode\n\n\
         You are in deep-interview mode. Gather requirements through Q&A.\n\n\
         ## Process\n\
         1. **Analyze** — Identify ambiguity in the request\n\
         2. **Ask** — Pose clarifying questions (max 3 per round)\n\
         3. **Score** — Rate ambiguity 1-10\n\
         4. **Repeat** — Until ambiguity < 3\n\
         5. **Summarize** — Confirm requirements\n\n\
         ## Question Guidelines\n\
         - Ask one question at a time\n\
         - Be specific, not vague\n\
         - Offer options when possible\n\
         - Explain why you're asking\n\n\
         ## Ambiguity Score\n\
         - 1-2: Crystal clear, proceed\n\
         - 3-4: Mostly clear, minor questions\n\
         - 5-6: Some ambiguity, need clarification\n\
         - 7-8: Significant ambiguity, many questions\n\
         - 9-10: Very unclear, fundamental questions"
            .to_string()
    }

    fn execute(&self, ctx: &WorkflowContext) -> WorkflowAction {
        let round: u32 = ctx
            .metadata
            .get("interview_round")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let ambiguity: u32 = ctx
            .metadata
            .get("ambiguity_score")
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        if round >= MAX_ROUNDS {
            return WorkflowAction::Complete(format!(
                "Interview complete after {} rounds. Proceeding with gathered requirements.",
                round
            ));
        }

        if ambiguity < AMBIGUITY_THRESHOLD {
            return WorkflowAction::Complete(
                "Requirements are clear enough. Proceeding.".to_string(),
            );
        }

        // Build interview prompt based on round
        let reminder = if round == 0 {
            format!(
                "## Deep Interview — Round {}/{}\n\n\
                 Analyze the following request for ambiguity:\n{}\n\n\
                 Ask up to 3 clarifying questions to reduce ambiguity.\n\
                 Score the current ambiguity level (1-10).",
                round + 1,
                MAX_ROUNDS,
                ctx.user_input
            )
        } else {
            format!(
                "## Deep Interview — Round {}/{}\n\n\
                 Based on the answers so far, ask follow-up questions.\n\
                 Current ambiguity score: {}/10\n\
                 Target: below {}/10",
                round + 1,
                MAX_ROUNDS,
                ambiguity,
                AMBIGUITY_THRESHOLD
            )
        };

        let mut metadata = HashMap::new();
        metadata.insert("interview_round".to_string(), (round + 1).to_string());
        metadata.insert("ambiguity_score".to_string(), ambiguity.to_string());

        WorkflowAction::ContinueWithMetadata {
            reminder,
            metadata,
        }
    }

    fn on_turn_complete(&self, response: &str, metadata: &HashMap<String, String>) -> WorkflowAction {
        let round: u32 = metadata
            .get("interview_round")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Try to extract ambiguity score from response
        let new_ambiguity = extract_ambiguity_score(response).unwrap_or(3);

        if new_ambiguity < AMBIGUITY_THRESHOLD {
            return WorkflowAction::Complete(
                "Requirements gathered. Ambiguity is low enough to proceed.".to_string(),
            );
        }

        if round >= MAX_ROUNDS {
            return WorkflowAction::Complete(format!(
                "Interview complete after {} rounds. Final ambiguity: {}/10",
                round, new_ambiguity
            ));
        }

        // Continue interview with updated score
        let mut updated_metadata = metadata.clone();
        updated_metadata.insert("ambiguity_score".to_string(), new_ambiguity.to_string());

        WorkflowAction::ContinueWithMetadata {
            reminder: format!("Ambiguity score: {}/10. Continuing interview...", new_ambiguity),
            metadata: updated_metadata,
        }
    }
}

/// Extract ambiguity score from LLM response.
fn extract_ambiguity_score(response: &str) -> Option<u32> {
    // Look for patterns like "ambiguity: 7/10", "score: 7", "7 out of 10"
    let lower = response.to_lowercase();

    for line in lower.lines() {
        if line.contains("ambiguity") || line.contains("score") {
            // Try to find a number
            let numbers: Vec<u32> = line
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|s| s.parse().ok())
                .filter(|&n| n <= 10)
                .collect();
            if let Some(&score) = numbers.first() {
                return Some(score);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_score_from_response() {
        assert_eq!(
            extract_ambiguity_score("The ambiguity score is 7/10"),
            Some(7)
        );
        assert_eq!(
            extract_ambiguity_score("Current ambiguity: 3 out of 10"),
            Some(3)
        );
        assert_eq!(extract_ambiguity_score("No score here"), None);
    }
}
