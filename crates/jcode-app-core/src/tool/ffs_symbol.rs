use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_RESULTS: usize = 30;

pub struct FfsSymbolTool;

impl FfsSymbolTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct FfsSymbolInput {
    name: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Clone)]
struct SymbolResult {
    file: String,
    line_num: usize,
    kind: String,
    name: String,
    line: String,
}

#[async_trait]
impl Tool for FfsSymbolTool {
    fn name(&self) -> &str {
        "symbol"
    }

    fn description(&self) -> &str {
        "Find symbol definitions (functions, structs, types, classes) across the workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": {
                    "type": "string",
                    "description": "Symbol name to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Search directory (defaults to current directory)."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: FfsSymbolInput = serde_json::from_value(input)?;

        let symbol_name = params.name.clone();
        let base_path_str = params.path.clone().unwrap_or_else(|| ".".to_string());
        let base = ctx.resolve_path(Path::new(&base_path_str));

        if !base.exists() {
            return Err(anyhow::anyhow!("Directory not found: {}", base_path_str));
        }

        let results = tokio::task::spawn_blocking(move || {
            symbol_search_blocking(&base, &symbol_name)
        })
        .await??;

        let mut output = String::new();
        if results.is_empty() {
            output.push_str(&format!(
                "No symbols found matching '{}'\n",
                params.name
            ));
            return Ok(ToolOutput::new(output));
        }

        output.push_str(&format!(
            "Found {} symbol(s) matching '{}'\n\n",
            results.len(),
            params.name
        ));

        // Group by file
        let mut current_file = String::new();
        for result in &results {
            if result.file != current_file {
                if !current_file.is_empty() {
                    output.push('\n');
                }
                output.push_str(&format!("{}:\n", result.file));
                current_file = result.file.clone();
            }
            output.push_str(&format!(
                "  {:>4}  {:<10} {} {}",
                result.line_num, result.kind, result.name, result.line
            ));
            if !result.line.ends_with('\n') {
                output.push('\n');
            }
        }

        if results.len() >= MAX_RESULTS {
            output.push_str(&format!(
                "\n... results truncated at {} matches\n",
                MAX_RESULTS
            ));
        }

        Ok(ToolOutput::new(output))
    }
}

fn symbol_search_blocking(base: &Path, symbol_name: &str) -> Result<Vec<SymbolResult>> {
    // Compile language-specific patterns that match Rust-like symbol definitions
    let patterns: Vec<(Regex, &str)> = vec![
        // Rust
        (Regex::new(&format!(r"^\s*pub\s+(unsafe\s+)?fn\s+{}[\s<(]", regex::escape(symbol_name)))?, "fn"),
        (Regex::new(&format!(r"^\s*(unsafe\s+)?fn\s+{}[\s<(]", regex::escape(symbol_name)))?, "fn"),
        (Regex::new(&format!(r"^\s*pub\s+struct\s+{}\b", regex::escape(symbol_name)))?, "struct"),
        (Regex::new(&format!(r"^\s*struct\s+{}\b", regex::escape(symbol_name)))?, "struct"),
        (Regex::new(&format!(r"^\s*pub\s+enum\s+{}\b", regex::escape(symbol_name)))?, "enum"),
        (Regex::new(&format!(r"^\s*enum\s+{}\b", regex::escape(symbol_name)))?, "enum"),
        (Regex::new(&format!(r"^\s*pub\s+trait\s+{}\b", regex::escape(symbol_name)))?, "trait"),
        (Regex::new(&format!(r"^\s*trait\s+{}\b", regex::escape(symbol_name)))?, "trait"),
        (Regex::new(&format!(r"^\s*(pub\s+)?(unsafe\s+)?impl\s+.*{}[\s<]", regex::escape(symbol_name)))?, "impl"),
        (Regex::new(&format!(r"^\s*pub\s+(type|mod)\s+{}\b", regex::escape(symbol_name)))?, "type"),
        (Regex::new(&format!(r"^\s*(type|mod)\s+{}\b", regex::escape(symbol_name)))?, "type"),
        (Regex::new(&format!(r"^\s*pub\s+(const|static)\s+{}\b", regex::escape(symbol_name)))?, "const"),
        (Regex::new(&format!(r"^\s*(const|static)\s+{}\b", regex::escape(symbol_name)))?, "const"),
        // TS/JS
        (Regex::new(&format!(r"^\s*(export\s+)?(async\s+)?function\s+{}\s*\(", regex::escape(symbol_name)))?, "function"),
        (Regex::new(&format!(r"^\s*(export\s+)?class\s+{}\b", regex::escape(symbol_name)))?, "class"),
        (Regex::new(&format!(r"^\s*(export\s+)?interface\s+{}\b", regex::escape(symbol_name)))?, "interface"),
        (Regex::new(&format!(r"^\s*(export\s+)?type\s+{}\b", regex::escape(symbol_name)))?, "type"),
        (Regex::new(&format!(r"^\s*(export\s+)?enum\s+{}\b", regex::escape(symbol_name)))?, "enum"),
        (Regex::new(&format!(r"^\s*(export\s+)?(default\s+)?const\s+{}\s*=", regex::escape(symbol_name)))?, "const"),
        // Python
        (Regex::new(&format!(r"^\s*(async\s+)?def\s+{}\s*\(", regex::escape(symbol_name)))?, "def"),
        (Regex::new(&format!(r"^\s*class\s+{}\b", regex::escape(symbol_name)))?, "class"),
        // Go
        (Regex::new(&format!(r"^\s*func\s+{}\s*\(", regex::escape(symbol_name)))?, "func"),
        (Regex::new(&format!(r"^\s*func\s+\([^)]*\)\s+{}\s*\(", regex::escape(symbol_name)))?, "method"),
        (Regex::new(&format!(r"^\s*type\s+{}\b", regex::escape(symbol_name)))?, "type"),
    ];

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
        let patterns = patterns.clone();
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

                    for (re, kind) in &patterns {
                        if let Some(caps) = re.captures(line) {
                            let relative = path
                                .strip_prefix(&base)
                                .unwrap_or(path)
                                .display()
                                .to_string();

                            local_results.push(SymbolResult {
                                file: relative,
                                line_num: line_num + 1,
                                kind: kind.to_string(),
                                name: symbol_name.to_string(),
                                line: line.trim().to_string(),
                            });
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

    // Sort by file then line number
    final_results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line_num.cmp(&b.line_num)));
    final_results.truncate(MAX_RESULTS);

    Ok(final_results)
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
    fn test_symbol_search_finds_rust_fn() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("lib.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(
            file,
            "pub fn hello_world() {{\n    println!(\"hello\");\n}}\n\nfn helper() {{}}\n"
        )
        .unwrap();

        let results = symbol_search_blocking(temp_dir.path(), "hello_world").unwrap();
        assert!(!results.is_empty(), "should find hello_world");
        assert!(results.iter().any(|r| r.name == "hello_world"));
    }

    #[test]
    fn test_symbol_search_finds_rust_struct() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("lib.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "pub struct MyConfig {{\n    value: i32,\n}}\n").unwrap();

        let results = symbol_search_blocking(temp_dir.path(), "MyConfig").unwrap();
        assert!(!results.is_empty(), "should find MyConfig");
    }

    #[test]
    fn test_symbol_search_no_results() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("lib.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "pub fn existing() {{}}\n").unwrap();

        let results = symbol_search_blocking(temp_dir.path(), "NonExistentSymbol").unwrap();
        assert!(results.is_empty(), "should find nothing");
    }

    #[test]
    fn test_symbol_search_finds_in_subdir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sub_dir = temp_dir.path().join("src");
        std::fs::create_dir(&sub_dir).unwrap();
        let file_path = sub_dir.join("main.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "fn run() {{\n    todo!()\n}}\n").unwrap();

        let results = symbol_search_blocking(temp_dir.path(), "run").unwrap();
        assert!(!results.is_empty(), "should find run function in subdir");
    }

    #[test]
    fn test_execute_via_tool_interface() {
        let tool = FfsSymbolTool::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("mod.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        write!(file, "pub struct TestSymbol;\n").unwrap();

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
            "name": "TestSymbol",
        });

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.execute(input, ctx));
        assert!(result.is_ok(), "should succeed: {:?}", result.err());
        let output = result.unwrap();
        assert!(
            output.output.contains("TestSymbol"),
            "output should mention symbol"
        );
    }
}
