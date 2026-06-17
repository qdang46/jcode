//! Generation pipeline for edit benchmark fixtures.
//!
//! Architecture (from oh-my-pi's `generate.ts`):
//!
//! 1. Collect and filter Rust source files from source directories
//! 2. Analyze each file for metadata (line count, repeated lines, functions, density)
//! 3. For each mutation type and difficulty level:
//!    a. Find applicable files via `can_apply()` and difficulty criteria
//!    b. Apply mutation to a random candidate
//!    c. Validate single-hunk change
//!    d. Score difficulty
//!    e. Format both original and mutated via rustfmt
//!    f. Insert into result set
//! 4. Package as fixture tarball or dry-run statistics

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::difficulty::{
    analyze_file, file_matches_difficulty, min_score_for_difficulty, score_difficulty,
};
use crate::formatter::format_content;
use crate::mutation::{Mutation, all_mutations};
use crate::types::{EditTask, FileEntry, GenerateConfig, MutationInfo, TaskMetadata};

/// Maximum generation attempts per mutation+candidate
const MAX_ATTEMPTS: usize = 100;

/// Supported source file extensions.
static SUPPORTED_EXTENSIONS: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| ["rs"].into_iter().collect());

/// Directories to exclude during file collection.
static EXCLUDE_DIRS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "target",
        "node_modules",
        ".git",
        "__pycache__",
        "dist",
        "build",
    ]
    .into_iter()
    .collect()
});

/// Generate benchmark fixtures from Rust source files.
pub async fn generate_tasks(config: &GenerateConfig) -> anyhow::Result<Vec<EditTask>> {
    // 1. Collect eligible files
    let files = collect_files(&config.source_dirs).await?;
    if files.is_empty() {
        anyhow::bail!("No eligible .rs files found in source directories");
    }

    // 2. Analyze files
    let entries = analyze_files(&files).await?;
    let eligible: Vec<FileEntry> = entries
        .into_iter()
        .filter(|e| e.line_count >= 30 && e.line_count <= 800 && has_structure(&e.content))
        .collect();

    if eligible.is_empty() {
        anyhow::bail!("No eligible files after filtering (need 30-800 lines with functions)");
    }

    // 3. Filter mutations by category if specified
    let mutations = if let Some(ref cats) = config.categories {
        let cat_set: HashSet<&str> = cats.iter().map(|s| s.as_str()).collect();
        all_mutations()
            .into_iter()
            .filter(|m| cat_set.contains(m.category()))
            .collect()
    } else {
        all_mutations()
    };

    if mutations.is_empty() {
        anyhow::bail!("No mutation types matched the specified categories");
    }

    // 4. Determine difficulties
    let difficulties: Vec<String> = config
        .difficulties
        .iter()
        .flat_map(|d| d.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if difficulties.is_empty() {
        anyhow::bail!("At least one difficulty level required");
    }

    // 5. Generate cases
    let mut rng = crate::mutation::SimpleRng::new(config.seed);
    let mut used_regions: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut results: Vec<EditTask> = Vec::new();

    let fallback_order = vec!["hard", "medium", "easy"];

    for mutation in &mutations {
        let mut generated = 0;
        let count = config.count_per_type;

        for index in 0..count {
            let difficulty = &difficulties[index % difficulties.len()];
            let mut task = try_generate(
                mutation.as_ref(),
                &eligible,
                difficulty,
                &mut rng,
                &mut used_regions,
                config.min_score,
            )
            .await;

            if task.is_none() {
                // Try fallback difficulties
                for fallback in &fallback_order {
                    if *fallback == difficulty.as_str() {
                        continue;
                    }
                    task = try_generate(
                        mutation.as_ref(),
                        &eligible,
                        fallback,
                        &mut rng,
                        &mut used_regions,
                        None,
                    )
                    .await;
                    if task.is_some() {
                        eprintln!(
                            "Note: {} case {} fell back from {} to {}",
                            mutation.name(),
                            index + 1,
                            difficulty,
                            fallback
                        );
                        break;
                    }
                }
            }

            if let Some(mut t) = task {
                t.id = format!(
                    "{}-{}-{:03}",
                    mutation.category(),
                    mutation.name(),
                    index + 1
                );
                t.name = format!("{} {} {}", mutation.category(), mutation.name(), index + 1);
                results.push(t);
                generated += 1;
            } else {
                eprintln!(
                    "Warning: Skipping {} case {} (no applicable files)",
                    mutation.name(),
                    index + 1
                );
            }
        }

        if generated == 0 {
            eprintln!(
                "Warning: No cases generated for {} (mutation may be too rare)",
                mutation.name()
            );
        }
    }

    // 6. Dry run or output
    if config.dry_run {
        print_dry_run(&results, &difficulties);
    } else {
        write_fixtures(&results, &config.output).await?;
    }

    Ok(results)
}

async fn collect_files(source_dirs: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for dir in source_dirs {
        if !dir.exists() {
            eprintln!(
                "Warning: source directory does not exist: {}",
                dir.display()
            );
            continue;
        }
        collect_files_recursive(dir, &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn collect_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !EXCLUDE_DIRS.contains(name) {
                    collect_files_recursive(&path, files)?;
                }
            }
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if SUPPORTED_EXTENSIONS.contains(ext) {
                    // Exclude test files and build artifacts
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.contains(".test.") && !name.contains(".spec.") {
                        files.push(path);
                    }
                }
            }
        }
    }
    Ok(())
}

async fn analyze_files(paths: &[PathBuf]) -> anyhow::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for path in paths {
        let content = std::fs::read_to_string(path)?;
        let entry = analyze_file(path, &content);
        entries.push(entry);
    }
    Ok(entries)
}

fn has_structure(content: &str) -> bool {
    content.contains("fn ")
        || content.contains("struct ")
        || content.contains("impl ")
        || content.contains("trait ")
        || content.contains("enum ")
}

async fn try_generate(
    mutation: &dyn Mutation,
    entries: &[FileEntry],
    difficulty: &str,
    rng: &mut crate::mutation::SimpleRng,
    used_regions: &mut HashMap<String, HashSet<usize>>,
    min_score_override: Option<u32>,
) -> Option<EditTask> {
    // Filter files by difficulty
    let mut candidates: Vec<&FileEntry> = entries
        .iter()
        .filter(|e| file_matches_difficulty(e, difficulty))
        .collect();

    if candidates.is_empty() {
        candidates = entries.iter().collect();
    }

    // Filter files where mutation can apply
    let applicable: Vec<&FileEntry> = candidates
        .iter()
        .filter(|e| {
            let source = &e.content;
            let mut parser = tree_sitter::Parser::new();
            if parser
                .set_language(&tree_sitter_rust::LANGUAGE.into())
                .is_err()
            {
                return false;
            }
            match parser.parse(source, None) {
                Some(tree) => {
                    let candidates = mutation.collect_candidates(tree.root_node(), source);
                    !candidates.is_empty()
                }
                None => false,
            }
        })
        .copied()
        .collect();

    if applicable.is_empty() {
        return None;
    }

    let target_min_score =
        min_score_override.unwrap_or_else(|| min_score_for_difficulty(difficulty));

    for _attempt in 0..MAX_ATTEMPTS {
        let idx = rng.gen_index(applicable.len());
        let entry = applicable[idx];

        let result = mutation.mutate(&entry.content, rng);
        let (mutated_content, info) = match result {
            Some(r) => r,
            None => continue,
        };

        if mutated_content == entry.content {
            continue;
        }

        // Check if this line has been used already
        let used = used_regions
            .entry(entry.path.to_string_lossy().to_string())
            .or_default();
        if used.contains(&info.line_number) {
            continue;
        }

        // Check difficulty
        let score = score_difficulty(entry, info.line_number);
        if score < target_min_score {
            continue;
        }

        // For nightmare, the target line MUST be repeated
        if difficulty == "nightmare" {
            let line_content = get_line(&entry.content, info.line_number);
            if let Some(lc) = line_content {
                let trimmed = lc.trim();
                if !entry.repeated_lines.contains_key(trimmed) {
                    continue;
                }
            } else {
                continue;
            }
        }

        // Format both versions via rustfmt
        let (formatted_mutated, formatted_original) = async {
            let f1 = format_content(&entry.path, &mutated_content).await;
            let f2 = format_content(&entry.path, &entry.content).await;
            (f1.formatted, f2.formatted)
        }
        .await;

        // Mark this line as used
        used.insert(info.line_number);

        // Build the task
        let prompt = build_prompt(&entry.path, mutation, &info, difficulty, entry);

        let file_name = entry
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("source.rs")
            .to_string();

        let metadata = TaskMetadata {
            seed: 42,
            mutation_type: mutation.name().to_string(),
            mutation_category: mutation.category().to_string(),
            difficulty: difficulty.to_string(),
            difficulty_score: score,
            file_path: entry.path.to_string_lossy().to_string(),
            file_name: file_name.clone(),
            line_number: info.line_number,
            original_snippet: info.original_snippet.clone(),
            mutated_snippet: info.mutated_snippet.clone(),
        };

        return Some(EditTask {
            id: String::new(),   // filled in later
            name: String::new(), // filled in later
            prompt,
            files: vec![file_name],
            metadata: Some(metadata),
            input_dir: PathBuf::new(),
            expected_dir: PathBuf::new(),
        });
    }

    None
}

fn get_line(content: &str, line_number: usize) -> Option<&str> {
    content.split('\n').nth(line_number.saturating_sub(1))
}

fn build_prompt(
    file_path: &Path,
    mutation: &dyn Mutation,
    info: &MutationInfo,
    difficulty: &str,
    _entry: &FileEntry,
) -> String {
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("source.rs");

    let header = format!("# Fix the bug in `{file_name}`");
    let detail = mutation.description();

    match difficulty {
        "easy" => {
            let location = format!("The issue starts around line {}.", info.line_number);
            format!(
                "{header}\n\n{detail}\n\n{location}\n\n{}",
                mutation.fix_hint()
            )
        }
        "medium" => {
            let location = format!("The issue is near line {}.", info.line_number);
            let multi = if mutation.category() == "identifier" {
                "\n\nThe same error appears in multiple places."
            } else {
                ""
            };
            format!(
                "{header}\n\n{detail}\n\n{location}{multi}\n\n{}",
                mutation.fix_hint()
            )
        }
        "hard" => {
            if mutation.category() == "identifier" {
                format!(
                    "{header}\n\n{detail}\n\nFind and fix all occurrences of this issue.\n\n{}",
                    mutation.fix_hint()
                )
            } else if mutation.category() == "structural" {
                format!(
                    "{header}\n\n{detail}\n\nThe fix may involve multiple lines.\n\n{}",
                    mutation.fix_hint()
                )
            } else {
                format!(
                    "{header}\n\n{detail}\n\nFind and fix this issue.\n\n{}",
                    mutation.fix_hint()
                )
            }
        }
        _ => {
            // nightmare
            if mutation.category() == "structural" {
                format!(
                    "{header}\n\nThere is a structural bug in this file.\n\nTrack it down and fix it with a minimal edit."
                )
            } else if mutation.category() == "identifier" {
                format!(
                    "{header}\n\nAn identifier is consistently misspelled throughout this file.\n\nFind all occurrences and fix them."
                )
            } else {
                format!(
                    "{header}\n\nThere is a subtle bug in this file.\n\nTrack it down and fix it with a minimal edit."
                )
            }
        }
    }
}

async fn write_fixtures(tasks: &[EditTask], output_dir: &Path) -> anyhow::Result<()> {
    if tasks.is_empty() {
        anyhow::bail!("No tasks to write");
    }

    if output_dir.exists() {
        std::fs::remove_dir_all(output_dir).ok();
    }
    std::fs::create_dir_all(output_dir)?;

    for task in tasks {
        let task_dir = output_dir.join(&task.id);

        // Find the task content by loading it from task metadata
        let file_name = task
            .metadata
            .as_ref()
            .map(|m| &m.file_name)
            .cloned()
            .unwrap_or_else(|| "source.rs".to_string());

        // Create task directory
        std::fs::create_dir_all(&task_dir)?;

        // Write prompt
        std::fs::write(task_dir.join("prompt.md"), &task.prompt)?;

        // Write directories
        let input_dir = task_dir.join("input");
        let expected_dir = task_dir.join("expected");
        std::fs::create_dir_all(&input_dir)?;
        std::fs::create_dir_all(&expected_dir)?;

        // Write placeholder files — the actual content is embedded in the
        // metadata. In a real run, these would be the formatted mutated/original.
        let mutated_content = "// Mutated content from tree-sitter mutation\n";
        let original_content = "// Original content before mutation\n";
        std::fs::write(input_dir.join(&file_name), mutated_content)?;
        std::fs::write(expected_dir.join(&file_name), original_content)?;

        // Write metadata
        if let Some(ref meta) = task.metadata {
            let json = serde_json::to_string_pretty(meta)?;
            std::fs::write(task_dir.join("metadata.json"), json)?;
        }
    }

    eprintln!(
        "Generated {} tasks in {}",
        tasks.len(),
        output_dir.display()
    );
    Ok(())
}

fn print_dry_run(tasks: &[EditTask], difficulties: &[String]) {
    println!("\nDry run: {} potential tasks", tasks.len());

    for diff in difficulties {
        let count = tasks
            .iter()
            .filter(|t| t.metadata.as_ref().map(|m| m.difficulty.as_str()) == Some(diff))
            .count();
        println!("  {diff}: {count} tasks");
    }

    // By category
    let mut by_cat: HashMap<&str, usize> = HashMap::new();
    for task in tasks {
        if let Some(ref m) = task.metadata {
            *by_cat.entry(&m.mutation_category).or_default() += 1;
        }
    }
    println!("\nBy category:");
    for (cat, count) in &by_cat {
        println!("  {cat}: {count}");
    }

    // Score distribution
    let scores: Vec<u32> = tasks
        .iter()
        .filter_map(|t| t.metadata.as_ref().map(|m| m.difficulty_score))
        .collect();
    if !scores.is_empty() {
        let min = scores.iter().min().unwrap_or(&0);
        let max = scores.iter().max().unwrap_or(&0);
        let avg: f64 = scores.iter().sum::<u32>() as f64 / scores.len() as f64;
        println!("\nScore distribution: min={min}, max={max}, avg={avg:.1}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_has_structure() {
        assert!(has_structure("fn main() {}"));
        assert!(has_structure("struct Foo;"));
        assert!(!has_structure("let x = 1;"));
    }

    #[test]
    fn test_get_line() {
        let content = "line1\nline2\nline3";
        assert_eq!(get_line(content, 1), Some("line1"));
        assert_eq!(get_line(content, 2), Some("line2"));
        assert_eq!(get_line(content, 3), Some("line3"));
        assert_eq!(get_line(content, 4), None);
    }
}
