use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use globset::{Glob, GlobBuilder, GlobMatcher};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const DEFAULT_MAX_FILES: usize = 100;

pub struct FfsGlobTool;

impl FfsGlobTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct FfsGlobInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    fuzzy: Option<bool>,
    #[serde(default)]
    max_files: Option<usize>,
}

#[derive(Clone, Debug)]
struct FuzzyResult {
    path: String,
    score: usize,
    is_dir: bool,
}

#[async_trait]
impl Tool for FfsGlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files by glob pattern or fuzzy name match. In glob mode (default), uses globset::Glob for pattern matching with gitignore-aware parallel walk. In fuzzy mode (fuzzy=true), does case-insensitive substring matching against filenames and paths, scoring results."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "intent": super::intent_schema_property(),
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern or fuzzy name to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Base path (defaults to current directory)."
                },
                "fuzzy": {
                    "type": "boolean",
                    "description": "When true, performs case-insensitive fuzzy substring matching instead of glob matching."
                },
                "max_files": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 100)."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: FfsGlobInput = serde_json::from_value(input)?;

        let base_path_str = params.path.clone().unwrap_or_else(|| ".".to_string());
        let base = ctx.resolve_path(Path::new(&base_path_str));
        let pattern = params.pattern.clone();
        let max_files = params.max_files.unwrap_or(DEFAULT_MAX_FILES);

        if !base.exists() {
            return Err(anyhow::anyhow!("Directory not found: {}", base_path_str));
        }

        let use_fuzzy = params.fuzzy.unwrap_or(false);

        let results = tokio::task::spawn_blocking(move || {
            if use_fuzzy {
                fuzzy_search_blocking(&base, &pattern, max_files)
            } else {
                glob_search_blocking(&base, &pattern, max_files)
            }
        })
        .await??;

        let mut output = String::new();
        if use_fuzzy {
            output.push_str(&format!(
                "Fuzzy search for '{}' in {}: {} results\n\n",
                params.pattern,
                base_path_str,
                results.len()
            ));
            for (path_str, _score) in &results {
                output.push_str(path_str);
                output.push('\n');
            }
        } else {
            output.push_str(&format!(
                "Glob '{}' in {}: {} files\n\n",
                params.pattern,
                base_path_str,
                results.len()
            ));
            for (path_str, _mtime) in &results {
                output.push_str(path_str);
                output.push('\n');
            }
        }

        if results.len() >= max_files {
            output.push_str(&format!(
                "\n... results truncated (showing {} of more)",
                max_files
            ));
        }

        Ok(ToolOutput::new(output))
    }
}

/// Glob search using globset::Glob with gitignore-aware parallel walk.
fn glob_search_blocking(base: &Path, pattern: &str, max_files: usize) -> Result<Vec<(String, std::time::SystemTime)>> {
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|e| anyhow::anyhow!("Invalid glob pattern '{}': {}", pattern, e))?;
    let matcher = glob.compile_matcher();

    let collect_limit = max_files * 2;
    let results: Arc<Mutex<Vec<(String, std::time::SystemTime)>>> = Arc::new(Mutex::new(Vec::with_capacity(max_files)));
    let count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

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
        let matcher = matcher.clone();
        let results = results.clone();
        let count = count.clone();
        let base = base_owned.clone();

        Box::new(move |entry| {
            if count.load(Ordering::Relaxed) >= collect_limit {
                return ignore::WalkState::Quit;
            }

            let entry = match entry {
                Ok(e) => e,
                Err(_) => return ignore::WalkState::Continue,
            };

            let ft = match entry.file_type() {
                Some(ft) => ft,
                None => return ignore::WalkState::Continue,
            };
            if ft.is_dir() {
                return ignore::WalkState::Continue;
            }

            let path = entry.path();
            let relative = path.strip_prefix(&base).unwrap_or(path);
            let path_str = relative.to_string_lossy();

            if matcher.is_match(relative) {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);

                count.fetch_add(1, Ordering::Relaxed);
                let mut guard = results
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.push((path_str.to_string(), mtime));
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

    final_results.sort_by(|a, b| b.1.cmp(&a.1));
    final_results.truncate(max_files);

    Ok(final_results)
}

/// Fuzzy search: case-insensitive substring matching with scoring.
/// Uses sequential walk to avoid closure type issues.
fn fuzzy_search_blocking(base: &Path, query: &str, max_files: usize) -> Result<Vec<(String, std::time::SystemTime)>> {
    let query_lower = query.to_ascii_lowercase();
    let collect_limit = max_files * 2;

    struct Scored {
        path: String,
        mtime: std::time::SystemTime,
        score: usize,
    }

    let mut results: Vec<Scored> = Vec::new();

    let walker = ignore::WalkBuilder::new(base)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        if results.len() >= collect_limit {
            break;
        }

        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let ft = match entry.file_type() {
            Some(ft) => ft,
            None => continue,
        };
        if ft.is_dir() {
            continue;
        }

        let relative = path.strip_prefix(base).unwrap_or(path);
        let path_str = relative.to_string_lossy();
        let path_lower = path_str.to_ascii_lowercase();

        let filename = relative
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        let score = if filename.contains(&query_lower) {
            2
        } else if path_lower.contains(&query_lower) {
            1
        } else {
            continue;
        };

        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);

        results.push(Scored { path: path_str.to_string(), mtime, score });
    }

    // Sort by score (higher first), then mtime (newer first), then path
    results.sort_by(|a, b| b.score.cmp(&a.score).then(b.mtime.cmp(&a.mtime)).then(a.path.cmp(&b.path)));
    results.truncate(max_files);

    Ok(results.into_iter().map(|s| (s.path, s.mtime)).collect())
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
    fn test_glob_rs_files() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "main.rs", "fn main() {}");
        create_test_file(&dir, "lib.rs", "pub fn foo() {}");
        create_test_file(&dir, "README.md", "# Project");

        let base = dir.path().to_path_buf();
        let results = glob_search_blocking(&base, "*.rs", DEFAULT_MAX_FILES).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|(p, _)| p == "main.rs"));
        assert!(results.iter().any(|(p, _)| p == "lib.rs"));
    }

    #[test]
    fn test_glob_single_file() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "target.txt", "content");
        create_test_file(&dir, "other.txt", "other");

        let base = dir.path().to_path_buf();
        let results = glob_search_blocking(&base, "target.txt", DEFAULT_MAX_FILES).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "target.txt");
    }

    #[test]
    fn test_glob_no_match() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "main.rs", "fn main() {}");

        let base = dir.path().to_path_buf();
        let results = glob_search_blocking(&base, "*.py", DEFAULT_MAX_FILES).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuzzy_filename() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "hello_world.rs", "fn main() {}");
        create_test_file(&dir, "goodbye.rs", "fn bye() {}");
        create_test_file(&dir, "sub/hello_there.rs", "fn hi() {}");

        let base = dir.path().to_path_buf();
        let results = fuzzy_search_blocking(&base, "hello", DEFAULT_MAX_FILES).unwrap();
        // Both hello_world.rs and sub/hello_there.rs should match (filename contains "hello")
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|(p, _)| p == "hello_world.rs"));
        assert!(results.iter().any(|(p, _)| p == "sub/hello_there.rs"));
    }

    #[test]
    fn test_fuzzy_case_insensitive() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "HelloWorld.rs", "fn main() {}");

        let base = dir.path().to_path_buf();
        let results = fuzzy_search_blocking(&base, "helloworld", DEFAULT_MAX_FILES).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "HelloWorld.rs");
    }

    #[test]
    fn test_fuzzy_substring_path() {
        let dir = TempDir::new().unwrap();
        create_test_file(&dir, "src/lib/core.rs", "pub fn core() {}");
        create_test_file(&dir, "other.rs", "other");

        let base = dir.path().to_path_buf();
        // Search for "core" - should match both filename and full-path
        let results = fuzzy_search_blocking(&base, "core", DEFAULT_MAX_FILES).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|(p, _)| p.contains("core")));
    }

    #[test]
    fn test_fuzzy_limit() {
        let dir = TempDir::new().unwrap();
        for i in 0..20 {
            create_test_file(&dir, &format!("file_{}.rs", i), "fn f() {}");
        }

        let base = dir.path().to_path_buf();
        let results = fuzzy_search_blocking(&base, "file", 5).unwrap();
        assert!(results.len() <= 5);
    }

    #[test]
    fn test_glob_invalid_pattern() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().to_path_buf();
        let result = glob_search_blocking(&base, "[invalid", DEFAULT_MAX_FILES);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid glob pattern") || err_msg.contains("\\[invalid"));
    }
}
