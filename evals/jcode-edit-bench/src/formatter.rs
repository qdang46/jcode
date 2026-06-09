//! Rustfmt formatting wrapper for edit benchmark file normalization.
//!
//! Provides formatted comparison analogous to oh-my-pi's `formatter.ts`
//! (which uses Prettier) but adapted for Rust files via `rustfmt`.

use std::path::Path;
use std::process::Stdio;

/// Result of formatting a single file.
#[derive(Debug)]
pub struct FormatResult {
    pub formatted: String,
    pub did_format: bool,
}

/// Format Rust source code using `rustfmt`.
///
/// Falls back to the original content if rustfmt is unavailable or fails.
pub async fn format_content(file_path: &Path, content: &str) -> FormatResult {
    // Check if it's a Rust file
    let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "rs" {
        return FormatResult {
            formatted: content.to_string(),
            did_format: false,
        };
    }

    // Try to run rustfmt in stdin mode
    let result = tokio::process::Command::new("rustfmt")
        .arg("--edition")
        .arg("2021")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();

    match result {
        Ok(mut child) => {
            use tokio::io::AsyncWriteExt;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(content.as_bytes()).await;
                let _ = stdin.flush().await;
            }
            // Drop stdin so rustfmt sees EOF
            drop(child.stdin.take());

            let output = child.wait_with_output().await;
            match output {
                Ok(out) if out.status.success() => {
                    let formatted = String::from_utf8_lossy(&out.stdout).to_string();
                    FormatResult {
                        formatted,
                        did_format: true,
                    }
                }
                _ => FormatResult {
                    formatted: content.to_string(),
                    did_format: false,
                },
            }
        }
        Err(_) => FormatResult {
            formatted: content.to_string(),
            did_format: false,
        },
    }
}

/// Normalize line endings to LF.
pub fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace("\r", "\n")
}

/// Collapse runs of 2+ consecutive blank lines into a single blank line.
pub fn normalize_blank_lines(text: &str) -> String {
    let re = regex::Regex::new(r"\n{3,}").unwrap();
    re.replace_all(text, "\n\n").to_string()
}

/// Compute indent distance between raw and formatted content.
/// Measures how much the formatter had to fix indentation.
/// Lower is better; 0 means no indent correction needed.
pub fn compute_indent_score(raw: &str, formatted: &str) -> f64 {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(raw, formatted);
    let mut total_distance = 0.0;
    let mut samples = 0;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                let line = change.value();
                let indent = line.chars().take_while(|c| c.is_whitespace()).count();
                // Pair with the next addition
                if let Some(next_add) = diff
                    .iter_all_changes()
                    .skip_while(|c| c.value() != change.value())
                    .find(|c| c.tag() == ChangeTag::Insert)
                {
                    let add_indent = next_add
                        .value()
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count();
                    // Weight tabs as 2 spaces
                    let removed_weight = line
                        .chars()
                        .take(indent)
                        .map(|c| if c == '\t' { 2 } else { 1 })
                        .sum::<usize>();
                    let added_weight = next_add
                        .value()
                        .chars()
                        .take(add_indent)
                        .map(|c| if c == '\t' { 2 } else { 1 })
                        .sum::<usize>();
                    total_distance += (removed_weight as f64 - added_weight as f64).abs();
                    samples += 1;
                }
            }
            _ => {}
        }
    }

    if samples == 0 {
        0.0
    } else {
        total_distance / samples as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_line_endings() {
        assert_eq!(normalize_line_endings("hello\r\nworld\r"), "hello\nworld\n");
    }

    #[test]
    fn test_normalize_blank_lines() {
        let input = "a\n\n\n\nb";
        assert_eq!(normalize_blank_lines(input), "a\n\nb");
    }

    #[test]
    fn test_compute_indent_score_perfect() {
        let code = "fn f() {\n    let x = 1;\n}\n";
        assert_eq!(compute_indent_score(code, code), 0.0);
    }
}
