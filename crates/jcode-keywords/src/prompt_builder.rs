//! Prompt builder — generate system prompt sections for active keyword modes.

use crate::state::ModeState;
use crate::registry::WorkflowKind;

/// Build a prompt section describing active keyword modes.
///
/// This is injected into the system prompt's static_part so the LLM
/// knows which workflows are active and how to behave.
pub fn build_keyword_prompt(state: &ModeState) -> String {
    if state.active_modes.is_empty() {
        return String::new();
    }

    let mut sections = Vec::new();
    sections.push("# Active Keyword Modes\n".to_string());
    sections.push("The user has activated the following modes via magic keywords:\n".to_string());

    for mode in &state.active_modes {
        let desc = workflow_description(mode.workflow);
        let remaining = mode.turn_limit.saturating_sub(mode.turn_count);
        sections.push(format!(
            "- **{}** — {} ({} turns remaining)\n",
            mode.workflow, desc, remaining
        ));
    }

    sections.push("\nFollow the instructions for each active mode.".to_string());

    sections.join("")
}

/// Get the workflow instruction text for a given workflow kind.
fn workflow_description(kind: WorkflowKind) -> &'static str {
    match kind {
        WorkflowKind::Ultrawork => {
            "Spawn 4 parallel sub-agents for independent subtasks. \
             Coordinate results, handle failures with retries. \
             Aggregate into a unified response."
        }
        WorkflowKind::Ultragoal => {
            "Track a durable goal across turns. \
             Allocate a token budget. \
             Report progress after each turn."
        }
        WorkflowKind::Ultraqa => {
            "Run QA cycle: implement → test → fix → repeat \
             until all tests pass. Max 5 iterations."
        }
        WorkflowKind::Ralplan => {
            "Consensus planning: generate a plan, \
             run adversarial review, revise based on feedback, \
             get approval before executing."
        }
        WorkflowKind::DeepInterview => {
            "Requirements gathering: ask clarifying questions, \
             score ambiguity on a 1-10 scale, \
             continue until ambiguity < 3."
        }
        WorkflowKind::Tdd => {
            "Test-driven development: write failing test first, \
             implement minimal code to pass, refactor. \
             Red → Green → Refactor cycle."
        }
        WorkflowKind::CodeReview => {
            "Code review: analyze code for bugs, style issues, \
             performance problems. Provide actionable feedback \
             with line references."
        }
        WorkflowKind::SecurityReview => {
            "Security review: OWASP Top 10 scan, \
             check for hardcoded secrets, \
             verify input validation, report findings."
        }
        WorkflowKind::Ultrathink => {
            "Extended thinking: reason deeply about the problem. \
             Consider edge cases, trade-offs, alternatives. \
             Provide thorough analysis."
        }
        WorkflowKind::Deepsearch => {
            "Codebase search: use multiple search strategies \
             (grep, AST, semantic). Build a context map \
             of relevant code locations."
        }
        WorkflowKind::Analyze => {
            "Deep analysis: structured examination of code/architecture. \
             Identify patterns, anti-patterns, improvement opportunities. \
             Provide ranked recommendations."
        }
        WorkflowKind::Wiki => {
            "Doc lookup: search local docs, README, AGENTS.md \
             and web documentation. Summarize findings \
             with source references."
        }
        WorkflowKind::AiSlopCleaner => {
            "AI slop cleanup: detect low-quality AI-generated code \
             (redundant comments, over-abstraction, dead code). \
             Fix with minimal, clean replacements."
        }
        WorkflowKind::Cancel => {
            "All modes cancelled. Return to normal operation."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ActiveMode;

    #[test]
    fn empty_state_returns_empty() {
        let state = ModeState::default();
        assert!(build_keyword_prompt(&state).is_empty());
    }

    #[test]
    fn active_mode_generates_prompt() {
        let state = ModeState {
            active_modes: vec![ActiveMode {
                workflow: WorkflowKind::Ultrawork,
                activated_at: "2026-01-01T00:00:00Z".to_string(),
                turn_count: 2,
                turn_limit: 10,
            }],
            updated_at: None,
        };
        let prompt = build_keyword_prompt(&state);
        assert!(prompt.contains("ultrawork"));
        assert!(prompt.contains("8 turns remaining"));
    }

    #[test]
    fn multiple_modes_in_prompt() {
        let state = ModeState {
            active_modes: vec![
                ActiveMode {
                    workflow: WorkflowKind::Ultrawork,
                    activated_at: "2026-01-01T00:00:00Z".to_string(),
                    turn_count: 0,
                    turn_limit: 10,
                },
                ActiveMode {
                    workflow: WorkflowKind::Tdd,
                    activated_at: "2026-01-01T00:00:00Z".to_string(),
                    turn_count: 0,
                    turn_limit: 10,
                },
            ],
            updated_at: None,
        };
        let prompt = build_keyword_prompt(&state);
        assert!(prompt.contains("ultrawork"));
        assert!(prompt.contains("tdd"));
    }
}
