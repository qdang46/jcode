use super::{Tool, ToolContext, ToolOutput, get_best_of_n_handle};
use anyhow::Result;
use async_trait::async_trait;
use hashline::{anchor, document::FileContent, hash as hashline_hash};
use jcode_best_of_n::ProposedContentStore;
use serde::Deserialize;
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

const FILE_TOUCH_PREVIEW_MAX_LINES: usize = 6;
const FILE_TOUCH_PREVIEW_MAX_BYTES: usize = 240;

pub struct ProposeHashlineEditTool;

impl ProposeHashlineEditTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct ProposeHashlineEditInput {
    #[serde(default)]
    intent: Option<String>,
    file_path: String,
    anchor: HashlineAnchor,
    old_string: String,
    new_string: String,
}

#[derive(Deserialize)]
struct HashlineAnchor {
    line: usize,
    hash: String,
}

#[async_trait]
impl Tool for ProposeHashlineEditTool {
    fn name(&self) -> &str {
        "ffs propose_hashline"
    }

    fn description(&self) -> &str {
        "Propose a xxh32 hashline edit without applying it (best-of-N mode). Writes proposed content to the ProposedContentStore."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["file_path", "anchor", "old_string", "new_string"],
            "properties": {
                "intent": super::intent_schema_property(),
                "file_path": {
                    "type": "string",
                    "description": "File path."
                },
                "anchor": {
                    "type": "object",
                    "required": ["line", "hash"],
                    "description": "xxh32 short hash anchor for edit verification.",
                    "properties": {
                        "line": {
                            "type": "integer",
                            "description": "1-based line number in the file."
                        },
                        "hash": {
                            "type": "string",
                            "description": "2-char xxh32 short hash of the line."
                        }
                    }
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace on the verified line."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: ProposeHashlineEditInput = serde_json::from_value(input)?;

        if params.old_string == params.new_string {
            return Err(anyhow::anyhow!(
                "old_string and new_string must be different"
            ));
        }

        // Propose tools require best-of-N context.
        let run_id = ctx
            .best_of_n_run_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("propose_hashline_edit requires best-of-N context"))?;
        let candidate_id = ctx
            .best_of_n_candidate_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("propose_hashline_edit requires best-of-N context"))?;

        // Resolve the store from the global best-of-N handle static.
        let store: std::sync::Arc<ProposedContentStore> = {
            let handle = get_best_of_n_handle()
                .ok_or_else(|| anyhow::anyhow!("best-of-N handle not initialized"))?;
            handle.store.clone()
        };

        let path = ctx.resolve_path(Path::new(&params.file_path));

        if !path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", params.file_path));
        }

        let content = tokio::fs::read_to_string(&path).await?;

        // Step 1: Verify the xxh32 anchor hash
        {
            use std::path::PathBuf;
            let fc = FileContent {
                path: PathBuf::from(""),
                raw: content.clone(),
                normalized: content.clone(),
                newline: hashline::document::NewlineStyle::Lf,
                trailing_newline: content.ends_with('\n'),
                hash: "0000".into(),
            };
            let entries = fc.lines_with_hashes();
            let line_idx = params.anchor.line.saturating_sub(1);
            if line_idx >= entries.len() {
                return Err(anyhow::anyhow!("line {} out of range", params.anchor.line));
            }
            let actual = hashline_hash::format_short_hash(entries[line_idx].short_hash);
            if actual != params.anchor.hash {
                return Err(anyhow::anyhow!(
                    "anchor hash mismatch at line {}: expected {}, actual {}",
                    params.anchor.line,
                    params.anchor.hash,
                    actual
                ));
            }
            let anchor_obj = anchor::parse_anchor(&format!("{}:{}", params.anchor.line, actual))
                .map_err(|e| anyhow::anyhow!("invalid anchor: {e}"))?;
            anchor::resolve(&anchor_obj, &fc)
                .map_err(|e| anyhow::anyhow!("anchor resolve failed: {e}"))?;
        }

        // Step 2: Apply edit — line-scoped replacement
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        let line_idx = params.anchor.line.saturating_sub(1);
        if line_idx >= lines.len() || !lines[line_idx].contains(&params.old_string) {
            return Err(anyhow::anyhow!(
                "old_string not found on line {}",
                params.anchor.line
            ));
        }
        lines[line_idx] = lines[line_idx].replacen(&params.old_string, &params.new_string, 1);
        let new_content = lines.join("\n");
        let start_line = params.anchor.line;
        let end_line = start_line + params.new_string.lines().count().saturating_sub(1);

        // Generate diff preview
        let diff = generate_diff(&params.old_string, &params.new_string, start_line);
        let preview = build_file_touch_preview(&diff);

        // Write the proposed content to the store. is_new_file = false
        // because the file already exists on disk.
        let run_id_typed = jcode_best_of_n::RunId(run_id.clone());
        store.set_proposed(
            &run_id_typed,
            params.file_path.clone(),
            new_content.clone(),
            candidate_id.clone(),
            false,
        );

        Ok(ToolOutput::new(format!(
            "[PROPOSED] {}: xxh32 hashline edit lines {}-{} (anchor verified)\n{}\n\n\
             Proposal stored for candidate '{}' in run '{}' (not written to disk).",
            params.file_path, start_line, end_line, diff, candidate_id, run_id
        ))
        .with_title(params.file_path.clone())
        .with_metadata(json!({
            "proposed": true,
            "run_id": run_id,
            "candidate_id": candidate_id,
            "start_line": start_line,
            "end_line": end_line,
            "preview": preview,
        })))
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
