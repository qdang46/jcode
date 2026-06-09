//! Fixture I/O utilities for reading/writing benchmark task directories.
//!
//! Fixtures are structured as:
//! ```text
//! fixtures/
//!   <caseId>/
//!     prompt.md       - Task prompt
//!     input/          - Mutated (buggy) source files
//!     expected/       - Original (correct) source files
//!     metadata.json   - Structured metadata
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::types::{EditTask, TaskMetadata};

/// Load all tasks from a fixtures directory.
///
/// Each subdirectory under `fixtures_dir` is expected to contain:
/// - `prompt.md` - task prompt
/// - `input/` - mutated source files
/// - `expected/` - expected source files
/// - `metadata.json` (optional) - structured metadata
pub fn load_tasks_from_dir(fixtures_dir: &Path) -> anyhow::Result<Vec<EditTask>> {
    let mut tasks = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(fixtures_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in &entries {
        let challenge_dir = entry.path();
        let case_id = entry.file_name();
        let case_id = case_id.to_string_lossy().to_string();

        let prompt_path = challenge_dir.join("prompt.md");
        let input_dir = challenge_dir.join("input");
        let expected_dir = challenge_dir.join("expected");
        let metadata_path = challenge_dir.join("metadata.json");

        // Read prompt
        let prompt = std::fs::read_to_string(&prompt_path)
            .map_err(|e| anyhow::anyhow!("Missing prompt.md in {case_id}: {e}"))?
            .trim()
            .to_string();

        // Verify directories exist
        if !input_dir.is_dir() {
            anyhow::bail!("Missing input/ directory in {case_id}");
        }
        if !expected_dir.is_dir() {
            anyhow::bail!("Missing expected/ directory in {case_id}");
        }

        // List files
        let files = list_files(&input_dir)?;

        // Read metadata (optional)
        let metadata = if metadata_path.exists() {
            let text = std::fs::read_to_string(&metadata_path)?;
            Some(serde_json::from_str::<TaskMetadata>(&text)?)
        } else {
            None
        };

        // Derive name from case_id
        let name = case_id
            .split(|c: char| c == '-' || c == '_')
            .map(|part| {
                let mut c = part.chars();
                match c.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        tasks.push(EditTask {
            id: case_id,
            name,
            prompt,
            files,
            metadata,
            input_dir,
            expected_dir,
        });
    }

    Ok(tasks)
}

/// Recursively list files in a directory, returning relative paths.
pub fn list_files(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        list_files_recursive(dir, dir, &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn list_files_recursive(root: &Path, dir: &Path, files: &mut Vec<String>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            list_files_recursive(root, &entry_path, files)?;
        } else if entry_path.is_file() {
            let relative = entry_path
                .strip_prefix(root)
                .unwrap_or(&entry_path)
                .to_string_lossy()
                .to_string();
            files.push(relative);
        }
    }
    Ok(())
}

/// Save a task to a directory structure.
pub fn save_task(
    task_dir: &Path,
    task: &EditTask,
    mutated_content: &str,
    original_content: &str,
) -> anyhow::Result<()> {
    let input_dir = task_dir.join("input");
    let expected_dir = task_dir.join("expected");

    std::fs::create_dir_all(&input_dir)?;
    std::fs::create_dir_all(&expected_dir)?;

    // Write mutated content to input/
    for file in &task.files {
        std::fs::write(input_dir.join(file), mutated_content)?;
        std::fs::write(expected_dir.join(file), original_content)?;
    }

    // Write prompt
    std::fs::write(task_dir.join("prompt.md"), &task.prompt)?;

    // Write metadata
    if let Some(ref meta) = task.metadata {
        let json = serde_json::to_string_pretty(meta)?;
        std::fs::write(task_dir.join("metadata.json"), json)?;
    }

    Ok(())
}

/// Validate fixture integrity for all tasks in a directory.
#[derive(Debug)]
pub struct FixtureValidationIssue {
    pub task_id: String,
    pub message: String,
}

pub fn validate_fixtures(fixtures_path: &Path) -> Vec<FixtureValidationIssue> {
    let mut issues = Vec::new();

    let Ok(entries) = std::fs::read_dir(fixtures_path) else {
        issues.push(FixtureValidationIssue {
            task_id: "(root)".into(),
            message: format!("Cannot read directory: {}", fixtures_path.display()),
        });
        return issues;
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let task_dir = entry.path();
        let task_id = entry.file_name().to_string_lossy().to_string();
        let prompt_path = task_dir.join("prompt.md");
        let input_dir = task_dir.join("input");
        let expected_dir = task_dir.join("expected");
        let metadata_path = task_dir.join("metadata.json");

        if !prompt_path.exists() {
            issues.push(FixtureValidationIssue {
                task_id: task_id.clone(),
                message: "prompt.md is missing".into(),
            });
        } else if let Ok(content) = std::fs::read_to_string(&prompt_path) {
            if content.trim().is_empty() {
                issues.push(FixtureValidationIssue {
                    task_id: task_id.clone(),
                    message: "prompt.md is empty".into(),
                });
            }
        }

        if !input_dir.is_dir() {
            issues.push(FixtureValidationIssue {
                task_id: task_id.clone(),
                message: "input/ directory is missing".into(),
            });
        }

        if !expected_dir.is_dir() {
            issues.push(FixtureValidationIssue {
                task_id: task_id.clone(),
                message: "expected/ directory is missing".into(),
            });
        }

        if let Ok(input_files) = list_files(&input_dir) {
            if input_files.is_empty() {
                issues.push(FixtureValidationIssue {
                    task_id: task_id.clone(),
                    message: "input/ is empty".into(),
                });
            }
        }

        if !metadata_path.exists() {
            issues.push(FixtureValidationIssue {
                task_id: task_id.clone(),
                message: "metadata.json is missing".into(),
            });
        }
    }

    issues
}
