use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

use ffs_symbol::lang::detect_file_type;
use ffs_symbol::outline::get_outline_entries;
use ffs_symbol::types::{FileType, OutlineEntry, OutlineKind};

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
        "Show the structural outline of a file — functions, structs, classes — detected by tree-sitter AST parsing."
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
                || kind_lower.contains("test")
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
                || kind_lower.contains("module")
                || kind_lower.contains("const")
                || kind_lower.contains("property")
                || kind_lower.contains("export")
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

/// Extract outline entries using tree-sitter AST parsing via ffs-symbol.
///
/// Delegates language detection to `detect_file_type` and structural parsing
/// to `get_outline_entries`. Non-code file types return an empty outline.
fn outline_blocking(content: &str, file_path: &Path, max_items: usize) -> Vec<OutlineItem> {
    let file_type = detect_file_type(file_path);
    let lang = match file_type {
        FileType::Code(lang) => lang,
        _ => return Vec::new(),
    };

    let entries = get_outline_entries(content, lang);

    let mut items: Vec<OutlineItem> = Vec::new();
    for entry in &entries {
        if items.len() >= max_items {
            break;
        }
        items.push(outline_entry_to_item(entry));
        // Flatten children from class-like bodies (methods, properties, etc.)
        for child in &entry.children {
            if items.len() >= max_items {
                break;
            }
            items.push(outline_entry_to_item(child));
        }
    }

    items.truncate(max_items);
    items
}

/// Convert an OutlineKind to a short display string, consistent with the
/// original regex-based kind labels.
fn outline_kind_str(kind: OutlineKind) -> &'static str {
    match kind {
        OutlineKind::Function => "fn",
        OutlineKind::Struct => "struct",
        OutlineKind::Class => "class",
        OutlineKind::Interface => "interface",
        OutlineKind::Enum => "enum",
        OutlineKind::TypeAlias => "type",
        OutlineKind::Constant => "const",
        OutlineKind::Variable => "variable",
        OutlineKind::Export => "export",
        OutlineKind::Property => "property",
        OutlineKind::Module => "module",
        OutlineKind::Import => "import",
    }
}

/// Convert an ffs-symbol OutlineEntry into the internal OutlineItem.
fn outline_entry_to_item(entry: &OutlineEntry) -> OutlineItem {
    OutlineItem {
        kind: outline_kind_str(entry.kind).to_string(),
        label: entry.name.clone(),
        start_line: entry.start_line as usize,
        end_line: entry.end_line as usize,
        line_count: (entry.end_line - entry.start_line + 1) as usize,
    }
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
        assert!(
            kinds.contains(&"import"),
            "should detect import: {:?}",
            kinds
        );
        assert!(
            kinds.contains(&"struct"),
            "should detect struct: {:?}",
            kinds
        );
        assert!(
            kinds.contains(&"fn"),
            "should detect fn: {:?}",
            kinds
        );
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

export class MyComponent {
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
            kinds.contains(&"fn"),
            "should detect fn: {:?}",
            kinds
        );
        // export class MyComponent is parsed as export_statement by tree-sitter,
        // so the kind is "export" not "class". The class body's methods appear
        // as child entries within the export node.
        assert!(
            kinds.contains(&"export"),
            "should detect export (wrapping class): {:?}",
            kinds
        );
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
    fn test_outline_unknown_extension_returns_empty() {
        let content = "some random content";
        let path = Path::new("test.unknown");
        let items = outline_blocking(content, path, 20);
        assert!(items.is_empty(), "unknown extension should produce no items");
    }

    #[test]
    fn test_execute_finds_rust_structure() {
        let tool = FfsOutlineTool::new();
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
            output.output.contains("import")
                || output.output.contains("struct")
                || output.output.contains("fn"),
            "output should contain kind labels: {}",
            output.output
        );
    }
}
