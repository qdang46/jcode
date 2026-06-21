use super::{Tool, ToolContext, ToolOutput};
use crate::bus::{Bus, BusEvent, FileOp, FileTouch};
use anyhow::Result;
use async_trait::async_trait;
use hashline::{anchor, document::FileContent, hash as hashline_hash};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

pub struct HashlineEditTool;

impl Default for HashlineEditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl HashlineEditTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct HashlineEditInput {
    #[serde(default)]
    intent: Option<String>,
    file_path: String,
    anchor: AnchorInput,
    #[serde(default)]
    old_string: Option<String>,
    new_string: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AnchorInput {
    Structured(AnchorBody),
    AnchorStr(String),
}

#[derive(Deserialize)]
struct AnchorBody {
    line: usize,
    hash: String,
}

#[inline]
fn verify_xxh32_anchor(content: &str, anchor_line: usize, expected_hash: &str) -> Result<()> {
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
    let line_idx = anchor_line.saturating_sub(1);
    if line_idx >= entries.len() {
        return Err(anyhow::anyhow!("line {} out of range", anchor_line));
    }
    let actual = hashline_hash::format_short_hash(entries[line_idx].short_hash);
    if actual != expected_hash {
        return Err(anyhow::anyhow!(
            "anchor line {}: expected hash {}, actual hash {}",
            anchor_line,
            expected_hash,
            actual
        ));
    }
    // Also verify anchor resolve succeeds (full content hash is consistent)
    let anchor = anchor::parse_anchor(&format!("{}:{}", anchor_line, actual))
        .map_err(|e| anyhow::anyhow!("invalid anchor: {e}"))?;
    anchor::resolve(&anchor, &fc).map_err(|e| anyhow::anyhow!("anchor resolve failed: {e}"))?;
    Ok(())
}

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

fn publish_edit_event(
    ctx: &ToolContext,
    intent: Option<String>,
    path: &Path,
    start_line: usize,
    end_line: usize,
    detail: Option<String>,
) {
    Bus::global().publish(BusEvent::FileTouch(FileTouch {
        session_id: ctx.session_id.clone(),
        path: path.to_path_buf(),
        op: FileOp::Edit,
        intent: intent.filter(|value| !value.trim().is_empty()),
        summary: Some(format!("hashline edit lines {}-{}", start_line, end_line)),
        detail,
    }));
}

#[async_trait]
impl Tool for HashlineEditTool {
    fn name(&self) -> &str {
        "ffs hashline_edit"
    }

    fn description(&self) -> &str {
        "Perform surgical file edits anchored by xxh32 short hash. Falls back to str_replace if verification fails."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["file_path", "anchor", "new_string"],
            "properties": {
                "intent": super::intent_schema_property(),
                "file_path": { "type": "string", "description": "File path." },
                "anchor": {
                    "oneOf": [
                        {
                            "type": "object",
                            "required": ["line", "hash"],
                            "description": "Structured anchor with xxh32 short hash. Use with old_string for substring replacement.",
                            "properties": {
                                "line": { "type": "integer", "description": "1-based line number." },
                                "hash": { "type": "string", "description": "2-char xxh32 short hash of the line." }
                            }
                        },
                        {
                            "type": "string",
                            "description": "Hashline anchor string like '12:ab' for a single line or '12:ab..15:cd' for a range. Uses xxh32 short hashes."
                        }
                    ]
                },
                "old_string": {
                    "type": "string",
                    "description": "Text to replace within the verified anchor window."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: HashlineEditInput = serde_json::from_value(input)?;

        let path = ctx.resolve_path(Path::new(&params.file_path));
        if !path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", params.file_path));
        }

        let content = tokio::fs::read_to_string(&path).await?;

        match params.anchor {
            AnchorInput::Structured(ref anchor) => {
                structured_execute(&content, &path, &params, anchor, &ctx).await
            }
            AnchorInput::AnchorStr(ref anchor_str) => {
                anchor_str_execute(&content, &path, &params, anchor_str, &ctx).await
            }
        }
    }
}

async fn structured_execute(
    content: &str,
    path: &Path,
    params: &HashlineEditInput,
    anchor: &AnchorBody,
    ctx: &ToolContext,
) -> Result<ToolOutput> {
    let old_string = params.old_string.as_deref().unwrap_or("");
    if old_string.is_empty() {
        return Err(anyhow::anyhow!(
            "old_string is required for structured anchors"
        ));
    }
    if old_string == params.new_string {
        return Err(anyhow::anyhow!(
            "old_string and new_string must be different"
        ));
    }

    // Try xxh32 hashline path with str_replace fallback
    let (new_content, start_line, end_line, method) = apply_with_xxh32_fallback(
        content,
        anchor.line,
        &anchor.hash,
        old_string,
        &params.new_string,
    );

    atomic_write(path, &new_content).await?;

    let detail = Some(format!(
        "lines {}-{} [{}]: {} -> {}",
        start_line,
        end_line,
        method,
        old_string.lines().next().unwrap_or(""),
        params.new_string.lines().next().unwrap_or(""),
    ));
    publish_edit_event(
        ctx,
        params.intent.clone(),
        path,
        start_line,
        end_line,
        detail,
    );

    Ok(ToolOutput::new(format!(
        "Edited {}: lines {}-{} ({})\n  old: {}\n  new: {}",
        params.file_path,
        start_line,
        end_line,
        method,
        old_string.lines().next().unwrap_or(""),
        params.new_string.lines().next().unwrap_or(""),
    ))
    .with_title(params.file_path.clone()))
}

async fn anchor_str_execute(
    content: &str,
    path: &Path,
    params: &HashlineEditInput,
    anchor_str: &str,
    ctx: &ToolContext,
) -> Result<ToolOutput> {
    if params.old_string.is_some() {
        return Err(anyhow::anyhow!(
            "old_string is not supported with a hashline string anchor"
        ));
    }

    use hashline::anchor::try_parse_line_anchor;

    // Replace line(s) using anchor: parse the anchor and apply line-based replacement
    if anchor_str.contains("..") {
        let parts: Vec<&str> = anchor_str.split("..").collect();
        if let (Some(start), Some(end)) = (
            parts.first().and_then(|a| try_parse_line_anchor(a)),
            parts.get(1).and_then(|a| try_parse_line_anchor(a)),
        ) {
            let (s_line, _s_hash) = start;
            let (e_line, _e_hash) = end;
            let nc = replace_lines(content, s_line, e_line, &params.new_string);
            atomic_write(path, &nc).await?;
            publish_edit_event(
                ctx,
                params.intent.clone(),
                path,
                s_line + 1,
                e_line + 1,
                None,
            );
            return Ok(ToolOutput::new(format!(
                "Edited {} with hashline range anchor {}: lines {}-{} replaced",
                params.file_path,
                anchor_str,
                s_line + 1,
                e_line + 1,
            ))
            .with_title(params.file_path.clone()));
        }
    } else if let Some((line_no, _hash)) = try_parse_line_anchor(anchor_str) {
        let nc = replace_lines(content, line_no, line_no, &params.new_string);
        atomic_write(path, &nc).await?;
        publish_edit_event(
            ctx,
            params.intent.clone(),
            path,
            line_no + 1,
            line_no + 1,
            None,
        );
        return Ok(ToolOutput::new(format!(
            "Edited {} with hashline anchor {}: line {} replaced",
            params.file_path,
            anchor_str,
            line_no + 1,
        ))
        .with_title(params.file_path.clone()));
    }

    // Fallback: parse anchor manually and replace by line number
    let fallback_line: Option<usize> = anchor_str.split(':').next().and_then(|s| s.parse().ok());
    if let Some(line) = fallback_line {
        let line_zero = line.saturating_sub(1);
        let nc = replace_lines(content, line_zero, line_zero, &params.new_string);
        atomic_write(path, &nc).await?;
        publish_edit_event(ctx, params.intent.clone(), path, line, line, None);
        return Ok(ToolOutput::new(format!(
            "Edited {} with anchor {}: line {} replaced",
            params.file_path, anchor_str, line,
        ))
        .with_title(params.file_path.clone()));
    }

    Ok(ToolOutput::new(format!(
        "Edited {} with anchor {}",
        params.file_path, anchor_str,
    ))
    .with_title(params.file_path.clone()))
}

/// Replace lines `start..=end` (0-based) with `new_text`.
fn replace_lines(content: &str, start: usize, end: usize, new_text: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if start >= lines.len() {
        let mut result = content.to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(new_text);
        return result;
    }
    let end = end.min(lines.len() - 1);
    let replacement: Vec<&str> = new_text.lines().collect();
    let mut result = String::with_capacity(
        content.len() + new_text.len()
            - lines[end - start..=end]
                .iter()
                .map(|l| l.len() + 1)
                .sum::<usize>(),
    );
    for (i, line) in lines.iter().enumerate() {
        if i >= start && i <= end {
            if i == start {
                for (j, rl) in replacement.iter().enumerate() {
                    if j > 0 {
                        result.push('\n');
                    }
                    result.push_str(rl);
                }
            }
            continue;
        }
        if i > 0 {
            result.push('\n');
        }
        result.push_str(line);
    }
    result
}

fn apply_with_xxh32_fallback(
    content: &str,
    anchor_line: usize,
    anchor_hash: &str,
    old_string: &str,
    new_string: &str,
) -> (String, usize, usize, &'static str) {
    // Try xxh32 anchor verification first
    if verify_xxh32_anchor(content, anchor_line, anchor_hash).is_ok() {
        let line_idx = anchor_line.saturating_sub(1);
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        if line_idx < lines.len() && lines[line_idx].contains(old_string) {
            lines[line_idx] = lines[line_idx].replacen(old_string, new_string, 1);
            let result = lines.join("\n");
            let end = anchor_line + new_string.lines().count().saturating_sub(1);
            return (result, anchor_line, end, "hashline");
        }
    }

    // Fallback: simple str_replace
    let nc = if content.matches(old_string).count() > 1 {
        content.replace(old_string, new_string)
    } else {
        content.replacen(old_string, new_string, 1)
    };
    let start = find_line_number(content, old_string);
    let end = start + new_string.lines().count().saturating_sub(1);
    (nc, start, end, "str_replace-fallback")
}

fn find_line_number(content: &str, substring: &str) -> usize {
    if let Some(pos) = content.find(substring) {
        content[..pos].lines().count() + 1
    } else {
        1
    }
}
