use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

const MAX_ITEMS: usize = 50;
const MAX_LINE_LEN: usize = 2000;

pub struct FfsOutlineTool;

impl FfsOutlineTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct FfsOutlineInput {
    file: String,
    #[serde(default = "default_max_items")]
    max_items: usize,
}

fn default_max_items() -> usize {
    20
}

#[derive(Debug, Clone)]
struct OutlineItem {
    kind: String,
    label: String,
    start_line: usize,
    end_line: usize,
    line_count: usize,
}

#[async_trait]
impl Tool for FfsOutlineTool {
    fn name(&self) -> &str {
        "outline"
    }

    fn description(&self) -> &str {
        "Show the structural outline of a file — functions, structs, classes — detected by regex patterns."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["file"],
            "properties": {
                "intent": super::intent_schema_property(),
                "file": {
                    "type": "string",
                    "description": "File path to outline."
                },
                "max_items": {
                    "type": "integer",
                    "description": "Max items to return. Default 20."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: FfsOutlineInput = serde_json::from_value(input)?;

        let file_path = ctx.resolve_path(Path::new(&params.file));

        if !file_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", params.file));
        }
        if !file_path.is_file() {
            return Err(anyhow::anyhow!("Not a file: {}", params.file));
        }

        let max_items = params.max_items.max(1).min(MAX_ITEMS);

        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| anyhow::anyhow!("Failed to read file: {}", e))?;

        let items = outline_blocking(&content, &file_path, max_items);

        let mut output = String::new();
        output.push_str(&format!("Outline for: {}\n\n", file_path.display()));

        if items.is_empty() {
            output.push_str("No structural items found.\n");
            return Ok(ToolOutput::new(output));
        }

        // Group by kind
        let mut functions: Vec<&OutlineItem> = Vec::new();
        let mut types: Vec<&OutlineItem> = Vec::new();
        let mut imports: Vec<&OutlineItem> = Vec::new();
        let mut other: Vec<&OutlineItem> = Vec::new();

        for item in &items {
            let kind_lower = item.kind.to_lowercase();
            if kind_lower.contains("fn")
                || kind_lower.contains("function")
                || kind_lower.contains("method")
                || kind_lower.contains("def")
            {
                functions.push(item);
            } else if kind_lower.contains("struct")
                || kind_lower.contains("enum")
                || kind_lower.contains("trait")
                || kind_lower.contains("impl")
                || kind_lower.contains("class")
                || kind_lower.contains("interface")
                || kind_lower.contains("type")
                || kind_lower.contains("macro")
                || kind_lower.contains("signature")
            {
                types.push(item);
            } else if kind_lower.contains("import")
                || kind_lower.contains("use")
                || kind_lower.contains("require")
            {
                imports.push(item);
            } else {
                other.push(item);
            }
        }

        if !functions.is_empty() {
            output.push_str("=== Functions ===\n");
            for item in &functions {
                output.push_str(&format!(
                    "  {:>4}  {:<8} {} ({} lines)\n",
                    item.start_line, item.kind, item.label, item.line_count
                ));
            }
            output.push('\n');
        }

        if !types.is_empty() {
            output.push_str("=== Types / Macros / Signatures ===\n");
            for item in &types {
                output.push_str(&format!(
                    "  {:>4}  {:<8} {} ({} lines)\n",
                    item.start_line, item.kind, item.label, item.line_count
                ));
            }
            output.push('\n');
        }

        if !imports.is_empty() {
            output.push_str("=== Imports ===\n");
            for item in &imports {
                output.push_str(&format!(
                    "  {:>4}  {:<8} {} ({} lines)\n",
                    item.start_line, item.kind, item.label, item.line_count
                ));
            }
            output.push('\n');
        }

        if !other.is_empty() {
            output.push_str("=== Other ===\n");
            for item in &other {
                output.push_str(&format!(
                    "  {:>4}  {:<8} {} ({} lines)\n",
                    item.start_line, item.kind, item.label, item.line_count
                ));
            }
            output.push('\n');
        }

        if items.len() >= max_items {
            output.push_str(&format!("... results truncated at {} items\n", max_items));
        }

        Ok(ToolOutput::new(output))
    }
}

fn outline_blocking(content: &str, file_path: &Path, max_items: usize) -> Vec<OutlineItem> {
    let extension = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let patterns: Vec<(Regex, &str)> = match extension.as_str() {
        "rs" => vec![
            (
                Regex::new(r"^\s*pub\s+(unsafe\s+)?fn\s+(\w+)").unwrap(),
                "fn",
            ),
            (Regex::new(r"^\s*(unsafe\s+)?fn\s+(\w+)").unwrap(), "fn"),
            (
                Regex::new(r"^\s*pub\s+(unsafe\s+)?trait\s+(\w+)").unwrap(),
                "trait",
            ),
            (
                Regex::new(r"^\s*(unsafe\s+)?trait\s+(\w+)").unwrap(),
                "trait",
            ),
            (
                Regex::new(r"^\s*pub\s+(unsafe\s+)?impl\s+").unwrap(),
                "impl",
            ),
            (Regex::new(r"^\s*(unsafe\s+)?impl\s+").unwrap(), "impl"),
            (Regex::new(r"^\s*pub\s+struct\s+(\w+)").unwrap(), "struct"),
            (Regex::new(r"^\s*struct\s+(\w+)").unwrap(), "struct"),
            (Regex::new(r"^\s*pub\s+enum\s+(\w+)").unwrap(), "enum"),
            (Regex::new(r"^\s*enum\s+(\w+)").unwrap(), "enum"),
            (Regex::new(r"^\s*pub\s+(type|mod)\s+(\w+)").unwrap(), "type"),
            (Regex::new(r"^\s*(type|mod)\s+(\w+)").unwrap(), "type"),
            (Regex::new(r"^\s*#\[derive\(").unwrap(), "derive"),
            (
                Regex::new(r"^\s*(pub\s+)?macro_rules!\s*!(\w+)").unwrap(),
                "macro",
            ),
            (Regex::new(r"^\s*pub\s+use\s+").unwrap(), "use"),
            (Regex::new(r"^\s*use\s+").unwrap(), "use"),
            (
                Regex::new(r"^\s*pub\s+(const|static)\s+(\w+)").unwrap(),
                "const",
            ),
            (Regex::new(r"^\s*(const|static)\s+(\w+)").unwrap(), "const"),
        ],
        "js" | "jsx" | "ts" | "tsx" => vec![
            (
                Regex::new(r"^\s*(export\s+)?(async\s+)?function\s+(\w+)").unwrap(),
                "function",
            ),
            (
                Regex::new(r"^\s*(export\s+)?(async\s+)?function\s*\*?\s*(\w+)").unwrap(),
                "function",
            ),
            (
                Regex::new(r"^\s*(export\s+)?class\s+(\w+)").unwrap(),
                "class",
            ),
            (
                Regex::new(r"^\s*(export\s+)?interface\s+(\w+)").unwrap(),
                "interface",
            ),
            (Regex::new(r"^\s*(export\s+)?type\s+(\w+)").unwrap(), "type"),
            (Regex::new(r"^\s*(export\s+)?enum\s+(\w+)").unwrap(), "enum"),
            (
                Regex::new(r"^\s*(export\s+)?(default\s+)?const\s+(\w+)\s*=").unwrap(),
                "const",
            ),
            (
                Regex::new(r"^\s*(export\s+)?let\s+(\w+)\s*=").unwrap(),
                "let",
            ),
            (Regex::new(r"^\s*import\s+").unwrap(), "import"),
            (Regex::new(r"^\s*require\s*\(").unwrap(), "require"),
            (
                Regex::new(r"^\s*(export\s+)?(abstract\s+)?class\s+(\w+)").unwrap(),
                "class",
            ),
        ],
        "py" => vec![
            (Regex::new(r"^\s*(async\s+)?def\s+(\w+)").unwrap(), "def"),
            (Regex::new(r"^\s*class\s+(\w+)").unwrap(), "class"),
            (Regex::new(r"^\s*import\s+").unwrap(), "import"),
            (Regex::new(r"^\s*from\s+\S+\s+import\s+").unwrap(), "from"),
            (Regex::new(r"^\s*@\w+").unwrap(), "decorator"),
        ],
        "go" => vec![
            (Regex::new(r"^\s*func\s+(\w+)").unwrap(), "func"),
            (
                Regex::new(r"^\s*func\s+\([^)]*\)\s+(\w+)").unwrap(),
                "method",
            ),
            (Regex::new(r"^\s*type\s+(\w+)\s+struct").unwrap(), "struct"),
            (
                Regex::new(r"^\s*type\s+(\w+)\s+interface").unwrap(),
                "interface",
            ),
            (Regex::new(r"^\s*import\s+").unwrap(), "import"),
        ],
        "java" => vec![
            (
                Regex::new(r"^\s*(public|private|protected)\s+(static\s+)?\w+\s+(\w+)\s*\(")
                    .unwrap(),
                "method",
            ),
            (Regex::new(r"^\s*class\s+(\w+)").unwrap(), "class"),
            (Regex::new(r"^\s*interface\s+(\w+)").unwrap(), "interface"),
            (Regex::new(r"^\s*enum\s+(\w+)").unwrap(), "enum"),
            (Regex::new(r"^\s*import\s+").unwrap(), "import"),
        ],
        "c" | "h" | "cpp" | "hpp" | "cc" | "cxx" => vec![
            (Regex::new(r"^\s*\w+\s+(\w+)\s*\(").unwrap(), "function"),
            (
                Regex::new(r"^\s*(class|struct|enum|union)\s+(\w+)").unwrap(),
                "type",
            ),
            (Regex::new(r"^\s*template\s*<").unwrap(), "template"),
            (Regex::new(r"^\s*#include").unwrap(), "include"),
            (Regex::new(r"^\s*#define").unwrap(), "define"),
        ],
        _ => vec![
            // Generic patterns for any language
            (Regex::new(r"^\s*fn\s+(\w+)").unwrap(), "fn"),
            (Regex::new(r"^\s*function\s+(\w+)").unwrap(), "function"),
            (Regex::new(r"^\s*class\s+(\w+)").unwrap(), "class"),
            (Regex::new(r"^\s*struct\s+(\w+)").unwrap(), "struct"),
            (Regex::new(r"^\s*enum\s+(\w+)").unwrap(), "enum"),
            (Regex::new(r"^\s*interface\s+(\w+)").unwrap(), "interface"),
            (Regex::new(r"^\s*(pub\s+)?impl\s+").unwrap(), "impl"),
            (Regex::new(r"^\s*(pub\s+)?trait\s+(\w+)").unwrap(), "trait"),
            (Regex::new(r"^\s*(pub\s+)?def\s+(\w+)").unwrap(), "def"),
            (Regex::new(r"^\s*import\s+").unwrap(), "import"),
            (Regex::new(r"^\s*use\s+").unwrap(), "use"),
            (
                Regex::new(r"^\s*(pub\s+)?(const|static)\s+(\w+)").unwrap(),
                "const",
            ),
            (Regex::new(r"^\s*#\s*include").unwrap(), "include"),
        ],
    };

    let mut items: Vec<OutlineItem> = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    for (line_num, line) in lines.iter().enumerate() {
        if items.len() >= max_items {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
        {
            continue;
        }

        for (re, kind) in &patterns {
            if let Some(caps) = re.captures(line) {
                let label = if caps.len() > 2 {
                    // Try the last capture group (position 2 or 3 depending on pattern)
                    caps.get(caps.len() - 1)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_else(|| trimmed.chars().take(60).collect())
                } else if caps.len() > 1 {
                    caps.get(1)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_else(|| trimmed.chars().take(60).collect())
                } else {
                    // For patterns with no capture groups (like `use`, `import`, `impl`)
                    trimmed.chars().take(60).collect()
                };

                items.push(OutlineItem {
                    kind: kind.to_string(),
                    label,
                    start_line: line_num + 1,
                    end_line: line_num + 1,
                    line_count: estimate_line_count(&lines, line_num, total_lines),
                });
                break;
            }
        }
    }

    items.truncate(max_items);
    items
}

/// Estimate how many lines a structural element spans by scanning for
/// a closing brace at the original indentation level, or falling back
/// to the next blank line or end of file.
fn estimate_line_count(lines: &[&str], start: usize, total_lines: usize) -> usize {
    if start + 1 >= total_lines {
        return 1;
    }

    let _indent = lines[start]
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();

    // Scan forward to find closing brace at same indent level
    let mut brace_depth = 0i32;
    let mut found_opening = false;

    for i in start..total_lines {
        let line = lines[i];
        for ch in line.chars() {
            match ch {
                '{' => {
                    brace_depth += 1;
                    found_opening = true;
                }
                '}' => {
                    brace_depth -= 1;
                    if found_opening && brace_depth == 0 {
                        // Closing brace at the outer scope level
                        return i - start + 1;
                    }
                }
                _ => {}
            }
        }
    }

    // Fallback: count until blank line or section break
    let mut count = 1;
    for i in (start + 1)..total_lines.min(start + 30) {
        let line = lines[i].trim();
        if line.is_empty() {
            break;
        }
        count += 1;
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolExecutionMode;
    use std::io::Write;

    #[test]
    fn test_outline_detects_rust_functions() {
        let content = r#"
use std::collections::HashMap;

pub struct MyStruct {
    field: i32,
}

impl MyStruct {
    pub fn new() -> Self {
        Self { field: 0 }
    }

    fn helper(&self) -> i32 {
        self.field
    }
}

pub fn main() -> Result<()> {
    let x = 1;
    Ok(())
}
"#;
        let path = Path::new("test.rs");
        let items = outline_blocking(content, path, 20);
        assert!(!items.is_empty(), "should find items");
        let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"use"), "should detect use: {:?}", kinds);
        assert!(
            kinds.contains(&"struct"),
            "should detect struct: {:?}",
            kinds
        );
        assert!(kinds.contains(&"impl"), "should detect impl: {:?}", kinds);
        assert!(kinds.contains(&"fn"), "should detect fn: {:?}", kinds);
    }

    #[test]
    fn test_outline_detects_typescript_functions() {
        let content = r#"
import { Component } from 'react';

interface Props {
    name: string;
}

function greet(name: string): string {
    return `Hello ${name}`;
}

export class MyComponent implements Props {
    render() {
        return null;
    }
}
"#;
        let path = Path::new("test.tsx");
        let items = outline_blocking(content, path, 20);
        assert!(!items.is_empty(), "should find items");
        let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
        assert!(
            kinds.contains(&"import"),
            "should detect import: {:?}",
            kinds
        );
        assert!(
            kinds.contains(&"interface"),
            "should detect interface: {:?}",
            kinds
        );
        assert!(
            kinds.contains(&"function"),
            "should detect function: {:?}",
            kinds
        );
        assert!(kinds.contains(&"class"), "should detect class: {:?}", kinds);
    }

    #[test]
    fn test_outline_empty_file() {
        let content = "";
        let path = Path::new("test.rs");
        let items = outline_blocking(content, path, 20);
        assert!(items.is_empty());
    }

    #[test]
    fn test_outline_respects_max_items() {
        let content = (0..100)
            .map(|i| format!("pub fn test_{}() {{}}\n", i))
            .collect::<String>();
        let path = Path::new("test.rs");
        let items = outline_blocking(&content, path, 5);
        assert_eq!(items.len(), 5, "should be limited to 5 items");
    }

    #[test]
    fn test_execute_finds_rust_structure() {
        let tool = FfsOutlineTool::new();
        // Create a temp file
        let mut tmpfile = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
        write!(
            tmpfile,
            "use std::fmt;\n\npub struct Point {{\n    x: i32,\n    y: i32,\n}}\n\nimpl Point {{\n    pub fn new(x: i32, y: i32) -> Self {{\n        Self {{ x, y }}\n    }}\n}}\n"
        )
        .unwrap();

        let ctx = ToolContext {
            session_id: "test".to_string(),
            message_id: "test".to_string(),
            tool_call_id: "test".to_string(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::Direct,
            best_of_n_run_id: None,
            best_of_n_candidate_id: None,
        };

        let input = json!({
            "file": tmpfile.path().to_string_lossy().to_string(),
            "max_items": 10
        });

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.execute(input, ctx));
        assert!(result.is_ok(), "should succeed: {:?}", result.err());
        let output = result.unwrap();
        assert!(
            output.output.contains("Point"),
            "should mention Point struct"
        );
        assert!(
            output.output.contains("use")
                || output.output.contains("struct")
                || output.output.contains("fn"),
            "output should contain kind labels: {}",
            output.output
        );
    }
}
