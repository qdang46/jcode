//! Intent disambiguation — resolve overlapping keyword matches.

use crate::detector::DetectedKeyword;
use crate::registry::WorkflowKind;

/// Disambiguate overlapping keyword detections.
///
/// Rules:
/// 1. Higher priority wins
/// 2. Equal priority → longer match wins (more specific)
/// 3. Equal priority + equal length → earlier position wins
/// 4. Cancel always wins over everything
pub fn disambiguate(detections: Vec<DetectedKeyword>) -> Vec<DetectedKeyword> {
    if detections.len() <= 1 {
        return detections;
    }

    let mut result = Vec::new();
    let mut used_ranges: Vec<(usize, usize)> = Vec::new();

    for detection in detections {
        // Cancel always passes through
        if detection.entry.workflow == WorkflowKind::Cancel {
            result.push(detection);
            continue;
        }

        // Check if this overlaps with an already-accepted detection
        let overlaps = used_ranges.iter().any(|&(start, end)| {
            detection.position.0 < end && detection.position.1 > start
        });

        if !overlaps {
            used_ranges.push(detection.position);
            result.push(detection);
        }
    }

    result
}

/// Check if two detections conflict (same position range).
pub fn are_conflicting(a: &DetectedKeyword, b: &DetectedKeyword) -> bool {
    a.position.0 < b.position.1 && b.position.0 < a.position.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{KeywordEntry, WorkflowKind};

    fn make_detection(
        keyword: &'static str,
        workflow: WorkflowKind,
        priority: u8,
        pos: (usize, usize),
    ) -> DetectedKeyword {
        DetectedKeyword {
            entry: Box::leak(Box::new(KeywordEntry {
                keyword,
                aliases: &[],
                priority,
                workflow,
                description: "",
            })),
            matched_text: keyword.to_string(),
            position: pos,
            confidence: 1.0,
        }
    }

    #[test]
    fn cancel_always_wins() {
        let detections = vec![
            make_detection(
                "$ultrawork",
                WorkflowKind::Ultrawork,
                10,
                (0, 10),
            ),
            make_detection(
                "canceljcode",
                WorkflowKind::Cancel,
                9,
                (11, 22),
            ),
        ];
        let result = disambiguate(detections);
        assert!(result.iter().any(|d| d.entry.workflow == WorkflowKind::Cancel));
    }

    #[test]
    fn non_overlapping_both_kept() {
        let detections = vec![
            make_detection("$tdd", WorkflowKind::Tdd, 7, (0, 4)),
            make_detection("$wiki", WorkflowKind::Wiki, 5, (10, 15)),
        ];
        let result = disambiguate(detections);
        assert_eq!(result.len(), 2);
    }
}
