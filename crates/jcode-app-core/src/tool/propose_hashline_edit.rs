use super::{get_best_of_n_handle, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use hashline::sha256_window;
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
    hash_sha256: String,
    #[serde(default = "default_context_window")]
    context_window: usize,
}

fn default_context_window() -> usize {
    0
}

#[async_trait]
impl Tool for ProposeHashlineEditTool {
    fn name(&self) -> &str {
        "propose_hashline_edit"
    }

    fn description(&self) -> &str {
        "Propose a hashline-anchored edit without applying it (best-of-N mode). \
         Writes the proposed content to the ProposedContentStore for the orchestrator to evaluate."
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
                    "required": ["line", "hash_sha256"],
                    "description": "Line hash anchor for edit verification.",
                    "properties": {
                        "line": {
                            "type": "integer",
                            "description": "1-based line number in the file."
                        },
                        "hash_sha256": {
                            "type": "string",
                            "description": "SHA-256 hash of the anchor window (line +/- context_window)."
                        },
                        "context_window": {
                            "type": "integer",
                            "description": "Number of surrounding lines to include in hash (default: 0)."
                        }
                    }
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to replace within the verified anchor window."
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

        // Step 1: Verify the anchor hash
        sha256_window::verify_anchor(
            &content,
            params.anchor.line,
            &params.anchor.hash_sha256,
            params.anchor.context_window,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Step 2: Apply edit within the anchor window (in memory)
        let (new_content, start_line, end_line) = sha256_window::apply_edit_within_window(
            &content,
            params.anchor.line,
            &params.old_string,
            &params.new_string,
            params.anchor.context_window,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Generate diff preview
        let diff = generate_diff(
            &params.old_string,
            &params.new_string,
            start_line,
        );
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
            "[PROPOSED] {}: hashline edit lines {}-{} (anchor verified)\n{}\n\n\
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
