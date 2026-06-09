use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

async fn current_handle() -> Option<crate::tool::BestOfNOrchestratorHandle> {
    super::get_best_of_n_handle()
}

pub struct ProposeWriteTool;

impl ProposeWriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct ProposeWriteInput {
    #[serde(default)]
    intent: Option<String>,
    file_path: String,
    content: String,
}

#[async_trait]
impl Tool for ProposeWriteTool {
    fn name(&self) -> &str {
        "propose_write"
    }

    fn description(&self) -> &str {
        "Propose writing a file without applying it (best-of-N mode)"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["file_path", "content"],
            "properties": {
                "intent": super::intent_schema_property(),
                "file_path": {
                    "type": "string",
                    "description": "File path."
                },
                "content": {
                    "type": "string",
                    "description": "File content."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: ProposeWriteInput = serde_json::from_value(input)?;

        let path = ctx.resolve_path(Path::new(&params.file_path));

        // Read old content from disk (if any) for the diff summary.
        let existed = path.exists();
        let old_content = if existed {
            tokio::fs::read_to_string(&path).await.ok()
        } else {
            None
        };

        // Best-of-N context: run id + candidate id must be set on the
        // tool context, otherwise this tool was called outside a
        // candidate execution and we have nowhere to store the proposal.
        let run_id = ctx
            .best_of_n_run_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("propose_write requires best-of-N context"))?;
        let candidate_id = ctx
            .best_of_n_candidate_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("propose_write requires best-of-N context"))?;

        // The handle itself lives on the shared `Registry`. If the agent
        // never installed a best-of-N orchestrator we have no place to
        // record the proposal and the call must fail.
        let handle = current_handle()
            .await
            .ok_or_else(|| anyhow::anyhow!("propose_write requires best-of-N context"))?;

        // Use the user-supplied path as the dedup key inside the store
        // so it matches original_files[].file_path in build_diffs.
        let file_key = params.file_path.clone();
        let run_id_typed = jcode_best_of_n::RunId(run_id.clone());
        handle
            .store
            .set_proposed(&run_id_typed, file_key, &params.content, candidate_id.clone(), !existed);

        let line_count = params.content.lines().count();
        let diff = match old_content.as_deref() {
            Some(old) => generate_diff_summary(old, &params.content),
            None => generate_diff_summary("", &params.content),
        };

        let header = if existed {
            format!(
                "[PROPOSED] update {} ({} lines){}",
                params.file_path,
                line_count,
                if diff.is_empty() { "" } else { ":" }
            )
        } else {
            format!(
                "[PROPOSED] create {} ({} lines):",
                params.file_path, line_count
            )
        };

        Ok(ToolOutput::new(format!("{header}\n{diff}")).with_title(params.file_path.clone()))
    }
}

/// Generate a compact diff: "42- old" / "42+ new" (max 20 lines).
///
/// Mirrors the helper used in `write.rs` so the proposed-mode output is
/// visually identical to a real write (modulo the `[PROPOSED]` prefix).
fn generate_diff_summary(old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();
    let mut lines_shown = 0;
    const MAX_LINES: usize = 20;

    let mut old_line = 1usize;
    let mut new_line = 1usize;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
                continue;
            }
            ChangeTag::Delete => {
                let content = change.value().trim();
                old_line += 1;
                if content.is_empty() {
                    continue;
                }
                if lines_shown >= MAX_LINES {
                    output.push_str("...\n");
                    break;
                }
                output.push_str(&format!("{}- {}\n", old_line - 1, content));
                lines_shown += 1;
            }
            ChangeTag::Insert => {
                let content = change.value().trim();
                new_line += 1;
                if content.is_empty() {
                    continue;
                }
                if lines_shown >= MAX_LINES {
                    output.push_str("...\n");
                    break;
                }
                output.push_str(&format!("{}+ {}\n", new_line - 1, content));
                lines_shown += 1;
            }
        }
    }

    output.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_diff_summary_new_file() {
        let old = "";
        let new = "line one\nline two\nline three";
        let diff = generate_diff_summary(old, new);

        assert!(diff.contains("1+ line one"));
        assert!(diff.contains("2+ line two"));
        assert!(diff.contains("3+ line three"));
    }

    #[test]
    fn test_generate_diff_summary_modify() {
        let old = "line one\nline two\nline three";
        let new = "line one\nchanged two\nline three";
        let diff = generate_diff_summary(old, new);

        assert!(diff.contains("2- line two"));
        assert!(diff.contains("2+ changed two"));
    }
}
