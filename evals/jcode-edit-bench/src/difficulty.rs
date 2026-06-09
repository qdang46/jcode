//! Difficulty scoring for mutation-based benchmarks.
//!
//! Scores are computed per-file per-mutation using factors like
//! file length, repeated lines, code density, and nesting depth.
//! Based on oh-my-pi's `scoreDifficulty` in `generate.ts`.

use std::collections::HashMap;

use crate::types::FileEntry;

/// Compute a difficulty score (0-20) for a mutation at the given line in a file.
///
/// Factors:
/// - File length: >300→+3, >150→+1
/// - Middle of file: line in 33-66%→+2
/// - Repeated lines: n repeats→+min(n,5)
/// - Similar function blocks: ≥5→+3, ≥3→+1
/// - Code density: >0.75→+2, >0.65→+1
/// - Nesting depth: indent≥16→+2, indent≥8→+1
pub fn score_difficulty(entry: &FileEntry, line_number: usize) -> u32 {
    let mut score: u32 = 0;

    // File length
    if entry.line_count > 300 {
        score += 3;
    } else if entry.line_count > 150 {
        score += 1;
    }

    // Middle of file
    let middle_start = entry.line_count / 3;
    let middle_end = (entry.line_count * 2) / 3;
    if line_number >= middle_start && line_number <= middle_end {
        score += 2;
    }

    // Repeated lines
    if let Some(line_content) = get_line_content(entry, line_number) {
        if let Some(positions) = entry.repeated_lines.get(line_content.trim()) {
            let repeats = positions.len();
            score += std::cmp::min(repeats as u32, 5);
        }
    }

    // Similar function blocks
    if entry.function_count >= 5 {
        score += 3;
    } else if entry.function_count >= 3 {
        score += 1;
    }

    // Code density
    if entry.density > 0.75 {
        score += 2;
    } else if entry.density > 0.65 {
        score += 1;
    }

    // Nesting depth
    if let Some(line_content) = get_line_content(entry, line_number) {
        let indent = line_content.len() - line_content.trim_start().len();
        if indent >= 16 {
            score += 2;
        } else if indent >= 8 {
            score += 1;
        }
    }

    score
}

fn get_line_content<'a>(entry: &'a FileEntry, line_number: usize) -> Option<&'a str> {
    let lines: Vec<&str> = entry.content.split('\n').collect();
    if line_number >= 1 && line_number <= lines.len() {
        Some(lines[line_number - 1])
    } else {
        None
    }
}

/// Minimum difficulty score for each level.
pub fn min_score_for_difficulty(difficulty: &str) -> u32 {
    match difficulty {
        "easy" => 0,
        "medium" => 2,
        "hard" => 5,
        "nightmare" => 8,
        _ => 0,
    }
}

/// Determine difficulty zones.
pub fn difficulty_from_score(score: u32) -> &'static str {
    if score >= 8 {
        "nightmare"
    } else if score >= 5 {
        "hard"
    } else if score >= 2 {
        "medium"
    } else {
        "easy"
    }
}

/// Analyze a file's repeated lines, function count, density, and max indent.
pub fn analyze_file(path: &std::path::Path, content: &str) -> FileEntry {
    let lines: Vec<&str> = content.split('\n').collect();
    let line_count = lines.len();

    // Repeated lines
    let mut repeated_lines: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.len() >= 10 && trimmed != "{" && trimmed != "}" && trimmed != "};" {
            repeated_lines
                .entry(trimmed.to_string())
                .or_default()
                .push(i + 1);
        }
    }
    // Remove singletons
    repeated_lines.retain(|_, v| v.len() > 1);

    // Function count
    let function_count = content.matches("fn ").count();

    // Density
    let non_whitespace = content.chars().filter(|c| !c.is_whitespace()).count();
    let density = if content.is_empty() {
        0.0
    } else {
        non_whitespace as f64 / content.len() as f64
    };

    // Max indent
    let max_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .max()
        .unwrap_or(0);

    FileEntry {
        path: path.to_path_buf(),
        content: content.to_string(),
        line_count,
        repeated_lines,
        function_count,
        density,
        max_indent,
    }
}

/// Check if file is eligible for a given difficulty level.
pub fn file_matches_difficulty(entry: &FileEntry, difficulty: &str) -> bool {
    match difficulty {
        "easy" => entry.line_count < 150 && entry.repeated_lines.len() < 3,
        "medium" => entry.line_count >= 100 && entry.line_count <= 300,
        "hard" => entry.line_count > 200 && entry.function_count >= 3,
        "nightmare" => !entry.repeated_lines.is_empty() && entry.line_count > 200,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_file_is_easy() {
        let content = "fn a() -> i32 { 1 }\nfn b() -> i32 { 2 }\n";
        let entry = analyze_file(std::path::Path::new("test.rs"), content);
        let score = score_difficulty(&entry, 1);
        assert!(score <= 2);
    }

    #[test]
    fn test_analyze_function_count() {
        let content = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n";
        let entry = analyze_file(std::path::Path::new("test.rs"), content);
        assert_eq!(entry.function_count, 4);
    }

    #[test]
    fn test_density_computation() {
        let dense = "fn f(){let x=1;x+2}";
        let sparse = "fn f() {\n    let x = 1;\n    x + 2\n}";
        let d_entry = analyze_file(std::path::Path::new("t.rs"), dense);
        let s_entry = analyze_file(std::path::Path::new("t.rs"), sparse);
        assert!(d_entry.density > s_entry.density);
    }
}
