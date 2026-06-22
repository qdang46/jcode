use super::{Tool, ToolContext, ToolOutput};
use crate::bus::{Bus, BusEvent, FileOp, FileTouch};
use crate::tool::hashline_snapshots;
use anyhow::Result;
use async_trait::async_trait;
use hashline::{anchor, document::FileContent, hash as hashline_hash};
use jcode_hooks::{
    DispatchConfig, HookContext, HookEvent, HookInputBuilder, HookRegistry, load_hooks_config,
};
use serde::Deserialize;
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

const FILE_TOUCH_PREVIEW_MAX_LINES: usize = 6;
const FILE_TOUCH_PREVIEW_MAX_BYTES: usize = 240;

pub struct EditTool;

impl EditTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct EditInput {
    #[serde(default)]
    intent: Option<String>,
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
    /// Optional hashline tag for drift verification. When present, the current
    /// file hash must match the snapshot before any edit is applied.
    #[serde(default)]
    tag: Option<String>,
}

/// Write `content` to `path` atomically via temp file + rename.
async fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let pid = std::process::id();
    let temp_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("{name}.jcode-tmp.{pid}"),
        None => format!("jcode-tmp.{pid}"),
    };
    let temp_path = path.with_file_name(temp_name);
    if let Err(e) = tokio::fs::write(&temp_path, content).await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(anyhow::anyhow!(
            "failed to write temp file {}: {}",
            temp_path.display(),
            e
        ));
    }
    if let Err(e) = tokio::fs::rename(&temp_path, path).await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(anyhow::anyhow!(
            "failed to atomically rename {} -> {}: {}",
            temp_path.display(),
            path.display(),
            e
        ));
    }
    Ok(())
}

/// Attempt hashline-anchored edit using xxh32 anchor; fall back to str_replace.
fn apply_edit(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
    line: usize,
) -> (String, usize, usize, &'static str) {
    // Build a FileContent to compute native xxh32 hashes for anchor verification
    use std::path::PathBuf;
    let fc = FileContent {
        path: PathBuf::from(""),
        raw: content.to_string(),
        normalized: content.to_string(),
        newline: hashline::document::NewlineStyle::Lf,
        trailing_newline: content.ends_with('\n'),
        hash: "0000".into(),
    };
    let entries = fc.lines_with_hashes();
    let line_idx = line.saturating_sub(1);

    // Try xxh32 anchor verification (oh-my-pi native)
    let anchor_valid =
        line_idx < entries.len() && entries[line_idx].content.contains(old_string) && {
            let short = hashline_hash::format_short_hash(entries[line_idx].short_hash);
            let anchor_str = format!("{}:{}", line, short);
            anchor::parse_anchor(&anchor_str)
                .ok()
                .and_then(|a| anchor::resolve(&a, &fc).ok())
                .is_some()
        };

    if anchor_valid {
        // Anchor verified — do line-scoped replacement
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        if line_idx < lines.len() && lines[line_idx].contains(old_string) {
            if replace_all {
                lines[line_idx] = lines[line_idx].replace(old_string, new_string);
            } else {
                lines[line_idx] = lines[line_idx].replacen(old_string, new_string, 1);
            }
            let result = lines.join("\n");
            let end = line + new_string.lines().count().saturating_sub(1);
            return (result, line, end, "hashline");
        }
    }

    // Fallback: simple str_replace
    let label = "str_replace-fallback";
    if replace_all {
        let nc = content.replace(old_string, new_string);
        let start = line;
        let end = start + new_string.lines().count().saturating_sub(1);
        (nc, start, end, label)
    } else {
        let nc = content.replacen(old_string, new_string, 1);
        let start = line;
        let end = start + new_string.lines().count().saturating_sub(1);
        (nc, start, end, label)
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Replace text in a file. Uses hashline xxh32 anchored editing with drift detection; falls back to str_replace if anchor verification fails."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["file_path", "old_string", "new_string"],
            "properties": {
                "intent": super::intent_schema_property(),
                "file_path": {
                    "type": "string",
                    "description": "File path."
                },
                "old_string": {
                    "type": "string",
                    "description": "Text to replace."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all matches."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: EditInput = serde_json::from_value(input)?;

        if params.old_string == params.new_string {
            return Err(anyhow::anyhow!(
                "old_string and new_string must be different"
            ));
        }

        let path = ctx.resolve_path(Path::new(&params.file_path));

        // Verify optional hashline tag
        if let Some(ref tag) = params.tag {
            if let Some(snap) = hashline_snapshots::by_hash(&path, tag.as_str()) {
                if !path.exists() {
                    return Err(anyhow::anyhow!("File not found: {}", params.file_path));
                }
                let content = tokio::fs::read_to_string(&path).await?;
                let current_tag = hashline_snapshots::compute_file_tag(&content);
                if current_tag != snap.hash {
                    return Err(anyhow::anyhow!(
                        "File has changed since read (tag was {}, current is {}). \
                         Re-read the file before editing.",
                        snap.hash, current_tag
                    ));
                }
            }
        }

        if !path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", params.file_path));
        }

        let content = tokio::fs::read_to_string(&path).await?;

        // Count occurrences
        let occurrences = content.matches(&params.old_string).count();

        if occurrences == 0 {
            // Try flexible matching
            return try_flexible_match(&content, &params.old_string, &params.file_path);
        }

        if occurrences > 1 && !params.replace_all {
            return Err(anyhow::anyhow!(
                "old_string found {} times in the file. Either:\n\
                 1. Provide more context to make it unique, or\n\
                 2. Set replace_all: true to replace all occurrences",
                occurrences
            ));
        }

        // Find line number where old_string starts (for the first occurrence)
        let start_line = find_line_number(&content, &params.old_string);

        // Apply edit with hashline → str_replace fallback
        let (new_content, actual_start, actual_end, method) = apply_edit(
            &content,
            &params.old_string,
            &params.new_string,
            params.replace_all,
            start_line,
        );

        // Write back atomically
        atomic_write(&path, &new_content).await?;

        // Generate a diff with line numbers
        let diff = generate_diff(&params.old_string, &params.new_string, actual_start);

        // Publish file touch event for swarm coordination
        let end_line = actual_start + params.new_string.lines().count().saturating_sub(1);
        let detail = build_file_touch_preview(&diff);
        Bus::global().publish(BusEvent::FileTouch(FileTouch {
            session_id: ctx.session_id.clone(),
            path: path.to_path_buf(),
            op: FileOp::Edit,
            intent: params
                .intent
                .clone()
                .filter(|value| !value.trim().is_empty()),
            summary: Some(format!(
                "edited lines {}-{} ({} occurrence{}, {})",
                actual_start,
                end_line,
                occurrences,
                if occurrences == 1 { "" } else { "s" },
                method,
            )),
            detail,
        }));

        // FileChanged hook (fire-and-forget, observational)
        {
            let session_id = ctx.session_id.clone();
            let cwd = ctx
                .working_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let file_path = path.to_string_lossy().to_string();
            let hook_diff = diff.clone();
            tokio::spawn(async move {
                let hook_config = load_hooks_config();
                let hook_registry = HookRegistry::from_config(hook_config.clone());
                let dispatch_config = DispatchConfig::from_settings(&hook_config.settings);
                let mut hook_ctx = HookContext::new(&session_id, "", &cwd, "FileChanged");
                hook_ctx.file_path = Some(file_path.clone());
                let handlers = hook_registry.get_matching(&HookEvent::FileChanged, &hook_ctx);
                if !handlers.is_empty() {
                    let mut hook_input = HookInputBuilder::new()
                        .session(&session_id, &cwd)
                        .event("FileChanged")
                        .build();
                    hook_input.file_path = Some(file_path);
                    hook_input.change_type = Some("modified".to_string());
                    hook_input.diff = Some(hook_diff);
                    let _ = jcode_hooks::dispatch_hooks(
                        &HookEvent::FileChanged,
                        &hook_input,
                        &handlers,
                        &dispatch_config,
                    )
                    .await;
                }
            });
        }

        // Extract context around the edit to help with consecutive edits
        let end_line = actual_start + params.new_string.lines().count().saturating_sub(1);
        let context = extract_context(&new_content, actual_start, end_line, 3);

        Ok(ToolOutput::new(format!(
            "Edited {}: replaced {} occurrence(s) ({})\n{}\n\nContext after edit (lines {}-{}):\n{}",
            params.file_path, occurrences, method, diff, context.0, context.1, context.2
        ))
        .with_title(params.file_path.clone()))
    }
}

/// Find the 1-based line number where a substring starts
fn find_line_number(content: &str, substring: &str) -> usize {
    if let Some(pos) = content.find(substring) {
        content[..pos].lines().count() + 1
    } else {
        1
    }
}

/// Generate a compact diff: "42- old" / "42+ new"
fn generate_diff(old: &str, new: &str, start_line: usize) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();

    let mut old_line = start_line;
    let mut new_line = start_line;

    for change in diff.iter_all_changes() {
        let content = change.value().trim();
        let (prefix, line_num) = match change.tag() {
            ChangeTag::Delete => {
                let num = old_line;
                old_line += 1;
                if content.is_empty() {
                    continue;
                }
                ("-", num)
            }
            ChangeTag::Insert => {
                let num = new_line;
                new_line += 1;
                if content.is_empty() {
                    continue;
                }
                ("+", num)
            }
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
                continue;
            }
        };

        // Compact format: "42- content" (no spaces)
        output.push_str(&format!("{}{} {}\n", line_num, prefix, content));
    }

    if output.is_empty() {
        String::new()
    } else {
        output.trim_end().to_string()
    }
}

fn build_file_touch_preview(diff: &str) -> Option<String> {
    let trimmed = diff.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut lines = trimmed.lines();
    let mut preview = lines
        .by_ref()
        .take(FILE_TOUCH_PREVIEW_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let mut truncated = lines.next().is_some();

    if preview.len() > FILE_TOUCH_PREVIEW_MAX_BYTES {
        preview = crate::util::truncate_str(&preview, FILE_TOUCH_PREVIEW_MAX_BYTES)
            .trim_end()
            .to_string();
        truncated = true;
    }

    if truncated {
        preview.push_str("\n…");
    }

    Some(preview)
}

/// Extract lines around the edited region, returns (start_line, end_line, content)
fn extract_context(
    content: &str,
    edit_start: usize,
    edit_end: usize,
    padding: usize,
) -> (usize, usize, String) {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Calculate range with padding (1-indexed to 0-indexed)
    let start = edit_start.saturating_sub(padding + 1);
    let end = (edit_end + padding).min(total_lines);

    let context_lines: Vec<String> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>4}│ {}", start + i + 1, line))
        .collect();

    (start + 1, end, context_lines.join("\n"))
}

fn try_flexible_match(content: &str, old_string: &str, file_path: &str) -> Result<ToolOutput> {
    // Try trimmed matching
    let trimmed = old_string.trim();
    if content.contains(trimmed) && trimmed != old_string {
        return Err(anyhow::anyhow!(
            "old_string not found exactly, but found after trimming whitespace.\n\
             Try using the exact string from the file, including leading/trailing whitespace."
        ));
    }

    // Try line-by-line matching with normalized whitespace
    let old_lines: Vec<&str> = old_string.lines().collect();
    let content_lines: Vec<&str> = content.lines().collect();

    for (i, window) in content_lines.windows(old_lines.len()).enumerate() {
        let matches = window
            .iter()
            .zip(old_lines.iter())
            .all(|(a, b)| a.trim() == b.trim());

        if matches {
            return Err(anyhow::anyhow!(
                "old_string not found exactly, but found with different indentation around line {}.\n\
                 Make sure to preserve the exact whitespace from the file.",
                i + 1
            ));
        }
    }

    Err(anyhow::anyhow!(
        "old_string not found in {}.\n\
         Use the read tool to see the current file contents.",
        file_path
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_diff_single_line_change() {
        let old = "hello world";
        let new = "hello rust";
        let diff = generate_diff(old, new, 10);

        assert!(diff.contains("10- hello world"), "Should show deleted line");
        assert!(diff.contains("10+ hello rust"), "Should show added line");
    }

    #[test]
    fn test_generate_diff_multi_line() {
        let old = "line one\nline two\nline three";
        let new = "line one\nmodified two\nline three";
        let diff = generate_diff(old, new, 5);

        assert!(diff.contains("6- line two"), "Should show deleted line");
        assert!(diff.contains("6+ modified two"), "Should show added line");
        assert!(
            !diff.contains("line one"),
            "Should not show unchanged lines"
        );
        assert!(
            !diff.contains("line three"),
            "Should not show unchanged lines"
        );
    }

    #[test]
    fn test_generate_diff_addition_only() {
        let old = "first\nthird";
        let new = "first\nsecond\nthird";
        let diff = generate_diff(old, new, 1);

        assert!(diff.contains("+ second"), "Should show added line");
    }

    #[test]
    fn test_generate_diff_deletion_only() {
        let old = "first\nsecond\nthird";
        let new = "first\nthird";
        let diff = generate_diff(old, new, 1);

        assert!(diff.contains("- second"), "Should show deleted line");
    }

    #[test]
    fn test_generate_diff_no_changes() {
        let old = "same content";
        let new = "same content";
        let diff = generate_diff(old, new, 1);

        assert!(diff.is_empty(), "No changes should produce empty diff");
    }

    #[test]
    fn test_generate_diff_line_number_format() {
        let old = "old";
        let new = "new";
        let diff = generate_diff(old, new, 42);

        assert!(
            diff.contains("42- old"),
            "Should have line number directly before minus"
        );
        assert!(
            diff.contains("42+ new"),
            "Should have line number directly before plus"
        );
    }

    #[test]
    fn test_find_line_number() {
        let content = "line 1\nline 2\nline 3\nline 4";

        assert_eq!(find_line_number(content, "line 1"), 1);
        assert_eq!(find_line_number(content, "line 2"), 2);
        assert_eq!(find_line_number(content, "line 3"), 3);
        assert_eq!(find_line_number(content, "line 4"), 4);
        assert_eq!(find_line_number(content, "not found"), 1);
    }

    #[test]
    fn test_extract_context() {
        let content =
            "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10";

        let (start, end, ctx) = extract_context(content, 5, 5, 2);

        assert_eq!(start, 3, "Should start at line 3 (5 - 2)");
        assert_eq!(end, 7, "Should end at line 7 (5 + 2)");
        assert!(ctx.contains("line 3"), "Should include line 3");
        assert!(ctx.contains("line 5"), "Should include edited line 5");
        assert!(ctx.contains("line 7"), "Should include line 7");
        assert!(!ctx.contains("line 2"), "Should not include line 2");
        assert!(!ctx.contains("line 8"), "Should not include line 8");
    }

    #[test]
    fn test_extract_context_at_start() {
        let content = "line 1\nline 2\nline 3\nline 4\nline 5";

        let (start, _end, ctx) = extract_context(content, 1, 1, 2);

        assert_eq!(start, 1, "Should start at line 1 (can't go before)");
        assert!(ctx.contains("line 1"), "Should include line 1");
        assert!(ctx.contains("line 3"), "Should include line 3");
    }

    #[test]
    fn test_extract_context_at_end() {
        let content = "line 1\nline 2\nline 3\nline 4\nline 5";

        let (_start, end, ctx) = extract_context(content, 5, 5, 2);

        assert_eq!(end, 5, "Should end at line 5 (can't go past)");
        assert!(ctx.contains("line 5"), "Should include line 5");
        assert!(ctx.contains("line 3"), "Should include line 3");
    }

    #[test]
    fn test_extract_context_range_past_end() {
        let content = "line 1\nline 2\nline 3\nline 4\nline 5";

        let (start, end, ctx) = extract_context(content, 4, 10, 1);

        assert_eq!(start, 3, "Should start at line 3 (4 - 1)");
        assert_eq!(end, 5, "Should clamp to last line");
        assert!(ctx.contains("line 3"), "Should include line 3");
        assert!(ctx.contains("line 5"), "Should include line 5");
    }

    #[test]
    fn test_apply_edit_hashline_path() {
        let content = "fn main() {\n    println!(\"hello\");\n}\n";
        let old = "    println!(\"hello\");";
        let new = "    println!(\"world\");";
        let line = find_line_number(content, old);

        let (result, start, end, method) = apply_edit(content, old, new, false, line);
        assert_eq!(
            method, "hashline",
            "should use xxh32 hashline when verification passes"
        );
        assert_eq!(start, 2);
        assert_eq!(end, 2);
        assert!(result.contains("world"));
        assert!(!result.contains("hello"));
    }

    #[test]
    fn test_apply_edit_fallback_path() {
        // Content where old_string spans multiple lines — xxh32 anchor verify fails,
        // forcing str_replace fallback.
        let content = "line1\nline2\nline3";
        let old = "line1\nline2";
        let new = "merged line";
        let line = find_line_number(content, old);

        let (result, _start, _end, method) = apply_edit(content, old, new, false, line);
        assert_eq!(
            method, "str_replace-fallback",
            "multi-line old should fall back"
        );
        assert!(result.contains("merged line"));
        assert!(!result.contains("line1\nline2"));
    }

    #[test]
    fn test_apply_edit_replace_all_fallback() {
        let content = "abc foo abc bar abc baz";
        let old = "abc";
        let new = "XYZ";

        // xxh32 anchor verify fails when old_string appears multiple times on same line
        let line = find_line_number(content, old);
        let (result, _start, _end, method) = apply_edit(content, old, new, true, line);
        assert_eq!(method, "str_replace-fallback");
        assert_eq!(result, "XYZ foo XYZ bar XYZ baz");
    }
}
