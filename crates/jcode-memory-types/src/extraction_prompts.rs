//! Prompt templates for the background memory extraction agent.
//!
//! The extraction agent runs as a perfect fork of the main conversation — same
//! system prompt, same message prefix. When the main agent writes memories
//! itself, extraction skips that turn. These prompts fire only when the main
//! agent didn't write.
//!
//! ## Strategy
//! - **Turn 1**: Issue all read/grep/glob calls in parallel to gather context
//! - **Turn 2**: Issue all write/edit calls in parallel to save memories
//! - No interleaving of reads and writes — minimizes turn count
//!
//! ## Reference
//! CCB: src/services/extractMemories/prompts.ts

use crate::extraction::ExtractionPromptVariant;

/// Build the opener section shared by both prompt variants.
fn opener(new_message_count: usize, existing_memories: &str) -> String {
    let manifest = if !existing_memories.is_empty() {
        format!(
            "\n\n## Existing memory files\n\n{}\n\nCheck this list before writing — update an existing file rather than creating a duplicate.",
            existing_memories
        )
    } else {
        String::new()
    };

    format!(
        "You are now acting as the memory extraction subagent. Analyze the most recent ~{count} messages above and use them to update your persistent memory systems.\n\n\
         Available tools: read, grep, glob, read-only bash (ls/find/cat/stat/wc/head/tail and similar), and write/edit for paths inside the memory directory only. \
         Bash rm is not permitted. All other tools will be denied.\n\n\
         You have a limited turn budget. The efficient strategy is: \
         turn 1 — issue all read calls in parallel for every file you might update; \
         turn 2 — issue all write/edit calls in parallel. Do not interleave reads and writes across multiple turns.\n\n\
         You MUST only use content from the last ~{count} messages to update your persistent memories. \
         Do not waste any turns attempting to investigate or verify that content further — no grepping source files, \
         no reading code to confirm a pattern exists, no git commands.{manifest}",
        count = new_message_count,
    )
}

/// Build the extraction prompt for auto-only memory.
///
/// Four-type taxonomy:
/// 1. User identity and role
/// 2. Project conventions and preferences
/// 3. Feedback and testing patterns
/// 4. Technical decisions and architecture
pub fn build_extraction_prompt(
    variant: &ExtractionPromptVariant,
    new_message_count: usize,
    existing_memories: &str,
    skip_index: bool,
) -> String {
    let header = opener(new_message_count, existing_memories);

    let types_section = match variant {
        ExtractionPromptVariant::Auto => {
            r#"
## Types of memories to save

1. **User Identity & Role** — Who the user is, their role, their goals. Examples: "User is a backend engineer working on a distributed systems project", "User prefers Rust for performance-critical components".
2. **Project Conventions & Preferences** — Coding style, naming conventions, tool preferences, workflow patterns. Examples: "Project uses snake_case for all identifiers", "User prefers async/await over manual future combinators".
3. **Feedback & Testing Patterns** — Testing preferences, CI setup, bug reproduction steps, quality standards. Examples: "All new code must include property-based tests", "User runs clippy as part of CI".
4. **Technical Decisions & Architecture** — Key design decisions, trade-offs, architecture diagrams, dependency choices. Examples: "The system uses a CQRS pattern with separate read/write databases", "Chose Actix-web over Axum due to WebSocket performance"."#
                .to_string()
        }
        ExtractionPromptVariant::Combined => {
            r#"
## Types of memories to save

1. **User Identity & Role** (scope: auto) — Who the user is...
2. **Project Conventions & Preferences** (scope: auto) — ...
3. **Feedback & Testing Patterns** (scope: team) — ...
4. **Technical Decisions & Architecture** (scope: team) — ..."#
                .to_string()
        }
    };

    let how_to_save = if skip_index {
        r#"
## How to save memories

Write each memory to its own file (e.g., `user_role.md`, `feedback_testing.md`) using frontmatter format:

```markdown
---
type: user_identity | project_convention | feedback_pattern | technical_decision
confidence: high | medium | low
tags: [comma-separated tags]
---

Memory content here...
```"#
            .to_string()
    } else {
        r#"
## How to save memories

**Step 1** — write the memory to its own file using frontmatter format.

**Step 2** — add a pointer to that file in `MEMORY.md`. Each entry should be one line, under ~150 characters.
Never write memory content directly into `MEMORY.md`."#
            .to_string()
    };

    format!("{}\n\n{}\n\n{}", header, types_section, how_to_save)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_extraction_prompt_auto() {
        let prompt = build_extraction_prompt(
            &ExtractionPromptVariant::Auto,
            5,
            "- user_role.md\n- project_conventions.md",
            false,
        );
        assert!(prompt.contains("memory extraction subagent"));
        assert!(prompt.contains("Types of memories to save"));
        assert!(prompt.contains("User Identity & Role"));
        assert!(prompt.contains("MEMORY.md"));
        assert!(prompt.contains("Existing memory files"));
    }

    #[test]
    fn test_build_extraction_prompt_skip_index() {
        let prompt = build_extraction_prompt(
            &ExtractionPromptVariant::Auto,
            3,
            "",
            true,
        );
        assert!(prompt.contains("memory extraction subagent"));
        assert!(!prompt.contains("MEMORY.md"), "Skip index mode should not mention MEMORY.md");
    }

    #[test]
    fn test_build_extraction_prompt_combined() {
        let prompt = build_extraction_prompt(
            &ExtractionPromptVariant::Combined,
            5,
            "",
            false,
        );
        assert!(prompt.contains("(scope: auto)"));
        assert!(prompt.contains("(scope: team)"));
    }

    #[test]
    fn test_build_extraction_prompt_empty_existing() {
        let prompt = build_extraction_prompt(
            &ExtractionPromptVariant::Auto,
            5,
            "",
            false,
        );
        // Should NOT mention existing files when there are none
        assert!(
            !prompt.contains("Existing memory files"),
            "Empty existing_memories should omit the section"
        );
    }
}
