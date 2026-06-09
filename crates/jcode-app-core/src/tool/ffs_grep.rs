use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use ffs_search::directory_grep::{DirectoryGrepMatch, grep_directory};
use ffs_search::grep::has_regex_metacharacters;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

const MAX_RESULTS: usize = 100;

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

#[async_trait]
impl Tool for FfsGrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search files for a pattern. Uses ffs-search directory_grep (regex-based with gitignore-aware walk). Auto-detects whether the pattern contains regex metacharacters; you can override with `regex: true/false`."
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

        // ffs-search's grep_directory only supports regex.
        // For literal patterns we escape regex metacharacters so the regex
        // engine does a literal match — effectively the same result.
        let effective_pattern = if use_regex {
            pattern
        } else {
            regex::escape(&pattern)
        };

        let base_owned = base.to_path_buf();

        let matches: Vec<DirectoryGrepMatch> = tokio::task::spawn_blocking(move || {
            grep_directory(&base_owned, &effective_pattern, MAX_RESULTS)
        })
        .await?;

        let mut output = String::new();
        output.push_str(&format!(
            "Found {} matches for '{}'\n\n",
            matches.len(),
            pattern_for_display
        ));

        let mut current_file = String::new();
        for m in &matches {
            if m.path != current_file {
                if !current_file.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("{}:\n", m.path));
                current_file = m.path.clone();
            }
            output.push_str(&format!("  {:>4}: {}\n", m.line_number, m.line));
        }

        if matches.len() >= MAX_RESULTS {
            output.push_str(&format!(
                "\n... results truncated at {} matches",
                MAX_RESULTS
            ));
        }

        Ok(ToolOutput::new(output))
    }
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
    fn test_grep_basic() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\nhello again\n");

        let results = grep_directory(dir.path(), "hello", 10);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.line.contains("hello")));
    }

    #[test]
    fn test_grep_no_match() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\n");

        let results = grep_directory(dir.path(), "nonexistent", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_grep_regex() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\nhello123\n");

        let results = grep_directory(dir.path(), r"hello\d+", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line, "hello123");
    }

    #[test]
    fn test_grep_regex_no_match() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "test.txt", "hello world\nfoo bar\n");

        let results = grep_directory(dir.path(), r"^\d+$", 10);
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
    fn test_grep_skips_binary_extensions() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "source.rs", "hello world\n");
        // Binary extension — should be skipped even if it contains the pattern
        create_test_file(&dir, "image.png", "hello world\n");

        let results = grep_directory(dir.path(), "hello", 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].path.contains("source.rs"));
    }
}
