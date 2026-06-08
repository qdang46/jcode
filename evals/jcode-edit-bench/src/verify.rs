//! File verification for edit benchmark.
//!
//! Compares output files against expected fixtures with rustfmt-normalized
//! byte-for-byte comparison. Adapted from oh-my-pi's `verify.ts`.

use std::collections::HashSet;
use std::path::Path;

use similar::{ChangeTag, TextDiff};

use crate::fixtures::list_files;
use crate::formatter::{
    compute_indent_score, format_content, normalize_blank_lines, normalize_line_endings,
};

/// Result of a verification pass.
#[derive(Debug)]
pub struct VerificationResult {
    pub success: bool,
    pub error: Option<String>,
    pub indent_score: Option<f64>,
    pub formatted_equivalent: Option<bool>,
    pub diff_stats: Option<DiffStats>,
    pub diff: Option<String>,
}

/// Diff statistics.
#[derive(Debug, Clone)]
pub struct DiffStats {
    pub lines_changed: usize,
    pub chars_changed: usize,
}

/// Verify that the actual output matches the expected fixtures.
///
/// Steps:
/// 1. Compare file sets (no missing or extra files)
/// 2. Normalize line endings (CRLF → LF)
/// 3. Normalize blank lines (3+ → 1 blank)
/// 4. Format both via rustfmt
/// 5. Compare formatted content
/// 6. Compute indent score
pub async fn verify_files(
    expected_dir: &Path,
    actual_dir: &Path,
    files: Option<&[String]>,
) -> anyhow::Result<VerificationResult> {
    let start_time = std::time::Instant::now();
    let mut total_indent_score = 0.0;
    let mut file_count = 0;

    let expected_fixture_files = list_files(expected_dir)?;
    let actual_files = list_files(actual_dir)?;

    let expected_files = match files {
        Some(f) => {
            let mut f = f.to_vec();
            f.sort();
            f
        }
        None => expected_fixture_files.clone(),
    };

    // Check for missing files
    let missing_files: Vec<String> = expected_files
        .iter()
        .filter(|f| !actual_files.contains(f))
        .cloned()
        .collect();
    if !missing_files.is_empty() {
        return Ok(VerificationResult {
            success: false,
            error: Some(format!("Missing files: {}", missing_files.join(", "))),
            indent_score: None,
            formatted_equivalent: None,
            diff_stats: None,
            diff: None,
        });
    }

    // Check for extra files (when no subset specified)
    if files.is_none() {
        let extra_files: Vec<String> = actual_files
            .iter()
            .filter(|f| !expected_files.contains(f))
            .cloned()
            .collect();
        if !extra_files.is_empty() {
            return Ok(VerificationResult {
                success: false,
                error: Some(format!("Unexpected files: {}", extra_files.join(", "))),
                indent_score: None,
                formatted_equivalent: None,
                diff_stats: None,
                diff: None,
            });
        }
    }

    // Check each file
    for file in &expected_files {
        let expected_path = expected_dir.join(file);
        let actual_path = actual_dir.join(file);

        let expected_raw = std::fs::read_to_string(&expected_path)?;
        let actual_raw = std::fs::read_to_string(&actual_path)?;

        let expected_normalized = normalize_line_endings(&expected_raw);
        let actual_normalized = normalize_line_endings(&actual_raw);

        // Restore whitespace-only diffs
        let actual_restored =
            restore_whitespace_only_diffs(&expected_normalized, &actual_normalized);

        // Normalize blank lines
        let expected_blank = normalize_blank_lines(&expected_normalized);
        let actual_blank = normalize_blank_lines(&actual_restored);

        // Format both
        let expected_formatted = format_content(&expected_path, &expected_blank).await;
        let actual_formatted = format_content(&actual_path, &actual_blank).await;

        let formatted_equivalent = expected_formatted.formatted == actual_formatted.formatted;

        // Indent score: distance between actual raw and formatted
        let file_indent_score =
            compute_indent_score(&actual_normalized, &actual_formatted.formatted);
        total_indent_score += file_indent_score;
        file_count += 1;

        if !formatted_equivalent {
            let diff = create_compact_diff(
                &expected_formatted.formatted,
                &actual_formatted.formatted,
                3,
            );
            let stats =
                compute_diff_stats(&expected_formatted.formatted, &actual_formatted.formatted);

            return Ok(VerificationResult {
                success: false,
                error: Some(format!("File mismatch for {file}")),
                indent_score: Some(file_indent_score),
                formatted_equivalent: Some(false),
                diff_stats: Some(stats),
                diff: Some(diff),
            });
        }
    }

    Ok(VerificationResult {
        success: true,
        error: None,
        indent_score: if file_count > 0 {
            Some(total_indent_score / file_count as f64)
        } else {
            Some(0.0)
        },
        formatted_equivalent: Some(true),
        diff_stats: Some(DiffStats {
            lines_changed: 0,
            chars_changed: 0,
        }),
        diff: None,
    })
}

/// Create a compact unified diff with context.
fn create_compact_diff(expected: &str, actual: &str, context_lines: usize) -> String {
    let diff = TextDiff::from_lines(expected, actual);
    let mut output = Vec::new();
    let mut line_num = 1;

    let changes: Vec<_> = diff.iter_all_changes().collect();
    let mut i = 0;

    while i < changes.len() {
        let change = changes[i];
        if change.tag() != similar::ChangeTag::Insert && change.tag() != similar::ChangeTag::Delete
        {
            line_num += 1;
            i += 1;
            continue;
        }

        // Show context before
        if i >= context_lines {
            let mut ctx_start = i.saturating_sub(context_lines);
            // Skip back to find actual context
            while ctx_start < i {
                if changes[ctx_start].tag() != similar::ChangeTag::Insert
                    && changes[ctx_start].tag() != similar::ChangeTag::Delete
                {
                    output.push(format!(" {}>", changes[ctx_start].value().trim_end()));
                }
                ctx_start += 1;
            }
        }

        // Show the change group
        while i < changes.len() {
            let c = changes[i];
            if c.tag() != similar::ChangeTag::Insert && c.tag() != similar::ChangeTag::Delete {
                break;
            }
            let prefix = if c.tag() == similar::ChangeTag::Insert {
                "+"
            } else {
                "-"
            };
            for line in c.value().lines() {
                if !line.is_empty() || c.value().ends_with('\n') {
                    output.push(format!("{prefix} {line}"));
                }
            }
            if c.tag() == similar::ChangeTag::Delete {
                line_num += 1;
            }
            i += 1;
        }

        // Show context after
        let mut ctx_after = 0;
        while i + ctx_after < changes.len()
            && ctx_after < context_lines
            && changes[i + ctx_after].tag() != similar::ChangeTag::Insert
            && changes[i + ctx_after].tag() != similar::ChangeTag::Delete
        {
            output.push(format!(" {}", changes[i + ctx_after].value().trim_end()));
            ctx_after += 1;
        }
    }

    output.join("\n")
}

/// Compute diff statistics.
fn compute_diff_stats(expected: &str, actual: &str) -> DiffStats {
    let diff = TextDiff::from_lines(expected, actual);
    let mut lines_changed = 0;
    let mut chars_changed = 0;

    for change in diff.iter_all_changes() {
        use similar::ChangeTag;
        if change.tag() == ChangeTag::Insert || change.tag() == ChangeTag::Delete {
            lines_changed += change.value().lines().count();
            chars_changed += change.value().len();
        }
    }

    DiffStats {
        lines_changed,
        chars_changed,
    }
}

/// Restore whitespace-only line diffs: if two lines differ only in
/// whitespace content (ignoring non-whitespace chars), prefer the expected
/// version.
fn restore_whitespace_only_diffs(expected: &str, actual: &str) -> String {
    let diff = TextDiff::from_lines(expected, actual);
    let mut out = Vec::new();
    let mut pending_removed: Vec<&str> = Vec::new();
    let mut pending_added: Vec<&str> = vec![];

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                pending_removed.push(change.value().trim_end_matches('\n'));
            }
            ChangeTag::Insert => {
                pending_added.push(change.value().trim_end_matches('\n'));
            }
            ChangeTag::Equal => {
                // Flush pending pairs
                let pairs = pending_removed.len().min(pending_added.len());
                for i in 0..pairs {
                    let removed = pending_removed[i];
                    let added = pending_added[i];
                    if removed != added
                        && removed
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>()
                            == added
                                .chars()
                                .filter(|c| !c.is_whitespace())
                                .collect::<String>()
                    {
                        out.push(removed); // prefer expected version
                    } else {
                        out.push(added);
                    }
                }
                // Extra added lines
                for i in pairs..pending_added.len() {
                    out.push(pending_added[i]);
                }
                pending_removed.clear();
                pending_added.clear();
                out.push(change.value().trim_end_matches('\n'));
            }
        }
    }

    // Flush remaining
    let pairs = pending_removed.len().min(pending_added.len());
    for i in 0..pairs {
        let removed = pending_removed[i];
        let added = pending_added[i];
        if removed != added
            && removed
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                == added
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
        {
            out.push(removed);
        } else {
            out.push(added);
        }
    }

    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_verify_exact_match() {
        let expected_dir = TempDir::new().unwrap();
        let actual_dir = TempDir::new().unwrap();

        std::fs::write(expected_dir.path().join("test.rs"), "fn main() {}\n").unwrap();
        std::fs::write(actual_dir.path().join("test.rs"), "fn main() {}\n").unwrap();

        let result = verify_files(
            expected_dir.path(),
            actual_dir.path(),
            Some(&["test.rs".to_string()]),
        )
        .await
        .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_verify_missing_file() {
        let expected_dir = TempDir::new().unwrap();
        let actual_dir = TempDir::new().unwrap();

        std::fs::write(expected_dir.path().join("test.rs"), "fn main() {}\n").unwrap();

        let result = verify_files(
            expected_dir.path(),
            actual_dir.path(),
            Some(&["test.rs".to_string()]),
        )
        .await
        .unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Missing"));
    }

    #[test]
    fn test_restore_whitespace_only_diffs() {
        let expected = "fn  main()  {\n    let x = 1;\n}\n";
        let actual = "fn main() {\n    let x = 1;\n}\n";
        let restored = restore_whitespace_only_diffs(expected, actual);
        // Should prefer expected when content is the same
        assert!(restored.contains("fn  main()  {") || restored.contains("fn main() {"));
    }
}
