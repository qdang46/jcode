//! Task size classification — suppress heavy modes for simple tasks.

/// Task size classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskSize {
    /// Simple: under 50 chars, no code blocks, no multi-line
    Simple,
    /// Medium: 50-200 chars, or has some structure
    Medium,
    /// Heavy: over 200 chars, has code blocks, multi-step instructions
    Heavy,
}

/// Classify the task size from user input.
///
/// Simple tasks suppress Heavy workflows (ultrawork, ralplan, ultraqa)
/// to avoid unnecessary overhead.
pub fn classify(input: &str) -> TaskSize {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        return TaskSize::Simple;
    }

    let has_code_block = trimmed.contains("```");
    let line_count = trimmed.lines().count();
    let char_count = trimmed.len();

    if char_count > 200 || (has_code_block && line_count > 5) {
        TaskSize::Heavy
    } else if char_count > 50 || has_code_block || line_count > 3 {
        TaskSize::Medium
    } else {
        TaskSize::Simple
    }
}

/// Check if a workflow should be suppressed given the task size.
///
/// Heavy workflows are suppressed for Simple tasks.
pub fn should_suppress(workflow: crate::registry::WorkflowKind, task_size: TaskSize) -> bool {
    use crate::registry::WorkflowKind;

    if task_size != TaskSize::Simple {
        return false;
    }

    matches!(
        workflow,
        WorkflowKind::Ultrawork
            | WorkflowKind::Ralplan
            | WorkflowKind::Ultraqa
            | WorkflowKind::DeepInterview
            | WorkflowKind::SecurityReview
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::WorkflowKind;

    #[test]
    fn simple_task() {
        assert_eq!(classify("fix the bug"), TaskSize::Simple);
        assert_eq!(classify("hello"), TaskSize::Simple);
        assert_eq!(classify(""), TaskSize::Simple);
    }

    #[test]
    fn medium_task() {
        assert_eq!(
            classify(
                "Please refactor the authentication module to use JWT tokens instead of sessions"
            ),
            TaskSize::Medium
        );
        assert_eq!(classify("```\nfn main() {}\n```"), TaskSize::Medium);
    }

    #[test]
    fn heavy_task() {
        let heavy_input = "a".repeat(250);
        assert_eq!(classify(&heavy_input), TaskSize::Heavy);

        let code_heavy = "```\nline1\nline2\nline3\nline4\nline5\nline6\nline7\n```";
        assert_eq!(classify(code_heavy), TaskSize::Heavy);
    }

    #[test]
    fn suppress_heavy_for_simple() {
        assert!(should_suppress(WorkflowKind::Ultrawork, TaskSize::Simple));
        assert!(!should_suppress(WorkflowKind::Ultrawork, TaskSize::Medium));
        assert!(!should_suppress(WorkflowKind::Ultrawork, TaskSize::Heavy));
    }

    #[test]
    fn never_suppress_lightweight() {
        assert!(!should_suppress(WorkflowKind::Ultrathink, TaskSize::Simple));
        assert!(!should_suppress(WorkflowKind::Wiki, TaskSize::Simple));
    }
}
