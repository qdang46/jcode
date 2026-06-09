use super::{Tool, ToolContext, ToolOutput};
use super::binary_ext::is_binary_extension;
use anyhow::Result;
use async_trait::async_trait;
use ffs_symbol::symbol_index::{SymbolIndex, SymbolLocation};
use serde::Deserialize;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_RESULTS: usize = 30;

/// Lazy per-process symbol index, keyed by workspace root.
///
/// On first `execute` call the whole workspace is scanned and indexed via
/// tree-sitter. Subsequent lookups (even from different tool invocations) reuse
/// this cached index. The index is rebuilt automatically if the workspace root
/// changes (e.g. the user navigated to a different project mid-session).
static SYMBOL_INDEX: OnceLock<Mutex<Option<CachedIndex>>> = OnceLock::new();

struct CachedIndex {
    /// Workspace root that was scanned.
    root: PathBuf,
    /// The fully-built symbol index.
    index: Arc<SymbolIndex>,
}

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

/// A matched symbol with the line text populated for display.
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

        // Determine the workspace root for indexing.
        // Walk up from the working directory (or the explicitly-specified path)
        // looking for a .git sentinel.
        let start_dir = ctx.working_dir.as_deref().unwrap_or(&base);
        let workspace_root = find_workspace_root(start_dir);

        // The display root is the more specific of the two: if the user
        // provided an explicit `path` we show relative paths under that;
        // otherwise we show paths under the workspace root.
        let display_root = if params.path.is_some() {
            base.clone()
        } else {
            workspace_root.clone()
        };

        let results = tokio::task::spawn_blocking(move || -> Result<Vec<SymbolResult>> {
            let lock = SYMBOL_INDEX.get_or_init(|| Mutex::new(None));
            let mut guard = lock.lock().unwrap();

            // Initialise the index if this is the first call, or rebuild if
            // the workspace root has changed (e.g. different project).
            let index: &SymbolIndex = match &*guard {
                Some(cached) if cached.root == workspace_root => &cached.index,
                _ => {
                    let idx = build_symbol_index(&workspace_root)?;
                    let idx = Arc::new(idx);
                    *guard = Some(CachedIndex {
                        root: workspace_root.clone(),
                        index: idx.clone(),
                    });
                    // Safety: we just set guard, so this *must* be Some.
                    // We hold the lock so no other thread can have changed it.
                    &guard.as_ref().unwrap().index
                }
            };

            // Look up all locations for this symbol.
            let locations = index.lookup_exact(&symbol_name);

            // Filter by path prefix when the user specified a scope.
            let filtered: Vec<&SymbolLocation> = if params.path.is_some() {
                let base_str = base.to_string_lossy();
                locations
                    .iter()
                    .filter(|loc| loc.path.to_string_lossy().starts_with(base_str.as_ref()))
                    .collect()
            } else {
                locations.iter().collect()
            };

            // Sort: file then line number (same order as old impl).
            let mut sorted = filtered;
            sorted.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));

            // Cap at MAX_RESULTS.
            sorted.truncate(MAX_RESULTS);

            // Read line text for each result and build the output structs.
            let mut results: Vec<SymbolResult> = Vec::with_capacity(sorted.len());
            for loc in &sorted {
                let relative = loc
                    .path
                    .strip_prefix(&display_root)
                    .unwrap_or(&loc.path)
                    .display()
                    .to_string();

                let line_text = read_line_text(&loc.path, loc.line as usize);

                results.push(SymbolResult {
                    file: relative,
                    line_num: loc.line as usize,
                    kind: short_kind(&loc.kind).to_string(),
                    name: symbol_name.clone(),
                    line: line_text.unwrap_or_default(),
                });
            }

            Ok(results)
        })
        .await??;

        // -------- Format output (identical to original) --------
        let mut output = String::new();
        if results.is_empty() {
            output.push_str(&format!("No symbols found matching '{}'\n", params.name));
            return Ok(ToolOutput::new(output));
        }

        output.push_str(&format!(
            "Found {} symbol(s) matching '{}'\n\n",
            results.len(),
            params.name
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

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Walk up from `start` looking for a `.git` sentinel directory. Returns the
/// first ancestor (or `start` itself) that contains `.git`. If none is found,
/// `start` itself is used as the workspace root.
fn find_workspace_root(start: &Path) -> PathBuf {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join(".git").is_dir() {
            return dir.to_path_buf();
        }
        current = dir.parent();
    }
    start.to_path_buf()
}

/// Walk every (non-binary, non-directory) file under `root` and build a
/// `SymbolIndex` via tree-sitter parsing.
///
/// The parallel walker dispatches file-level `index_file` calls across rayon
/// worker threads. Only files recognised as code by `ffs_symbol::detect_file_type`
/// are indexed; everything else is skipped efficiently by `index_file`.
fn build_symbol_index(root: &Path) -> Result<SymbolIndex> {
    let index = Arc::new(SymbolIndex::new());

    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);

    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .threads(threads)
        .build_parallel();

    walker.run(|| {
        let index = index.clone();
        Box::new(move |entry| {
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
            if ft.is_dir() || ft.is_symlink() {
                return ignore::WalkState::Continue;
            }

            // Skip known binary extensions so we don't waste time reading
            // images, archives, etc.  (index_file also handles this via
            // detect_file_type, but catching the extension early avoids the
            // read + tree-sitter overhead for non-UTF-8 binaries.)
            if is_binary_extension(path) {
                return ignore::WalkState::Continue;
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => return ignore::WalkState::Continue,
            };
            let mtime = match std::fs::metadata(path).and_then(|m| m.modified()) {
                Ok(m) => m,
                Err(_) => return ignore::WalkState::Continue,
            };

            // index_file is a no-op for non-code files (returns 0).
            index.index_file(path, mtime, &content);

            ignore::WalkState::Continue
        })
    });

    // After `walker.run()` returns, all thread-local Arc clones have been
    // dropped. The only remaining strong count is the one we hold here,
    // so try_unwrap succeeds.
    Arc::try_unwrap(index).map_err(|_| anyhow::anyhow!("SymbolIndex still referenced after scan"))
}

/// Read the line at `line_num` (1-based) from `path`. Returns `None` when the
/// file cannot be read or the line does not exist.
fn read_line_text(path: &Path, line_num: usize) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().nth(line_num - 1).map(|l| l.trim().to_string())
}

/// Map the tree-sitter node `kind` string to the short display label used by
/// the current regex-based impl.
///
/// The list covers every entry in `ffs_symbol::treesitter::DEFINITION_KINDS`
/// that can be produced by the bundled grammars.
fn short_kind(kind: &str) -> Cow<'static, str> {
    match kind {
        // Primary function/method declarations
        "function_item"
        | "function_declaration"
        | "function_definition"
        | "function_expression"
        | "generator_function"
        | "method_definition"
        | "method_declaration"
        | "arrow_function" => Cow::Borrowed("fn"),

        // Classes
        "class_declaration" | "class_definition" => Cow::Borrowed("class"),

        // Rust / C-like structures
        "struct_item" | "object_declaration" => Cow::Borrowed("struct"),

        // Interfaces and traits
        "interface_declaration" | "trait_declaration" | "trait_item" => Cow::Borrowed("trait"),

        // Type aliases
        "type_alias_declaration" | "type_item" | "type_declaration" => Cow::Borrowed("type"),

        // Enumerations
        "enum_item" | "enum_declaration" => Cow::Borrowed("enum"),

        // Constants and statics
        "const_item" | "const_declaration" => Cow::Borrowed("const"),
        "static_item" => Cow::Borrowed("static"),

        // Variable declarations (JS/Python let/var)
        "lexical_declaration" | "variable_declaration" => Cow::Borrowed("var"),

        // Properties
        "property_declaration" => Cow::Borrowed("property"),

        // Rust impl blocks
        "impl_item" => Cow::Borrowed("impl"),

        // Modules / namespaces
        "mod_item" | "namespace_definition" => Cow::Borrowed("mod"),

        // Decorated / annotated definitions (Python decorators etc.)
        "decorated_definition" => Cow::Borrowed("decorated"),

        // Export statements (JS/TS `export { ... }`)
        "export_statement" => Cow::Borrowed("export"),

        // Elixir definitions
        "elixir_def" => Cow::Borrowed("def"),

        // Fallback — show the raw node kind so the caller can make sense of it
        _ => Cow::Owned(kind.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolExecutionMode;
    use std::io::Write;

    // --- Helper: build a fresh index for a single test, bypassing the static

    fn test_index(root: &Path) -> SymbolIndex {
        build_symbol_index(root).expect("build_symbol_index should succeed")
    }

    // --- Unit tests for build_symbol_index + lookup_exact ---

    #[test]
    fn finds_rust_fn() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("lib.rs");
        let mut file = std::fs::File::create(&f).unwrap();
        write!(file, "pub fn hello_world() {{\n    println!(\"hello\");\n}}\n\nfn helper() {{}}\n")
            .unwrap();

        let idx = test_index(tmp.path());
        let results = idx.lookup_exact("hello_world");
        assert!(!results.is_empty(), "should find hello_world");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn finds_rust_struct() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("lib.rs");
        let mut file = std::fs::File::create(&f).unwrap();
        write!(file, "pub struct MyConfig {{\n    value: i32,\n}}\n").unwrap();

        let idx = test_index(tmp.path());
        let results = idx.lookup_exact("MyConfig");
        assert!(!results.is_empty(), "should find MyConfig");
    }

    #[test]
    fn no_results_for_nonexistent() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("lib.rs");
        let mut file = std::fs::File::create(&f).unwrap();
        writeln!(file, "pub fn existing() {{}}").unwrap();

        let idx = test_index(tmp.path());
        let results = idx.lookup_exact("NonExistentSymbol");
        assert!(results.is_empty(), "should find nothing");
    }

    #[test]
    fn finds_symbol_in_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("src");
        std::fs::create_dir(&sub).unwrap();
        let f = sub.join("main.rs");
        let mut file = std::fs::File::create(&f).unwrap();
        write!(file, "fn run() {{\n    todo!()\n}}\n").unwrap();

        let idx = test_index(tmp.path());
        let results = idx.lookup_exact("run");
        assert!(!results.is_empty(), "should find run function in subdir");
    }

    // --- short_kind mapping ---

    #[test]
    fn short_kind_common_mappings() {
        assert_eq!(short_kind("function_item"), Cow::Borrowed("fn"));
        assert_eq!(short_kind("function_declaration"), Cow::Borrowed("fn"));
        assert_eq!(short_kind("struct_item"), Cow::Borrowed("struct"));
        assert_eq!(short_kind("enum_item"), Cow::Borrowed("enum"));
        assert_eq!(short_kind("trait_item"), Cow::Borrowed("trait"));
        assert_eq!(short_kind("impl_item"), Cow::Borrowed("impl"));
        assert_eq!(short_kind("type_item"), Cow::Borrowed("type"));
        assert_eq!(short_kind("const_item"), Cow::Borrowed("const"));
        assert_eq!(short_kind("static_item"), Cow::Borrowed("static"));
        assert_eq!(short_kind("class_declaration"), Cow::Borrowed("class"));
        assert_eq!(short_kind("interface_declaration"), Cow::Borrowed("trait"));
        assert_eq!(short_kind("mod_item"), Cow::Borrowed("mod"));
        assert_eq!(
            short_kind("unknown_kind"),
            Cow::Owned::<str>("unknown_kind".to_string())
        );
    }

    // --- read_line_text ---

    #[test]
    fn read_line_text_returns_correct_line() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("test.rs");
        std::fs::write(
            &f,
            "line one\n  line two\nline three\n",
        )
        .unwrap();

        assert_eq!(read_line_text(&f, 1).as_deref(), Some("line one"));
        assert_eq!(read_line_text(&f, 2).as_deref(), Some("line two"));
        assert_eq!(read_line_text(&f, 3).as_deref(), Some("line three"));
        assert_eq!(read_line_text(&f, 99), None);
    }

    // --- find_workspace_root ---

    #[test]
    fn workspace_root_with_git_sentinel() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        let root = find_workspace_root(&tmp.path().join("a/b"));
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn workspace_root_without_git_falls_back_to_start() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        let root = find_workspace_root(&tmp.path().join("a/b"));
        assert_eq!(root, tmp.path().join("a/b"));
    }

    // --- Integration: execute through the Tool interface ---
    //
    // NOTE: Because `SYMBOL_INDEX` is a process-global static, tests that call
    // `execute` share the cached index. Running them in parallel with other
    // execute-based tests could be flaky. This test is kept as a regression
    // check for the `execute` code path; prefer testing via the helper
    // functions above.

    #[test]
    fn execute_via_tool_interface() {
        let tool = FfsSymbolTool::new();
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("mod.rs");
        let mut file = std::fs::File::create(&f).unwrap();
        writeln!(file, "pub struct TestSymbol;").unwrap();

        let ctx = ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            tool_call_id: "test".to_string(),
            working_dir: Some(tmp.path().to_path_buf()),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::Direct,
            best_of_n_run_id: None,
            best_of_n_candidate_id: None,
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
