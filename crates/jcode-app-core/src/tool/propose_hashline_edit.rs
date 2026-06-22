use super::{Tool, ToolContext, ToolOutput, get_best_of_n_handle};
use crate::tool::hashline_snapshots;
use anyhow::Result;
use async_trait::async_trait;
use hashline::{anchor, document::FileContent, hash as hashline_hash};
use jcode_best_of_n::ProposedContentStore;
use serde::Deserialize;
use serde_json::{Value, json};
use similar::{ChangeTag, TextDiff};
use std::path::Path;

pub struct ProposeHashlineEditTool;

impl Default for ProposeHashlineEditTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ProposeHashlineEditTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProposeInput {
    Patch {
        file_path: String,
        #[serde(default)]
        intent: Option<String>,
        patch: String,
    },
    Structured {
        file_path: String,
        #[serde(default)]
        intent: Option<String>,
        anchor: HashlineAnchor,
        old_string: String,
        new_string: String,
    },
}

#[derive(Deserialize)]
struct HashlineAnchor {
    line: usize,
    hash: String,
}

const PROPOSE_DESCRIPTION: &str = "Propose a hashline edit without applying it (best-of-N mode). Two modes: (1) patch mode with [path#TAG] header + SWAP/DEL/INS ops, (2) anchor mode (legacy) with {line, hash} + old_string + new_string.";

#[async_trait]
impl Tool for ProposeHashlineEditTool {
    fn name(&self) -> &str {
        "propose_hashline"
    }

    fn description(&self) -> &str {
        PROPOSE_DESCRIPTION
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "file_path": { "type": "string", "description": "File path." },
                "patch": { "type": "string", "description": "Hashline patch with [path#TAG] header." },
                "anchor": { "type": "object", "required": ["line", "hash"], "properties": {
                    "line": { "type": "integer" },
                    "hash": { "type": "string" }
                }},
                "old_string": { "type": "string" },
                "new_string": { "type": "string" }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: ProposeInput = serde_json::from_value(input)?;

        let run_id = ctx.best_of_n_run_id.clone()
            .ok_or_else(|| anyhow::anyhow!("propose_hashline requires best-of-N context"))?;
        let candidate_id = ctx.best_of_n_candidate_id.clone()
            .ok_or_else(|| anyhow::anyhow!("propose_hashline requires best-of-N context"))?;

        let store: std::sync::Arc<ProposedContentStore> = {
            let handle = get_best_of_n_handle()
                .ok_or_else(|| anyhow::anyhow!("best-of-N handle not initialized"))?;
            handle.store.clone()
        };

        match params {
            ProposeInput::Patch { file_path, intent: _, patch } => {
                execute_propose_patch(file_path, patch, &run_id, &candidate_id, store, ctx).await
            }
            ProposeInput::Structured { file_path, intent: _, anchor, old_string, new_string } => {
                execute_propose_old(file_path, anchor, old_string, new_string, &run_id, &candidate_id, store, ctx).await
            }
        }
    }
}

async fn execute_propose_patch(
    file_path: String,
    patch: String,
    run_id: &str,
    candidate_id: &str,
    store: std::sync::Arc<ProposedContentStore>,
    ctx: ToolContext,
) -> Result<ToolOutput> {
    let path = ctx.resolve_path(Path::new(&file_path));
    if !path.exists() {
        return Err(anyhow::anyhow!("File not found: {file_path}"));
    }
    let current_raw = tokio::fs::read_to_string(&path).await?;
    let content = hashline::normalize::normalize_to_lf(&current_raw);

    let (edits, warnings, _has_block) = hashline::parser::parse_patch(&patch);
    if edits.is_empty() {
        let msg = if warnings.is_empty() {
            "hashline patch produced no edits".to_string()
        } else {
            format!("hashline patch produced no edits: {}", warnings.join("; "))
        };
        return Err(anyhow::anyhow!(msg));
    }

    // Verify snapshot tag if present in patch
    if let Some(t) = extract_tag_from_patch(&patch) {
        if let Some(snap) = hashline_snapshots::by_hash(&path, t.as_str()) {
            let current_tag = hashline_snapshots::compute_file_tag(&content);
            if current_tag != snap.hash {
                return Err(anyhow::anyhow!(
                    "file has changed since read (tag was {}, current is {}). Re-read first.",
                    snap.hash, current_tag
                ));
            }
        }
    }

    let path_str = path.to_string_lossy().to_string();
    let edits = hashline::block::resolve_block_edits(&edits, &content, &path_str, None)
        .map_err(|e| anyhow::anyhow!("block resolution: {e}"))?;

    let fc = FileContent {
        path: path.to_path_buf(),
        raw: current_raw,
        normalized: content.clone(),
        newline: hashline::document::NewlineStyle::Lf,
        trailing_newline: content.ends_with('\n'),
        hash: "0000".into(),
    };
    let entries = fc.lines_with_hashes();
    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    if content.ends_with('\n') && lines.last().map(|s| s.as_str()) == Some("") {
        lines.pop();
    }
    hashline::commands::patch::apply_edits(&mut lines, &entries, &path, &edits)?;

    let new_content = if content.ends_with('\n') {
        lines.join("\n") + "\n"
    } else {
        lines.join("\n")
    };

    let line_ending = hashline::normalize::detect_line_ending(&fc.raw);
    let final_text = if line_ending == hashline::normalize::LineEnding::Crlf {
        hashline::normalize::restore_line_endings(&new_content, line_ending)
    } else {
        new_content
    };

    let diff = generate_diff(&content, &final_text, 1);
    let preview = build_file_touch_preview(&diff);

    let run_id_typed = jcode_best_of_n::RunId(run_id.to_string());
    store.set_proposed(&run_id_typed, file_path.clone(), final_text, candidate_id.to_string(), false);

    let warnings_text = if warnings.is_empty() { String::new() } else { format!(" warnings: {}", warnings.join(", ")) };
    Ok(ToolOutput::new(format!(
        "[PROPOSED] {file_path}: hashline patch, {} edits applied{warnings_text}\n{diff}\n\nProposal stored for candidate '{candidate_id}' in run '{run_id}' (not written to disk).",
        edits.len(),
    ))
    .with_title(file_path.clone())
    .with_metadata(json!({
        "proposed": true,
        "run_id": run_id,
        "candidate_id": candidate_id,
        "preview": preview,
    })))
}

fn extract_tag_from_patch(patch: &str) -> Option<String> {
    let line = patch.lines().next()?.trim();
    let after_pound = line.rsplit('#').next()?;
    let tag = after_pound.trim_end_matches(']').trim();
    if tag.len() == 4 && tag.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(tag.to_uppercase())
    } else {
        None
    }
}

async fn execute_propose_old(
    file_path: String,
    anchor: HashlineAnchor,
    old_string: String,
    new_string: String,
    run_id: &str,
    candidate_id: &str,
    store: std::sync::Arc<ProposedContentStore>,
    ctx: ToolContext,
) -> Result<ToolOutput> {
    if old_string == new_string {
        return Err(anyhow::anyhow!("old_string and new_string must be different"));
    }
    let path = ctx.resolve_path(Path::new(&file_path));
    if !path.exists() {
        return Err(anyhow::anyhow!("File not found: {file_path}"));
    }
    let content = tokio::fs::read_to_string(&path).await?;
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
        let line_idx = anchor.line.saturating_sub(1);
        if line_idx >= entries.len() {
            return Err(anyhow::anyhow!("line {} out of range", anchor.line));
        }
        let actual = hashline_hash::format_short_hash(entries[line_idx].short_hash);
        if actual != anchor.hash {
            return Err(anyhow::anyhow!("anchor hash mismatch at line {}: expected {}, actual {}", anchor.line, anchor.hash, actual));
        }
        let anchor_obj = anchor::parse_anchor(&format!("{}:{}", anchor.line, actual))
            .map_err(|e| anyhow::anyhow!("invalid anchor: {e}"))?;
        anchor::resolve(&anchor_obj, &fc).map_err(|e| anyhow::anyhow!("anchor resolve failed: {e}"))?;
    }

    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let line_idx = anchor.line.saturating_sub(1);
    if line_idx >= lines.len() || !lines[line_idx].contains(&old_string) {
        return Err(anyhow::anyhow!("old_string not found on line {}", anchor.line));
    }
    lines[line_idx] = lines[line_idx].replacen(&old_string, &new_string, 1);
    let new_content = lines.join("\n");
    let start_line = anchor.line;
    let end_line = start_line + new_string.lines().count().saturating_sub(1);

    let diff = generate_diff(&old_string, &new_string, start_line);
    let preview = build_file_touch_preview(&diff);

    let run_id_typed = jcode_best_of_n::RunId(run_id.to_string());
    store.set_proposed(&run_id_typed, file_path.clone(), new_content, candidate_id.to_string(), false);

    Ok(ToolOutput::new(format!(
        "[PROPOSED] {file_path}: xxh32 hashline edit lines {start_line}-{end_line} (anchor verified)\n{diff}\n\nProposal stored for candidate '{candidate_id}' in run '{run_id}' (not written to disk)."
    ))
    .with_title(file_path.clone())
    .with_metadata(json!({
        "proposed": true,
        "run_id": run_id,
        "candidate_id": candidate_id,
        "start_line": start_line,
        "end_line": end_line,
        "preview": preview,
    })))
}

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
                if content.is_empty() { continue; }
                ("-", num)
            }
            ChangeTag::Insert => {
                let num = new_line;
                new_line += 1;
                if content.is_empty() { continue; }
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
    if output.is_empty() { String::new() } else { output.trim_end().to_string() }
}

fn build_file_touch_preview(diff: &str) -> Option<String> {
    let trimmed = diff.trim();
    if trimmed.is_empty() { return None; }
    const MAX_LINES: usize = 6;
    const MAX_BYTES: usize = 240;
    let mut lines = trimmed.lines();
    let mut preview = lines.by_ref().take(MAX_LINES).collect::<Vec<_>>().join("\n");
    let mut truncated = lines.next().is_some();
    if preview.len() > MAX_BYTES {
        preview = crate::util::truncate_str(&preview, MAX_BYTES).trim_end().to_string();
        truncated = true;
    }
    if truncated { preview.push_str("\n…"); }
    Some(preview)
}
