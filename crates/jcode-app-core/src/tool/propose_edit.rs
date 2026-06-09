use super::{get_best_of_n_handle, Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use jcode_best_of_n::ProposedContentStore;
use serde::Deserialize;
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

const FILE_TOUCH_PREVIEW_MAX_LINES: usize = 6;
const FILE_TOUCH_PREVIEW_MAX_BYTES: usize = 240;

pub struct ProposeEditTool;

impl ProposeEditTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct ProposeEditInput {
    #[serde(default)]
    intent: Option<String>,
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for ProposeEditTool {
    fn name(&self) -> &str {
        "propose_edit"
    }

    fn description(&self) -> &str {
        "Propose an edit without applying it (best-of-N mode). Writes the proposed content to the ProposedContentStore for the orchestrator to evaluate."
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
        let params: ProposeEditInput = serde_json::from_value(input)?;

        if params.old_string == params.new_string {
            return Err(anyhow::anyhow!(
                "old_string and new_string must be different"
            ));
        }

        // Propose tools require best-of-N context. The orchestrator stamps
        // run_id/candidate_id on the subagent's ToolContext; if either is
        // missing, the propose_* tools have nothing to attribute the
        // proposal to and must refuse to run.
        let run_id = ctx
            .best_of_n_run_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("propose_edit requires best-of-N context"))?;
        let candidate_id = ctx
            .best_of_n_candidate_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("propose_edit requires best-of-N context"))?;

        // Resolve the store from the global best-of-N handle static. The
        // orchestrator registers itself during Agent::new_with_session via
        // set_best_of_n_handle(), so this is only Some() while a
        // best-of-N run is in flight.
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

        // Count occurrences
        let occurrences = content.matches(&params.old_string).count();

        if occurrences == 0 {
            return Err(anyhow::anyhow!(
                "old_string not found in {}. Use the read tool to see the current file contents.",
                params.file_path
            ));
        }

        if occurrences > 1 && !params.replace_all {
            return Err(anyhow::anyhow!(
                "old_string found {} times in the file. Either:\n\
                 1. Provide more context to make it unique, or\n\
                 2. Set replace_all: true to replace all occurrences",
                occurrences
            ));
        }

        // Perform replacement in memory (no disk write)
        let new_content = if params.replace_all {
            content.replace(&params.old_string, &params.new_string)
        } else {
            content.replacen(&params.old_string, &params.new_string, 1)
        };

        // Find line number where edit starts (for the diff preview)
        let start_line = find_line_number(&content, &params.old_string);
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
            "[PROPOSED] {}: would replace {} occurrence(s)\n{}\n\n\
             Proposal stored for candidate '{}' in run '{}' (not written to disk).",
            params.file_path, occurrences, diff, candidate_id, run_id
        ))
        .with_title(params.file_path.clone())
        .with_metadata(json!({
            "proposed": true,
            "run_id": run_id,
            "candidate_id": candidate_id,
            "occurrences": occurrences,
            "preview": preview,
        })))
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
    fn test_find_line_number() {
        let content = "line 1\nline 2\nline 3\nline 4";

        assert_eq!(find_line_number(content, "line 1"), 1);
        assert_eq!(find_line_number(content, "line 2"), 2);
        assert_eq!(find_line_number(content, "line 3"), 3);
        assert_eq!(find_line_number(content, "line 4"), 4);
        assert_eq!(find_line_number(content, "not found"), 1);
    }
}
