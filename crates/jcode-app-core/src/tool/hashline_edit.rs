use super::{Tool, ToolContext, ToolOutput};
use crate::bus::{Bus, BusEvent, FileOp, FileTouch};
use crate::tool::hashline_loop_guard::NoopGuard;
use crate::tool::hashline_snapshots;
use anyhow::Result;
use async_trait::async_trait;
use hashline::{anchor, document::FileContent, hash as hashline_hash};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::LazyLock;

static NOOP_GUARD: LazyLock<NoopGuard> = LazyLock::new(NoopGuard::new);

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
#[serde(untagged)]
enum HashlineEditInput {
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
        anchor: OldAnchor,
        #[serde(default)]
        old_string: Option<String>,
        new_string: String,
    },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OldAnchor {
    Body { line: usize, hash: String },
    Str(String),
}

#[async_trait]
impl Tool for HashlineEditTool {
    fn name(&self) -> &str {
        "hashline_edit"
    }
    fn description(&self) -> &str {
        "Edit files using hashline patch format or structured anchors."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "file_path": { "type": "string" },
                "patch": { "type": "string", "description": "Hashline patch with [path#TAG] + SWAP/DEL/INS ops." },
                "anchor": { "oneOf": [
                    { "type": "object", "required": ["line", "hash"], "properties": { "line": { "type": "integer" }, "hash": { "type": "string" } } },
                    { "type": "string" }
                ]},
                "old_string": { "type": "string" },
                "new_string": { "type": "string" }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: HashlineEditInput = serde_json::from_value(input)?;
        match params {
            HashlineEditInput::Patch {
                file_path,
                intent,
                patch,
            } => execute_patch(file_path, intent, patch, ctx).await,
            HashlineEditInput::Structured {
                file_path,
                intent,
                anchor,
                old_string,
                new_string,
            } => execute_old(file_path, intent, anchor, old_string, new_string, ctx).await,
        }
    }
}

// ── Patch mode ───────────────────────────────────────────────────

async fn execute_patch(
    file_path: String,
    intent: Option<String>,
    patch: String,
    ctx: ToolContext,
) -> Result<ToolOutput> {
    let path = ctx.resolve_path(Path::new(&file_path));
    if !path.exists() {
        return Err(anyhow::anyhow!("File not found: {file_path}"));
    }
    let raw = tokio::fs::read_to_string(&path).await?;
    let content = hashline::normalize::normalize_to_lf(&raw);

    let (edits, pw, _has_block) = hashline::parser::parse_patch(&patch);
    if edits.is_empty() {
        let m = if pw.is_empty() {
            "empty patch".into()
        } else {
            format!("empty patch: {}", pw.join("; "))
        };
        return Err(anyhow::anyhow!("{m}"));
    }

    let new_text = {
        let snap_tag = extract_tag(&patch);
        let snapshot = snap_tag
            .as_ref()
            .and_then(|t| hashline_snapshots::by_hash(&path, t));
        match &snapshot {
            Some(snap) => {
                let al = collect_anchor_lines(&edits);
                if !hashline_snapshots::lines_were_seen(snap.seen_lines.as_ref(), &al) {
                    return Err(anyhow::anyhow!("edit refs unseen lines — re-read first"));
                }
                let ctag = hashline_snapshots::compute_file_tag(&content);
                if ctag != snap.hash {
                    let store = hashline_snapshots::global();
                    let recovered = {
                        let guard = store.read().expect("hashline snapshots lock poisoned");
                        try_recover(&*guard, &path, &content, &snap.hash, &edits)
                    };
                    if let Some(rec) = recovered {
                        let mut msg = format!("Recovered {file_path}: file changed since read");
                        if !rec.warnings.is_empty() {
                            msg.push_str(&format!(" ({})", rec.warnings.join("; ")));
                        }
                        atomic_write(&path, &rec.text).await?;
                        hashline_snapshots::record(&path, &rec.text, None);
                        return Ok(ToolOutput::new(msg).with_title(file_path));
                    }
                    return Err(anyhow::anyhow!(
                        "File changed (tag {}→{}) and recovery failed. Re-read.",
                        snap.hash,
                        ctag
                    ));
                }
                apply_edits_to_text(&content, &edits)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .text
            }
            None => {
                apply_edits_to_text(&content, &edits)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .text
            }
        }
    };

    let ch = xxhash64(&content);
    let nd = new_text == content || new_text == format!("{content}\n");
    if let Err(e) = NOOP_GUARD.record(path.to_path_buf(), !nd, ch) {
        return Err(anyhow::anyhow!("{e}"));
    }
    if nd {
        return Ok(ToolOutput::new(format!("(no change) {file_path}")));
    }

    let le = hashline::normalize::detect_line_ending(&raw);
    let ft = if le == hashline::normalize::LineEnding::Crlf {
        hashline::normalize::restore_line_endings(&new_text, le)
    } else {
        new_text
    };
    atomic_write(&path, &ft).await?;
    NOOP_GUARD.reset(&path);
    hashline_snapshots::record(&path, &ft, None);

    let ws = if pw.is_empty() {
        String::new()
    } else {
        format!(" (warnings: {})", pw.join("; "))
    };
    publish_edit_event(&ctx, intent, &path, 1, 1, None);
    Ok(ToolOutput::new(format!(
        "Edited {file_path}: {} edits applied{ws}",
        edits.len()
    ))
    .with_title(file_path))
}

fn apply_edits_to_text(
    text: &str,
    edits: &[hashline::types::Edit],
) -> std::result::Result<hashline::types::ApplyResult, String> {
    let p = std::path::Path::new("");
    let pstr = p.to_string_lossy();
    let edits = hashline::block::resolve_block_edits(edits, text, &pstr, None)
        .map_err(|e| e.to_string())?;
    let fc = FileContent {
        path: std::path::PathBuf::from(""),
        raw: text.to_string(),
        normalized: text.to_string(),
        newline: hashline::document::NewlineStyle::Lf,
        trailing_newline: text.ends_with('\n'),
        hash: "0000".into(),
    };
    let entries = fc.lines_with_hashes();
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    if text.ends_with('\n') && lines.last().map(|s| s.as_str()) == Some("") {
        lines.pop();
    }
    hashline::commands::patch::apply_edits(&mut lines, &entries, p, &edits)
        .map_err(|e| e.to_string())?;
    let nt = if text.ends_with('\n') {
        lines.join("\n") + "\n"
    } else {
        lines.join("\n")
    };
    let changed = if nt == text { None } else { Some(1) };
    Ok(hashline::types::ApplyResult {
        text: nt,
        first_changed_line: changed,
        warnings: Vec::new(),
        block_resolutions: Vec::new(),
    })
}

fn extract_tag(patch: &str) -> Option<String> {
    let line = patch.lines().next()?.trim();
    let tag = line.rsplit('#').next()?.trim_end_matches(']').trim();
    if tag.len() == 4 && tag.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(tag.to_uppercase())
    } else {
        None
    }
}

fn try_recover(
    store: &dyn hashline::snapshot_store::SnapshotStore,
    path: &Path,
    content: &str,
    snap_hash: &str,
    edits: &[hashline::types::Edit],
) -> Option<hashline::recovery::RecoveryResult> {
    let recovery = hashline::recovery::Recovery::new(store);
    let rargs = hashline::recovery::RecoveryArgs {
        path: path.to_string_lossy().to_string(),
        current_text: content.to_string(),
        file_hash: snap_hash.to_string(),
        edits: edits.to_vec(),
    };
    recovery.try_recover(&rargs, apply_edits_to_text)
}

fn collect_anchor_lines(edits: &[hashline::types::Edit]) -> Vec<usize> {
    use hashline::types::{Cursor, Edit};
    edits
        .iter()
        .filter_map(|e| match e {
            Edit::Insert { cursor, .. } => match cursor {
                Cursor::BeforeAnchor(a) | Cursor::AfterAnchor(a) => Some(a.line),
                _ => None,
            },
            Edit::Delete { anchor, .. } => Some(anchor.line),
            Edit::Block { anchor, .. } => Some(anchor.line),
        })
        .collect()
}

fn xxhash64(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

// ── Old mode ────────────────────────────────────────────────────

async fn execute_old(
    file_path: String,
    intent: Option<String>,
    anchor: OldAnchor,
    old_string: Option<String>,
    new_string: String,
    ctx: ToolContext,
) -> Result<ToolOutput> {
    let path = ctx.resolve_path(Path::new(&file_path));
    if !path.exists() {
        return Err(anyhow::anyhow!("File not found: {file_path}"));
    }
    let raw = tokio::fs::read_to_string(&path).await?;
    let le = hashline::normalize::detect_line_ending(&raw);
    let content = hashline::normalize::normalize_to_lf(&raw);
    match anchor {
        OldAnchor::Body { line, hash } => {
            let old = old_string.as_deref().unwrap_or("");
            if old.is_empty() {
                return Err(anyhow::anyhow!("old_string required"));
            }
            if old == new_string {
                return Err(anyhow::anyhow!("old and new must differ"));
            }
            let (nc, sl, el, method) =
                apply_with_xxh32_fallback(&content, line, &hash, old, &new_string);
            let ft = if le == hashline::normalize::LineEnding::Crlf {
                hashline::normalize::restore_line_endings(&nc, le)
            } else {
                nc
            };
            atomic_write(&path, &ft).await?;
            hashline_snapshots::record(&path, &ft, None);
            publish_edit_event(&ctx, intent, &path, sl, el, None);
            Ok(
                ToolOutput::new(format!("Edited {file_path}: lines {sl}-{el} ({method})"))
                    .with_title(file_path),
            )
        }
        OldAnchor::Str(anchor_str) => {
            if old_string.is_some() {
                return Err(anyhow::anyhow!(
                    "old_string not supported with string anchor"
                ));
            }
            let line: usize = anchor_str
                .split(':')
                .next()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("invalid anchor: {anchor_str}"))?;
            let nc = replace_lines(
                &content,
                line.saturating_sub(1),
                line.saturating_sub(1),
                &new_string,
            );
            let ft = if le == hashline::normalize::LineEnding::Crlf {
                hashline::normalize::restore_line_endings(&nc, le)
            } else {
                nc
            };
            atomic_write(&path, &ft).await?;
            hashline_snapshots::record(&path, &ft, None);
            publish_edit_event(&ctx, intent, &path, line, line, None);
            Ok(
                ToolOutput::new(format!("Edited {file_path}: line {line} replaced"))
                    .with_title(file_path),
            )
        }
    }
}

// ── Shared helpers ──────────────────────────────────────────────

async fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let pid = std::process::id();
    let temp_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => format!("{n}.jcode-tmp.{pid}"),
        None => format!("jcode-tmp.{pid}"),
    };
    let tp = path.with_file_name(temp_name);
    if let Err(e) = tokio::fs::write(&tp, content).await {
        let _ = tokio::fs::remove_file(&tp).await;
        return Err(anyhow::anyhow!("write temp {}: {}", tp.display(), e));
    }
    if let Err(e) = tokio::fs::rename(&tp, path).await {
        let _ = tokio::fs::remove_file(&tp).await;
        return Err(anyhow::anyhow!("rename: {}", e));
    }
    Ok(())
}

fn publish_edit_event(
    ctx: &ToolContext,
    intent: Option<String>,
    path: &Path,
    sl: usize,
    el: usize,
    detail: Option<String>,
) {
    Bus::global().publish(BusEvent::FileTouch(FileTouch {
        session_id: ctx.session_id.clone(),
        path: path.to_path_buf(),
        op: FileOp::Edit,
        intent: intent.filter(|v| !v.trim().is_empty()),
        summary: Some(format!("hashline edit lines {sl}-{el}")),
        detail,
    }));
}

fn replace_lines(content: &str, start: usize, end: usize, new_text: &str) -> String {
    let has_trailing_nl = content.ends_with('\n');
    let lines: Vec<&str> = content.lines().collect();
    if start >= lines.len() {
        let mut r = content.to_string();
        if !r.ends_with('\n') {
            r.push('\n');
        }
        r.push_str(new_text);
        if has_trailing_nl && !r.ends_with('\n') {
            r.push('\n');
        }
        return r;
    }
    let end = end.min(lines.len() - 1);
    let rep: Vec<&str> = new_text.lines().collect();
    let mut r = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i >= start && i <= end {
            if i == start {
                for (j, rl) in rep.iter().enumerate() {
                    if j > 0 {
                        r.push('\n');
                    }
                    r.push_str(rl);
                }
            }
            continue;
        }
        if i > 0 {
            r.push('\n');
        }
        r.push_str(l);
    }
    if has_trailing_nl && !r.ends_with('\n') {
        r.push('\n');
    }
    r
}

fn apply_with_xxh32_fallback(
    content: &str,
    anchor_line: usize,
    anchor_hash: &str,
    old: &str,
    new_str: &str,
) -> (String, usize, usize, &'static str) {
    if verify_xxh32_anchor(content, anchor_line, anchor_hash).is_ok() {
        let li = anchor_line.saturating_sub(1);
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        if li < lines.len() && lines[li].contains(old) {
            lines[li] = lines[li].replacen(old, new_str, 1);
            return (
                lines.join("\n"),
                anchor_line,
                anchor_line + new_str.lines().count().saturating_sub(1),
                "hashline",
            );
        }
    }
    let nc = content.replacen(old, new_str, 1);
    let start = find_line_number(content, old);
    (
        nc,
        start,
        start + new_str.lines().count().saturating_sub(1),
        "str_replace-fallback",
    )
}

fn find_line_number(content: &str, substring: &str) -> usize {
    content
        .find(substring)
        .map(|pos| content[..pos].lines().count() + 1)
        .unwrap_or(1)
}

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
    let li = anchor_line.saturating_sub(1);
    if li >= entries.len() {
        return Err(anyhow::anyhow!("line {anchor_line} out of range"));
    }
    let actual = hashline_hash::format_short_hash(entries[li].short_hash);
    if actual != expected_hash {
        return Err(anyhow::anyhow!(
            "anchor line {anchor_line}: expected {expected_hash}, actual {actual}"
        ));
    }
    let a = anchor::parse_anchor(&format!("{anchor_line}:{actual}"))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    anchor::resolve(&a, &fc).map_err(|e| anyhow::anyhow!("anchor resolve: {e}"))?;
    Ok(())
}
