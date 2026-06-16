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
        "# $deep-interview — Requirements Gathering Mode

MANDATORY: Say \"INTERVIEW MODE ENABLED!\" as your first response.

## Interview Protocol
1. Analyze the request for ambiguous terms
2. Ask clarifying questions (max 3 per round)
3. Score ambiguity 1-10 for each aspect
4. Repeat until total ambiguity < 3
5. Produce final requirements document

## Scoring
Report ambiguity as: Ambiguity: N/10
Threshold: score < 3 -> interview complete

## Completion
When done: [INTERVIEW:COMPLETE]
Output structured requirements."
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
            return WorkflowAction::Complete(format!("Interview complete after {} rounds.", round));
        }

        if ambiguity < AMBIGUITY_THRESHOLD {
            return WorkflowAction::Complete("Requirements are clear. Proceeding.".to_string());
        }

        let reminder = if round == 0 {
            format!(
                "## Deep Interview — Round {}/{}\n\n\
                 Analyze for ambiguity:\n{}\n\n\
                 Ask up to 3 clarifying questions.\n\
                 Report ambiguity as: `Ambiguity: N/10`",
                round + 1,
                MAX_ROUNDS,
                ctx.user_input
            )
        } else {
            format!(
                "## Deep Interview — Round {}/{}\n\n\
                 Current ambiguity: {}/10\n\
                 Target: below {}/10\n\
                 Ask follow-up questions.",
                round + 1,
                MAX_ROUNDS,
                ambiguity,
                AMBIGUITY_THRESHOLD
            )
        };

        let mut metadata = ctx.metadata.clone();
        metadata.insert("interview_round".to_string(), (round + 1).to_string());
        if !metadata.contains_key("ambiguity_score") {
            metadata.insert("ambiguity_score".to_string(), "5".to_string());
        }

        WorkflowAction::ContinueWithMetadata { reminder, metadata }
    }

    fn on_turn_complete(
        &self,
        response: &str,
        metadata: &HashMap<String, String>,
    ) -> WorkflowAction {
        // Check for explicit completion marker
        if response.contains("[INTERVIEW:COMPLETE]") {
            return WorkflowAction::Complete("Requirements gathered.".to_string());
        }

        let round: u32 = metadata
            .get("interview_round")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Extract ambiguity score using tighter pattern
        let new_ambiguity = extract_ambiguity_score(response).unwrap_or(4);

        if new_ambiguity < AMBIGUITY_THRESHOLD {
            return WorkflowAction::Complete(
                "Requirements gathered. Ambiguity is low.".to_string(),
            );
        }

        if round >= MAX_ROUNDS {
            return WorkflowAction::Complete(format!(
                "Interview complete after {} rounds. Ambiguity: {}/10",
                round, new_ambiguity
            ));
        }

        let mut updated = metadata.clone();
        updated.insert("ambiguity_score".to_string(), new_ambiguity.to_string());

        WorkflowAction::ContinueWithMetadata {
            reminder: format!("Ambiguity: {}/10. Continuing interview...", new_ambiguity),
            metadata: updated,
        }
    }
}

/// Extract ambiguity score from LLM response.
/// Uses tight pattern: requires "ambiguity" on the same line as a N/10 pattern.
fn extract_ambiguity_score(response: &str) -> Option<u32> {
    let lower = response.to_lowercase();

    for line in lower.lines() {
        if !line.contains("ambiguity") {
            continue;
        }
        // Look for N/10 pattern specifically
        if let Some(pos) = line.find("/10") {
            let before = &line[..pos];
            let num_str: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if let Ok(n @ ..=10) = num_str.parse::<u32>() {
                return Some(n);
            }
        }
        // Fallback: look for "ambiguity.*N" pattern
        for word in line.split_whitespace() {
            if let Ok(n @ ..=10) = word.parse::<u32>() {
                return Some(n);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_score_from_n_over_10() {
        assert_eq!(extract_ambiguity_score("Ambiguity: 7/10"), Some(7));
        assert_eq!(
            extract_ambiguity_score("The ambiguity is about 3/10"),
            Some(3)
        );
    }

    #[test]
    fn extract_score_requires_ambiguity_keyword() {
        // Should NOT match "score" without "ambiguity"
        assert_eq!(extract_ambiguity_score("The security score is 8/10"), None);
        assert_eq!(extract_ambiguity_score("Performance score: 6"), None);
    }

    #[test]
    fn extract_score_no_match() {
        assert_eq!(extract_ambiguity_score("No score here"), None);
        assert_eq!(extract_ambiguity_score(""), None);
    }
}
