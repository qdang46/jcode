//! Keyword detection — scan sanitized input for keyword triggers.

use crate::registry::KeywordEntry;
use crate::sanitizer;

/// A keyword detected in user input.
#[derive(Debug, Clone)]
pub struct DetectedKeyword {
    /// The matched keyword entry from the registry.
    pub entry: &'static KeywordEntry,
    /// The actual text that triggered the match.
    pub matched_text: String,
    /// Byte offset range (start, end) in the sanitized input.
    pub position: (usize, usize),
    /// Confidence score: 1.0 for exact keyword, 0.8-0.9 for alias match.
    pub confidence: f32,
}

/// Detect keywords in user input.
///
/// Returns all detected keywords, sorted by priority (highest first),
/// then by position (earliest first).
pub fn detect_keywords(input: &str) -> Vec<DetectedKeyword> {
    let sanitized = sanitizer::sanitize(input);
    if sanitized.is_empty() {
        return Vec::new();
    }
    let lower = sanitizer::to_lower(&sanitized);
    let registry = crate::registry::build_registry();
    let mut results = Vec::new();

    for entry in registry.iter() {
        // Check canonical keyword (case-insensitive)
        if let Some(pos) = lower.find(&entry.keyword.to_lowercase()) {
            results.push(DetectedKeyword {
                entry: *entry,
                matched_text: sanitized[pos..pos + entry.keyword.len()].to_string(),
                position: (pos, pos + entry.keyword.len()),
                confidence: 1.0,
            });
            continue;
        }

        // Check aliases (case-insensitive, fuzzy with Levenshtein ≤ 2, min 5 chars)
        for alias in entry.aliases {
            let alias_lower = alias.to_lowercase();
            if alias_lower.len() < 5 {
                // Short aliases: exact match only
                if let Some(pos) = lower.find(&alias_lower) {
                    let end = (pos + alias.len()).min(sanitized.len());
                    // Guard against non-char-boundary slicing
                    let end = sanitized
                        .char_indices()
                        .map(|(i, _)| i)
                        .take_while(|&i| i <= end)
                        .last()
                        .unwrap_or(pos);
                    results.push(DetectedKeyword {
                        entry: *entry,
                        matched_text: sanitized[pos..end].to_string(),
                        position: (pos, end),
                        confidence: 0.9,
                    });
                    break;
                }
                continue;
            }
            if let Some(pos) = find_fuzzy(&lower, &alias_lower, 2) {
                // Take the byte length of the actually-matched window, not the
                // alias itself, so a multi-byte alias cannot cause a panic on
                // a non-char-boundary slice.
                let match_len = lower[pos..]
                    .char_indices()
                    .nth(alias.chars().count())
                    .map(|(i, _)| i)
                    .unwrap_or(alias.len());
                let end = (pos + match_len).min(sanitized.len());
                results.push(DetectedKeyword {
                    entry: *entry,
                    matched_text: sanitized[pos..end].to_string(),
                    position: (pos, end),
                    confidence: 0.85,
                });
                break; // Only one alias match per entry
            }
        }
    }

    // Filter out fuzzy matches that overlap with exact matches
    let exact_ranges: Vec<(usize, usize)> = results
        .iter()
        .filter(|r| r.confidence >= 1.0)
        .map(|r| r.position)
        .collect();
    results.retain(|r| {
        if r.confidence >= 1.0 {
            return true;
        }
        // Fuzzy match must not overlap any exact match
        !exact_ranges
            .iter()
            .any(|&(es, ee)| r.position.0 < ee && r.position.1 > es)
    });

    // Sort by priority (highest first), then by position (earliest first)
    results.sort_by(|a, b| {
        b.entry
            .priority
            .cmp(&a.entry.priority)
            .then(a.position.0.cmp(&b.position.0))
    });

    // Deduplicate: keep highest-priority match per workflow kind
    deduplicate_by_workflow(results)
}

/// Find a substring with fuzzy matching (Levenshtein distance ≤ max_dist).
/// Returns the byte offset of the best match, or None.
fn find_fuzzy(haystack: &str, needle: &str, max_dist: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    // First try exact substring match (fast path)
    if let Some(pos) = haystack.find(needle) {
        return Some(pos);
    }

    // Fuzzy match: slide a window of needle length ± max_dist
    let needle_len = needle.chars().count();
    let min_len = needle_len.saturating_sub(max_dist);
    let max_len = needle_len + max_dist;

    let haystack_chars: Vec<char> = haystack.chars().collect();
    let _needle_chars: Vec<char> = needle.chars().collect();

    for window_len in min_len..=max_len {
        for i in 0..haystack_chars.len().saturating_sub(window_len - 1) {
            let window: String = haystack_chars[i..i + window_len].iter().collect();
            let dist = levenshtein_distance(&window, needle);
            if dist <= max_dist {
                // Convert char index back to byte offset
                let byte_offset: usize = haystack_chars[..i].iter().map(|c| c.len_utf8()).sum();
                return Some(byte_offset);
            }
        }
    }

    None
}

/// Compute Levenshtein distance between two strings.
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let n = a_chars.len();
    let m = b_chars.len();

    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }

    let mut prev = (0..=m).collect::<Vec<_>>();
    let mut curr = vec![0usize; m + 1];

    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[m]
}

/// Deduplicate detected keywords by workflow kind, keeping the highest-priority match.
fn deduplicate_by_workflow(mut results: Vec<DetectedKeyword>) -> Vec<DetectedKeyword> {
    let mut seen = std::collections::HashSet::new();
    results.retain(|kw| seen.insert(kw.entry.workflow));
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::WorkflowKind;

    #[test]
    fn detect_exact_keyword() {
        let results = detect_keywords("$ultrawork fix the bug");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.keyword, "$ultrawork");
        assert_eq!(results[0].confidence, 1.0);
    }

    #[test]
    fn detect_alias() {
        let results = detect_keywords("please run ulw on this");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.workflow, WorkflowKind::Ultrawork);
    }

    #[test]
    fn detect_cancel() {
        let results = detect_keywords("canceljcode");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.workflow, WorkflowKind::Cancel);
    }

    #[test]
    fn detect_natural_language() {
        let results = detect_keywords("think deeply about this problem");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.workflow, WorkflowKind::Ultrathink);
    }

    #[test]
    fn no_detection_on_plain_text() {
        let results = detect_keywords("hello world");
        assert!(results.is_empty());
    }

    #[test]
    fn detect_multiple_keywords_by_priority() {
        let results = detect_keywords("$ultrawork $tdd fix this");
        assert!(!results.is_empty());
        // ultrawork (priority 10) should come before tdd (priority 7)
        assert_eq!(results[0].entry.workflow, WorkflowKind::Ultrawork);
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
        assert_eq!(levenshtein_distance("", "abc"), 3);
    }

    #[test]
    fn detector_handles_multibyte_input_safely() {
        // Mixed CJK + ASCII should never panic, even if the alias slice
        // logic would have hit a non-char-boundary in the old impl.
        let results = detect_keywords("please 分析 this 代码 for me");
        // No alias is multi-byte in the current registry, so this is a no-op
        // detection but the call must not panic.
        for r in &results {
            // Each match's position must lie on char boundaries
            assert!(r.position.0 <= r.position.1);
            assert!(r.position.1 <= "please 分析 this 代码 for me".len());
        }
    }
}
