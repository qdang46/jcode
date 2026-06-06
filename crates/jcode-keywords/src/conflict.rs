//! Conflict detection — warn about incompatible mode combinations.

use crate::registry::WorkflowKind;

/// A conflict between two workflow kinds.
#[derive(Debug, Clone)]
pub struct Conflict {
    pub a: WorkflowKind,
    pub b: WorkflowKind,
    pub reason: &'static str,
}

/// Check for conflicts among a set of active workflow kinds.
///
/// Returns a list of conflicts found. Empty list means no conflicts.
pub fn check_conflicts(active: &[WorkflowKind]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();

    for (i, &a) in active.iter().enumerate() {
        for &b in &active[i + 1..] {
            if let Some(conflict) = pair_conflict(a, b) {
                conflicts.push(conflict);
            }
        }
    }

    conflicts
}

/// Check if two specific workflows conflict.
fn pair_conflict(a: WorkflowKind, b: WorkflowKind) -> Option<Conflict> {
    use WorkflowKind::*;

    match (a, b) {
        // TDD + ultrawork: TDD is sequential, ultrawork is parallel
        (Tdd, Ultrawork) | (Ultrawork, Tdd) => Some(Conflict {
            a,
            b,
            reason: "TDD is sequential (red-green-refactor) while ultrawork spawns parallel agents",
        }),
        // Cancel conflicts with everything except itself
        (Cancel, other) | (other, Cancel) if other != Cancel => Some(Conflict {
            a: Cancel,
            b: other,
            reason: "canceljcode will deactivate all other modes",
        }),
        // deep-interview + ultrawork: interview needs user interaction, ultrawork is autonomous
        (DeepInterview, Ultrawork) | (Ultrawork, DeepInterview) => Some(Conflict {
            a,
            b,
            reason: "deep-interview requires user interaction while ultrawork runs autonomously",
        }),
        _ => None,
    }
}

/// Format a conflict as a human-readable warning string.
pub fn format_warning(conflict: &Conflict) -> String {
    format!(
        "⚠ Conflict: {} + {} — {}",
        conflict.a, conflict.b, conflict.reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_conflicts_empty() {
        assert!(check_conflicts(&[]).is_empty());
    }

    #[test]
    fn no_conflicts_compatible() {
        assert!(check_conflicts(&[WorkflowKind::Ultrathink, WorkflowKind::Wiki]).is_empty());
    }

    #[test]
    fn tdd_ultrawork_conflict() {
        let conflicts = check_conflicts(&[WorkflowKind::Tdd, WorkflowKind::Ultrawork]);
        assert_eq!(conflicts.len(), 1);
    }

    #[test]
    fn cancel_conflicts_with_all() {
        let conflicts = check_conflicts(&[WorkflowKind::Cancel, WorkflowKind::Tdd]);
        assert_eq!(conflicts.len(), 1);
    }

    #[test]
    fn format_warning_works() {
        let conflict = Conflict {
            a: WorkflowKind::Tdd,
            b: WorkflowKind::Ultrawork,
            reason: "test reason",
        };
        let msg = format_warning(&conflict);
        assert!(msg.contains("tdd"));
        assert!(msg.contains("ultrawork"));
    }
}
