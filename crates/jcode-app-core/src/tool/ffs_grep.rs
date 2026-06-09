use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use memchr::memmem::Finder;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use super::binary_ext::is_binary_extension;
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_RESULTS: usize = 100;
const MAX_LINE_LEN: usize = 2000;

pub struct FfsGrepTool;

impl FfsGrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct FfsGrepInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    regex: Option<bool>,
}

#[derive(Clone)]
struct GrepResult {
    file: String,
    line_num: usize,
    line: String,
}

#[async_trait]
impl Tool for FfsGrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search files for a pattern. Uses SIMD-accelerated matching (via memchr) for literal patterns and regex for complex patterns. Auto-detects whether the pattern contains regex metacharacters; you can override with `regex: true/false`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "intent": super::intent_schema_property(),
                "pattern": {
                    "type": "string",
                    "description": "Search pattern (literal by default, regex if metacharacters detected or regex=true)."
                },
                "path": {
                    "type": "string",
                    "description": "Search path (defaults to current directory)."
                },
                "regex": {
                    "type": "boolean",
                    "description": "Force regex mode. When omitted, auto-detects from pattern content."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: FfsGrepInput = serde_json::from_value(input)?;

        let base_path_str = params.path.clone().unwrap_or_else(|| ".".to_string());
        let base = ctx.resolve_path(Path::new(&base_path_str));

        if !base.exists() {
            return Err(anyhow::anyhow!("Directory not found: {}", base_path_str));
        }

        let use_regex = params
            .regex
            .unwrap_or_else(|| has_regex_metacharacters(&params.pattern));
        let pattern = params.pattern.clone();
        let pattern_for_display = params.pattern.clone();

        let results = tokio::task::spawn_blocking({
            let pattern = pattern;
            move || {
                if use_regex {
                    grep_blocking_regex(&base, &pattern)
                } else {
                    grep_blocking_literal(&base, &pattern)
                }
            }
        })
        .await??;

        let mut output = String::new();
        output.push_str(&format!(
            "Found {} matches for '{}'\n\n",
            results.len(),
            pattern_for_display
        ));

        let mut current_file = String::new();
        for result in &results {
            if result.file != current_file {
                if !current_file.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("{}:\n", result.file));
                current_file = result.file.clone();
            }
            output.push_str(&format!("  {:>4}: {}\n", result.line_num, result.line));
        }

        if results.len() >= MAX_RESULTS {
            output.push_str(&format!(
                "\n... results truncated at {} matches",
                MAX_RESULTS
            ));
        }

        Ok(ToolOutput::new(output))
    }
}

/// Check if a pattern contains regex metacharacters.
fn has_regex_metacharacters(pattern: &str) -> bool {
    pattern.contains([
        '.', '*', '+', '?', '(', ')', '[', ']', '{', '}', '^', '$', '|', '\\',
    ])
}

/// Literal grep using SIMD-accelerated memchr::memmem::Finder.
fn grep_blocking_literal(base: &Path, pattern: &str) -> Result<Vec<GrepResult>> {
    let finder = Finder::new(pattern);
    let pattern_lower = pattern.to_ascii_lowercase();
    let finder_lower = Finder::new(&pattern_lower);
    let is_case_sensitive = pattern.chars().any(|c| c.is_ascii_uppercase());

    let hit_count = Arc::new(AtomicUsize::new(0));
    let results = Arc::new(std::sync::Mutex::new(Vec::new()));

    let walker = ignore::WalkBuilder::new(base)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .threads(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(8),
        )
        .build_parallel();

    let base_owned = base.to_path_buf();

    walker.run(|| {
        let finder = finder.clone();
        let finder_lower = finder_lower.clone();
        let is_case_sensitive = is_case_sensitive;
        let hit_count = hit_count.clone();
        let results = results.clone();
        let base = base_owned.clone();

        Box::new(move |entry| {
            if hit_count.load(Ordering::Relaxed) >= MAX_RESULTS {
                return ignore::WalkState::Quit;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => return ignore::WalkState::Continue,
            };

            let path = entry.path();

            let ft = match entry.file_type() {
                Some(ft) => ft,
                None => return ignore::WalkState::Continue,
            };
            if ft.is_dir() {
                return ignore::WalkState::Continue;
            }

            if is_binary_extension(path) {
                return ignore::WalkState::Continue;
            }

            if let Ok(content) = std::fs::read_to_string(path) {
                let mut local_results = Vec::new();
                for (line_num, line) in content.lines().enumerate() {
                    let matched = if is_case_sensitive {
                        finder.find(line.as_bytes()).is_some()
                    } else {
                        finder_lower
                            .find(line.to_ascii_lowercase().as_bytes())
                            .is_some()
                    };

                    if matched {
                        let relative = path
                            .strip_prefix(&base)
                            .unwrap_or(path)
                            .display()
                            .to_string();

                        let truncated = if line.len() > MAX_LINE_LEN {
                            let truncated_str = crate::util::truncate_str(line, MAX_LINE_LEN);
                            format!("{}...", truncated_str)
                        } else {
                            line.to_string()
                        };

                        local_results.push(GrepResult {
                            file: relative,
                            line_num: line_num + 1,
                            line: truncated,
                        });

                        if hit_count.load(Ordering::Relaxed) + local_results.len() >= MAX_RESULTS {
                            break;
                        }
                    }
                }

                if !local_results.is_empty() {
                    let count = local_results.len();
                    hit_count.fetch_add(count, Ordering::Relaxed);
                    let mut guard = results
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.extend(local_results);
                }
            }

            ignore::WalkState::Continue
        })
    });

    let mut final_results = match Arc::try_unwrap(results) {
        Ok(mutex) => mutex
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        Err(arc) => arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
    };

    final_results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line_num.cmp(&b.line_num)));
    final_results.truncate(MAX_RESULTS);

    Ok(final_results)
}

/// Regex-based grep.
fn grep_blocking_regex(base: &Path, pattern: &str) -> Result<Vec<GrepResult>> {
    let regex = Regex::new(pattern)?;

    let hit_count = Arc::new(AtomicUsize::new(0));
    let results = Arc::new(std::sync::Mutex::new(Vec::new()));

    let walker = ignore::WalkBuilder::new(base)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .threads(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(8),
        )
        .build_parallel();

    let base_owned = base.to_path_buf();

    walker.run(|| {
        let regex = regex.clone();
        let hit_count = hit_count.clone();
        let results = results.clone();
        let base = base_owned.clone();

        Box::new(move |entry| {
            if hit_count.load(Ordering::Relaxed) >= MAX_RESULTS {
                return ignore::WalkState::Quit;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => return ignore::WalkState::Continue,
            };

            let path = entry.path();

            let ft = match entry.file_type() {
                Some(ft) => ft,
                None => return ignore::WalkState::Continue,
            };
            if ft.is_dir() {
                return ignore::WalkState::Continue;
            }

            if is_binary_extension(path) {
                return ignore::WalkState::Continue;
            }

            if let Ok(content) = std::fs::read_to_string(path) {
                let mut local_results = Vec::new();
                for (line_num, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        let relative = path
                            .strip_prefix(&base)
                            .unwrap_or(path)
                            .display()
                            .to_string();

                        let truncated = if line.len() > MAX_LINE_LEN {
                            let truncated_str = crate::util::truncate_str(line, MAX_LINE_LEN);
                            format!("{}...", truncated_str)
                        } else {
                            line.to_string()
                        };

                        local_results.push(GrepResult {
                            file: relative,
                            line_num: line_num + 1,
                            line: truncated,
                        });

                        if hit_count.load(Ordering::Relaxed) + local_results.len() >= MAX_RESULTS {
                            break;
                        }
                    }
                }

                if !local_results.is_empty() {
                    let count = local_results.len();
                    hit_count.fetch_add(count, Ordering::Relaxed);
                    let mut guard = results
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.extend(local_results);
                }
            }

            ignore::WalkState::Continue
        })
    });

    let mut final_results = match Arc::try_unwrap(results) {
        Ok(mutex) => mutex
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        Err(arc) => arc
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
    };

    final_results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line_num.cmp(&b.line_num)));
    final_results.truncate(MAX_RESULTS);

    Ok(final_results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_file(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_literal_grep_simple() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\nhello again\n");

        let base = dir.path().to_path_buf();
        let results = grep_blocking_literal(&base, "hello").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.line.contains("hello")));
    }

    #[test]
    fn test_literal_grep_no_match() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\n");

        let base = dir.path().to_path_buf();
        let results = grep_blocking_literal(&base, "nonexistent").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_regex_grep() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\nhello123\n");

        let base = dir.path().to_path_buf();
        let results = grep_blocking_regex(&base, r"hello\d+").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line, "hello123");
    }

    #[test]
    fn test_regex_grep_no_match() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\n");

        let base = dir.path().to_path_buf();
        let results = grep_blocking_regex(&base, r"^\d+$").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_has_regex_metacharacters() {
        assert!(has_regex_metacharacters(r"foo.bar"));
        assert!(has_regex_metacharacters(r"foo+"));
        assert!(has_regex_metacharacters(r"(foo)"));
        assert!(!has_regex_metacharacters("hello"));
        assert!(!has_regex_metacharacters("foo_bar"));
        assert!(has_regex_metacharacters(r"^foo"));
        assert!(has_regex_metacharacters(r"foo$"));
        assert!(has_regex_metacharacters(r"foo|bar"));
    }

    #[test]
    fn test_is_binary_extension() {
        assert!(is_binary_extension(Path::new("image.png")));
        assert!(is_binary_extension(Path::new("archive.zip")));
        assert!(is_binary_extension(Path::new("binary.exe")));
        assert!(!is_binary_extension(Path::new("text.rs")));
        assert!(!is_binary_extension(Path::new("README.md")));
        assert!(!is_binary_extension(Path::new("Makefile")));
    }

    #[test]
    fn test_literal_case_insensitive() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "Hello World\nfoo bar\nHELLO again\n");

        let base = dir.path().to_path_buf();
        // "Hello" has uppercase first letter, so case-sensitive search
        let results = grep_blocking_literal(&base, "Hello").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line, "Hello World");

        // "hello" is all lowercase, does case-insensitive match
        let results = grep_blocking_literal(&base, "hello").unwrap();
        assert_eq!(results.len(), 2);
    }
}
