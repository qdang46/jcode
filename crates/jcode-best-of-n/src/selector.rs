use crate::config::SelectorConfig;
use crate::types::{CandidateDiff, CandidateStatus, SelectScore};

/// Result of selecting the best candidate.
#[derive(Debug, Clone)]
pub struct SelectionResult {
    /// Index of the winning candidate in the candidates slice.
    pub winner_index: usize,
    /// Human-readable reason for the selection.
    pub reason: String,
    /// All scores computed during selection (for debugging/display).
    pub scores: Vec<SelectScore>,
}

/// Determine the best candidate using deterministic scoring.
///
/// Selection policy (inspired by pi-agent-rust's `select_best_candidate`):
/// 1. Prefer successful candidates over failures
/// 2. Prefer candidates with meaningful changes over ghosts
/// 3. Prefer focused edits (fewer files changed)
/// 4. Prefer fewer total operations
/// 5. Prefer lower token cost
/// 6. Tiebreaker: earlier candidate index (stable sort)
pub fn select_best_candidate(
    candidates: &[CandidateDiff],
    _config: &SelectorConfig,
) -> SelectionResult {
    if candidates.is_empty() {
        return SelectionResult {
            winner_index: 0,
            reason: "No candidates available".to_string(),
            scores: Vec::new(),
        };
    }

    // Score each candidate
    let mut scored: Vec<(usize, SelectScore)> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let score = score_candidate(candidate, index, _config);
            (index, score)
        })
        .collect();

    // Sort by score (best first)
    scored.sort_by(|(_, a), (_, b)| cmp_scores(a, b));

    let winner_index = scored[0].0;
    let scores: Vec<SelectScore> = scored.iter().map(|(_, s)| s.clone()).collect();

    let winner = &candidates[winner_index];
    let reason = build_selection_reason(winner, &scored[0].1);

    SelectionResult {
        winner_index,
        reason,
        scores,
    }
}

/// Compute a score for a single candidate.
fn score_candidate(
    candidate: &CandidateDiff,
    index: usize,
    _config: &SelectorConfig,
) -> SelectScore {
    let success = matches!(candidate.status, CandidateStatus::Success);
    let has_changes = candidate.has_meaningful_changes();

    SelectScore {
        candidate_id: candidate.candidate_id.clone(),
        success,
        has_changes,
        file_count: candidate.changed_file_count(),
        op_count: candidate.total_ops(),
        tokens: candidate.total_tokens.unwrap_or(u64::MAX),
        index,
    }
}

/// Compare two scores. Returns Less if `a` is better than `b`.
fn cmp_scores(a: &SelectScore, b: &SelectScore) -> std::cmp::Ordering {
    // 1. Success over failure
    match (a.success, b.success) {
        (true, false) => return std::cmp::Ordering::Less,
        (false, true) => return std::cmp::Ordering::Greater,
        _ => {}
    }

    // 2. Non-ghost over ghost
    match (a.has_changes, b.has_changes) {
        (true, false) => return std::cmp::Ordering::Less,
        (false, true) => return std::cmp::Ordering::Greater,
        _ => {}
    }

    // 3. Fewer files changed (focused) — ties broken by fewer ops
    match a.file_count.cmp(&b.file_count) {
        std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
        std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
        std::cmp::Ordering::Equal => {}
    }

    // 4. Fewer operations
    match a.op_count.cmp(&b.op_count) {
        std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
        std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
        std::cmp::Ordering::Equal => {}
    }

    // 5. Lower token cost (if available)
    match (a.tokens, b.tokens) {
        (u64::MAX, u64::MAX) => {}
        (u64::MAX, _) => return std::cmp::Ordering::Greater,
        (_, u64::MAX) => return std::cmp::Ordering::Less,
        (a_tok, b_tok) => match a_tok.cmp(&b_tok) {
            std::cmp::Ordering::Less => return std::cmp::Ordering::Less,
            std::cmp::Ordering::Greater => return std::cmp::Ordering::Greater,
            std::cmp::Ordering::Equal => {}
        },
    }

    // 6. Tiebreaker: earlier index (stable, deterministic)
    a.index.cmp(&b.index)
}

/// Build a human-readable reason for the selection.
fn build_selection_reason(winner: &CandidateDiff, score: &SelectScore) -> String {
    let mut parts = Vec::new();

    if score.success {
        parts.push("succeeded".to_string());
    } else {
        parts.push("failed".to_string());
    }

    if score.has_changes {
        parts.push(format!(
            "changed {} file(s) in {} op(s)",
            score.file_count, score.op_count
        ));
    } else {
        parts.push("no changes".to_string());
    }

    if score.tokens != u64::MAX {
        parts.push(format!("{} tokens", score.tokens));
    }

    parts.push(format!("strategy: {}", winner.strategy.label));

    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CandidateStrategy, FileDiff};

    fn make_candidate(
        id: usize,
        status: CandidateStatus,
        file_count: usize,
        tokens: Option<u64>,
    ) -> CandidateDiff {
        let file_diffs: Vec<FileDiff> = (0..file_count)
            .map(|i| FileDiff {
                file_path: format!("file-{i}.rs"),
                unified_diff: format!(
                    "--- a/file-{i}.rs\n+++ b/file-{i}.rs\n@@ -1 +1 @@\n-old\n+new\n"
                ),
                old_content: "old".to_string(),
                new_content: "new".to_string(),
                is_new_file: false,
            })
            .collect();

        CandidateDiff {
            candidate_id: crate::types::CandidateId::new(id),
            strategy: CandidateStrategy {
                label: format!("strategy-{id}"),
                temperature: 0.5,
                model: None,
            },
            status,
            file_diffs,
            total_tokens: tokens,
            error: None,
        }
    }

    fn make_ghost_candidate(id: usize) -> CandidateDiff {
        CandidateDiff {
            candidate_id: crate::types::CandidateId::new(id),
            strategy: CandidateStrategy {
                label: format!("strategy-{id}"),
                temperature: 0.5,
                model: None,
            },
            status: CandidateStatus::Success,
            file_diffs: vec![FileDiff {
                file_path: "file.rs".to_string(),
                unified_diff: String::new(),
                old_content: "same".to_string(),
                new_content: "same".to_string(),
                is_new_file: false,
            }],
            total_tokens: Some(100),
            error: None,
        }
    }

    #[test]
    fn test_prefers_success_over_failure() {
        let config = SelectorConfig::default();
        let candidates = vec![
            make_candidate(0, CandidateStatus::Failed, 1, Some(100)),
            make_candidate(1, CandidateStatus::Success, 1, Some(100)),
        ];

        let result = select_best_candidate(&candidates, &config);
        assert_eq!(result.winner_index, 1);
    }

    #[test]
    fn test_prefers_meaningful_changes_over_ghost() {
        let config = SelectorConfig::default();
        let candidates = vec![
            make_ghost_candidate(0),
            make_candidate(1, CandidateStatus::Success, 1, Some(100)),
        ];

        let result = select_best_candidate(&candidates, &config);
        assert_eq!(result.winner_index, 1);
    }

    #[test]
    fn test_prefers_fewer_files() {
        let config = SelectorConfig::default();
        let candidates = vec![
            make_candidate(0, CandidateStatus::Success, 3, Some(100)),
            make_candidate(1, CandidateStatus::Success, 1, Some(100)),
        ];

        let result = select_best_candidate(&candidates, &config);
        assert_eq!(result.winner_index, 1);
    }

    #[test]
    fn test_prefers_lower_tokens() {
        let config = SelectorConfig::default();
        let candidates = vec![
            make_candidate(0, CandidateStatus::Success, 1, Some(500)),
            make_candidate(1, CandidateStatus::Success, 1, Some(100)),
        ];

        let result = select_best_candidate(&candidates, &config);
        assert_eq!(result.winner_index, 1);
    }

    #[test]
    fn test_tiebreaker_earlier_index() {
        let config = SelectorConfig::default();
        let candidates = vec![
            make_candidate(0, CandidateStatus::Success, 1, Some(100)),
            make_candidate(1, CandidateStatus::Success, 1, Some(100)),
        ];

        let result = select_best_candidate(&candidates, &config);
        assert_eq!(result.winner_index, 0);
    }

    #[test]
    fn test_empty_candidates() {
        let config = SelectorConfig::default();
        let candidates: Vec<CandidateDiff> = Vec::new();

        let result = select_best_candidate(&candidates, &config);
        assert_eq!(result.winner_index, 0);
        assert!(result.reason.contains("No candidates"));
    }

    #[test]
    fn test_selection_reason_mentions_strategy() {
        let config = SelectorConfig::default();
        let candidates = vec![make_candidate(0, CandidateStatus::Success, 1, Some(100))];

        let result = select_best_candidate(&candidates, &config);
        assert!(result.reason.contains("strategy-0"));
    }
}
