use super::{Tool, ToolContext, ToolOutput};
use crate::bus::{Bus, BusEvent, FileOp, FileTouch};
use anyhow::Result;
use async_trait::async_trait;
use hashline::sha256_window;
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
    hash_sha256: String,
    #[serde(default = "default_context_window")]
    context_window: usize,
}

fn default_context_window() -> usize {
    0
}

#[inline]
fn verify_anchor(
    content: &str,
    anchor_line: usize,
    expected_hash: &str,
    context_window: usize,
) -> Result<()> {
    sha256_window::verify_anchor(content, anchor_line, expected_hash, context_window)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

#[inline]
fn apply_edit_within_window(
    content: &str,
    anchor_line: usize,
    old_string: &str,
    new_string: &str,
    context_window: usize,
) -> Result<(String, usize, usize)> {
    sha256_window::apply_edit_within_window(
        content,
        anchor_line,
        old_string,
        new_string,
        context_window,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
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
        "hashline_edit"
    }

    fn description(&self) -> &str {
        "Perform surgical file edits anchored by line hash verification. Falls back to str_replace if hashline verification fails."
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
                            "required": ["line", "hash_sha256"],
                            "description": "Structured anchor with SHA-256 line hash. Use with old_string for substring replacement.",
                            "properties": {
                                "line": { "type": "integer", "description": "1-based line number." },
                                "hash_sha256": { "type": "string", "description": "SHA-256 hash of the anchor window." },
                                "context_window": { "type": "integer", "description": "Surrounding lines for hash (default: 0)." }
                            }
                        },
                        {
                            "type": "string",
                            "description": "Hashline anchor string like '12:ab' for a single line or '12:ab..15:cd' for a range. Uses xxh32 short hashes. Only valid when old_string is omitted."
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

    // Try hashline path with str_replace fallback
    let (new_content, start_line, end_line, method) = apply_with_fallback(
        content,
        anchor.line,
        old_string,
        &params.new_string,
        anchor.context_window,
    );

    atomic_write(path, &new_content).await?;

    let detail = Some(format!(
        "lines {}-{} ({}): {} -> {}",
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

    use hashline::anchor;
    use hashline::document::Document;
    use hashline::mutation;

    let doc = Document::from_str(path, content).map_err(|e| anyhow::anyhow!("{e}"))?;
    let index = doc.build_index();

    if anchor::looks_like_range_anchor(anchor_str) {
        let range = anchor::parse_range(anchor_str).map_err(|e| anyhow::anyhow!("{e}"))?;
        let (start_resolved, end_resolved) =
            anchor::resolve_range(&range, &doc, &index).map_err(|e| anyhow::anyhow!("{e}"))?;
        let (start_line, end_line) = (start_resolved.line_no, end_resolved.line_no);
        let mut doc = doc;
        mutation::replace_range(
            &mut doc,
            start_resolved.index,
            end_resolved.index,
            &params.new_string,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        let new_content = String::from_utf8(doc.render()).map_err(|e| anyhow::anyhow!("{e}"))?;

        atomic_write(path, &new_content).await?;
        publish_edit_event(ctx, params.intent.clone(), path, start_line, end_line, None);

        Ok(ToolOutput::new(format!(
            "Edited {} with hashline range anchor {}: lines {}-{} replaced",
            params.file_path, anchor_str, start_line, end_line,
        ))
        .with_title(params.file_path.clone()))
    } else {
        let parsed = anchor::parse_anchor(anchor_str).map_err(|e| anyhow::anyhow!("{e}"))?;
        let resolved =
            anchor::resolve(&parsed, &doc, &index).map_err(|e| anyhow::anyhow!("{e}"))?;
        let line = resolved.line_no;
        let idx = resolved.index;
        let mut doc = doc;
        mutation::replace_range(&mut doc, idx, idx, &params.new_string)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let new_content = String::from_utf8(doc.render()).map_err(|e| anyhow::anyhow!("{e}"))?;

        atomic_write(path, &new_content).await?;
        publish_edit_event(ctx, params.intent.clone(), path, line, line, None);

        Ok(ToolOutput::new(format!(
            "Edited {} with hashline anchor {}: line {} replaced",
            params.file_path, anchor_str, line,
        ))
        .with_title(params.file_path.clone()))
    }
}

fn apply_with_fallback(
    content: &str,
    anchor_line: usize,
    old_string: &str,
    new_string: &str,
    context_window: usize,
) -> (String, usize, usize, &'static str) {
    if let Ok((nc, s, e)) =
        apply_edit_within_window(content, anchor_line, old_string, new_string, context_window)
    {
        return (nc, s, e, "hashline");
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
