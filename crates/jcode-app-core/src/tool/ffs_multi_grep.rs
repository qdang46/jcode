use super::{Tool, ToolContext, ToolOutput};
use aho_corasick::AhoCorasick;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_RESULTS: usize = 100;
const MAX_LINE_LEN: usize = 2000;

pub struct FfsMultiGrepTool;

impl FfsMultiGrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct FfsMultiGrepInput {
    patterns: Vec<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default = "default_mode")]
    mode: String,
}

fn default_mode() -> String {
    "any".to_string()
}

#[derive(Clone)]
struct MultiGrepResult {
    file: String,
    line_num: usize,
    line: String,
}

#[async_trait]
impl Tool for FfsMultiGrepTool {
    fn name(&self) -> &str {
        "multi_grep"
    }

    fn description(&self) -> &str {
        "Search for multiple patterns simultaneously. Uses Aho-Corasick for SIMD-accelerated multi-pattern matching."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["patterns"],
            "properties": {
                "intent": super::intent_schema_property(),
                "patterns": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Patterns to search for (OR logic)."
                },
                "path": {
                    "type": "string",
                    "description": "Search directory (defaults to current directory)."
                },
                "mode": {
                    "type": "string",
                    "enum": ["any", "all"],
                    "description": "Match mode: 'any' (default) matches lines containing ANY pattern, 'all' matches lines containing ALL patterns."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: FfsMultiGrepInput = serde_json::from_value(input)?;

        if params.patterns.is_empty() {
            return Err(anyhow::anyhow!("At least one pattern is required"));
        }

        let patterns = params.patterns.clone();
        let patterns_for_display = params.patterns.clone();
        let base_path_str = params.path.clone().unwrap_or_else(|| ".".to_string());
        let base = ctx.resolve_path(Path::new(&base_path_str));
        let mode = params.mode.clone();
        let mode_for_display = params.mode.clone();

        if !base.exists() {
            return Err(anyhow::anyhow!("Directory not found: {}", base_path_str));
        }

        let results = tokio::task::spawn_blocking(move || {
            multi_grep_blocking(&base, &patterns, &mode)
        })
        .await??;

        let mut output = String::new();
        output.push_str(&format!(
            "Found {} matches for {} '{}'\n\n",
            results.len(),
            if mode_for_display == "all" { "all of" } else { "any of" },
            patterns_for_display.join("', '")
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

fn multi_grep_blocking(
    base: &Path,
    patterns: &[String],
    mode: &str,
) -> Result<Vec<MultiGrepResult>> {
    let ac = AhoCorasick::builder()
        .build(patterns.iter().map(|s| s.as_str()))
        .map_err(|e| anyhow::anyhow!("Failed to build Aho-Corasick automaton: {}", e))?;

    let mode_all = mode == "all";

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
        let ac = ac.clone();
        let patterns = patterns.to_vec();
        let mode_all = mode_all;
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

            // Use entry.file_type() (cached from readdir, no extra stat)
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
                    if hit_count.load(Ordering::Relaxed) + local_results.len() >= MAX_RESULTS {
                        break;
                    }

                    let matches = ac.find_iter(line).count();

                    let matched = if mode_all {
                        // "all" mode: line must contain ALL patterns
                        // Each pattern match increments the count; we need at least
                        // the number of patterns to confirm all matched
                        find_all_match(line, &patterns)
                    } else {
                        matches > 0
                    };

                    if matched {
                        let relative = path
                            .strip_prefix(&base)
                            .unwrap_or(path)
                            .display()
                            .to_string();

                        let truncated = if line.len() > MAX_LINE_LEN {
                            format!("{}...", crate::util::truncate_str(line, MAX_LINE_LEN))
                        } else {
                            line.to_string()
                        };

                        local_results.push(MultiGrepResult {
                            file: relative,
                            line_num: line_num + 1,
                            line: truncated,
                        });
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

    // Sort by file then line number for deterministic output
    final_results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line_num.cmp(&b.line_num)));
    final_results.truncate(MAX_RESULTS);

    Ok(final_results)
}

/// For "all" mode, check that every pattern matches somewhere in the line.
/// Uses simple substring search (patterns are already lowered to strings).
fn find_all_match(line: &str, patterns: &[String]) -> bool {
    patterns.iter().all(|p| line.contains(p.as_str()))
}

fn is_binary_extension(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy().to_lowercase();
        let binary_exts = [
            "png", "jpg", "jpeg", "gif", "bmp", "ico", "webp", "pdf", "zip", "tar", "gz", "bz2",
            "xz", "7z", "rar", "exe", "dll", "so", "dylib", "o", "a", "class", "pyc", "wasm",
            "mp3", "mp4", "avi", "mov", "mkv", "flac", "ogg", "wav", "ttf", "woff", "woff2",
        ];
        return binary_exts.contains(&ext.as_str());
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolExecutionMode;
    use std::io::Write;

    #[test]
    fn test_multi_grep_any_mode() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(
            file,
            "fn foo() {{\n}}\n\nfn bar() {{\n}}\n"
        )
        .unwrap();

        let results = multi_grep_blocking(temp_dir.path(), &["foo".to_string(), "bar".to_string()], "any").unwrap();
        assert_eq!(results.len(), 2, "should match both foo and bar lines");
    }

    #[test]
    fn test_multi_grep_all_mode() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(
            file,
            "// combination line\nfn combination() {{\n    let foo = 1;\n    let bar = 2;\n}}\n\nfn both() {{\n    // foo and bar\n    println!(\"foo bar\");\n}}\n"
        )
        .unwrap();

        let results = multi_grep_blocking(temp_dir.path(), &["foo".to_string(), "bar".to_string()], "all").unwrap();
        // The last comment line contains both "foo" and "bar", and the "// foo and bar" line does too
        assert!(
            results.iter().any(|r| r.line.contains("foo and bar") || r.line.contains("foo bar")),
            "should find line with both patterns"
        );
    }

    #[test]
    fn test_multi_grep_no_results() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "fn existing() {{}}\n").unwrap();

        let results =
            multi_grep_blocking(temp_dir.path(), &["xyzzy".to_string()], "any").unwrap();
        assert!(results.is_empty(), "should find nothing for non-matching pattern");
    }

    #[test]
    fn test_multi_grep_empty_patterns_still_works() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "fn test() {{}}\n").unwrap();

        let results =
            multi_grep_blocking(temp_dir.path(), &["".to_string()], "any").unwrap();
        assert!(!results.is_empty(), "empty pattern should match every line");
    }

    #[test]
    fn test_multi_grep_binary_extension_skipped() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("image.png");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "this is not really a png file but has the extension").unwrap();

        let results = multi_grep_blocking(temp_dir.path(), &["this".to_string()], "any").unwrap();
        assert!(results.is_empty(), "should skip binary extensions");
    }

    #[test]
    fn test_execute_via_tool_interface() {
        let tool = FfsMultiGrepTool::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("app.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "pub fn main() {{\n    let msg = \"hello\";\n    println!(\"{{}}\", msg);\n}}\n").unwrap();

        let ctx = ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            tool_call_id: "test".to_string(),
            working_dir: Some(temp_dir.path().to_path_buf()),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::Direct,
        };

        let input = json!({
            "patterns": ["main", "hello"],
        });

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.execute(input, ctx));
        assert!(result.is_ok(), "should succeed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.output.contains("Found"), "should have found results");
    }
}
