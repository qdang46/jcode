use super::{Tool, ToolContext, ToolOutput};
use crate::message::{ContentBlock, ToolCall};
use crate::session::Session;
use crate::storage;
use crate::{logging, util};
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod args;
mod context;
mod render;

#[cfg(test)]
use self::args::trace_or_smart_terms_owned;
use self::args::{build_smart_args_and_query, resolve_search_root, summarize_agentgrep_request};
use self::context::maybe_write_context_json;
#[cfg(test)]
use self::context::{
    collect_bash_exposure, collect_trace_exposure, tune_known_file, tune_known_region,
};
use self::render::render_smart_output;

// ─── Input types (unchanged) ───

#[derive(Debug, Deserialize)]
struct AgentGrepInput {
    #[serde(default = "default_agentgrep_mode")]
    mode: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    terms: Option<Vec<String>>,
    #[serde(default)]
    regex: Option<bool>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(rename = "type", default)]
    file_type: Option<String>,
    #[serde(default)]
    hidden: Option<bool>,
    #[serde(default)]
    no_ignore: Option<bool>,
    #[serde(default)]
    max_files: Option<usize>,
    #[serde(default)]
    max_regions: Option<usize>,
    #[serde(default)]
    full_region: Option<String>,
    #[serde(default)]
    debug_plan: Option<bool>,
    #[serde(default)]
    debug_score: Option<bool>,
    #[serde(default)]
    paths_only: Option<bool>,
}

fn default_agentgrep_mode() -> String {
    "trace".to_string()
}

// ─── Agentgrep-compatible types (replaced with FFS-backed implementations) ───

#[derive(Debug, Clone, Serialize)]
pub struct GrepMatch {
    pub line_number: usize,
    pub line_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchGroup {
    pub kind: String,
    pub label: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub matches: Vec<GrepMatch>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileMatches {
    pub path: String,
    pub language: String,
    pub role: String,
    pub matches: Vec<GrepMatch>,
    pub groups: Vec<MatchGroup>,
    pub total_symbols: usize,
    pub matched_symbol_count: usize,
    pub other_symbols: Vec<serde_json::Value>,
    pub other_symbols_omitted_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct GrepResult {
    pub query: String,
    pub regex: bool,
    pub root: String,
    pub files: Vec<FileMatches>,
    pub total_files: usize,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindFile {
    pub path: String,
    pub role: String,
    pub language: String,
    pub score: i32,
    pub why: Vec<String>,
    pub structure: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindResult {
    pub query: String,
    pub root: String,
    pub files: Vec<FindFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlineResult {
    pub path: String,
    pub language: String,
    pub role: String,
    pub items: Vec<OutlineItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlineItem {
    pub kind: String,
    pub label: String,
    pub start_line: usize,
    pub end_line: usize,
    pub line_count: usize,
}

// ─── Smart/trace types ───

#[derive(Debug, Clone, Serialize)]
pub struct SmartSummary {
    pub total_files: usize,
    pub total_regions: usize,
    pub best_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartStructure {
    pub items: Vec<OutlineItem>,
    pub omitted_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartRegion {
    pub kind: String,
    pub label: String,
    pub start_line: usize,
    pub end_line: usize,
    pub line_count: usize,
    pub score: i32,
    pub body: String,
    pub full_region: bool,
    pub why: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_applied: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartFile {
    pub path: String,
    pub role: String,
    pub language: String,
    pub score: i32,
    pub why: Vec<String>,
    pub structure: SmartStructure,
    pub regions: Vec<SmartRegion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_applied: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartResult {
    pub query: SmartQuery,
    pub root: String,
    pub summary: SmartSummary,
    pub files: Vec<SmartFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_applied: Option<String>,
}

// ─── CLI argument types (struct shapes matching what render/args expect) ───

#[derive(Debug, Clone)]
pub struct GrepArgs {
    pub query: String,
    pub regex: bool,
    pub json: bool,
    pub paths_only: bool,
    pub file_type: Option<String>,
    pub hidden: bool,
    pub no_ignore: bool,
    pub path: Option<String>,
    pub glob: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FindArgs {
    pub query_parts: Vec<String>,
    pub file_type: Option<String>,
    pub json: bool,
    pub paths_only: bool,
    pub debug_score: bool,
    pub max_files: usize,
    pub hidden: bool,
    pub no_ignore: bool,
    pub path: Option<String>,
    pub glob: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OutlineArgs {
    pub path: String,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct SmartArgs {
    pub path: Option<String>,
    pub file_type: Option<String>,
    pub max_files: usize,
    pub max_regions: usize,
    pub full_region: Option<String>,
    pub debug_plan: bool,
    pub debug_score: bool,
    pub json: bool,
    pub paths_only: bool,
    pub hidden: bool,
    pub no_ignore: bool,
}

#[derive(Debug, Clone)]
pub enum FullRegionMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartQuery {
    pub subject: String,
    pub relation: Relation,
    pub support: Vec<String>,
    pub kind: Option<String>,
    pub path_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum Relation {
    Defined,
    CalledFrom,
    TriggeredFrom,
    Rendered,
    Populated,
    ComesFrom,
    Handled,
    Implementation,
    Custom(String),
}

impl Relation {
    pub fn parse(value: &str) -> Self {
        match value.to_lowercase().replace([' ', '-'], "_").as_str() {
            "defined" | "definition" => Self::Defined,
            "called_from" | "callers" => Self::CalledFrom,
            "triggered_from" => Self::TriggeredFrom,
            "rendered" | "render" => Self::Rendered,
            "populated" | "populate" => Self::Populated,
            "comes_from" | "source" => Self::ComesFrom,
            "handled" | "handler" => Self::Handled,
            "implementation" => Self::Implementation,
            other => Self::Custom(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Defined => "defined",
            Self::CalledFrom => "called_from",
            Self::TriggeredFrom => "triggered_from",
            Self::Rendered => "rendered",
            Self::Populated => "populated",
            Self::ComesFrom => "comes_from",
            Self::Handled => "handled",
            Self::Implementation => "implementation",
            Self::Custom(v) => v.as_str(),
        }
    }
}

#[derive(Debug)]
pub enum ParseError {
    MissingSubject,
    MissingRelation,
    InvalidTerm(String),
}

pub fn parse_smart_query<I, S>(terms: I) -> std::result::Result<SmartQuery, ParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut subject = None;
    let mut relation = None;
    let mut kind = None;
    let mut path_hint = None;
    let mut support = Vec::new();

    for term in terms {
        let t = term.as_ref().trim();
        if let Some(val) = t.strip_prefix("subject:") {
            subject = Some(val.trim().to_string());
        } else if let Some(val) = t.strip_prefix("relation:") {
            relation = Some(Relation::parse(val.trim()));
        } else if let Some(val) = t.strip_prefix("kind:") {
            kind = Some(val.trim().to_string());
        } else if let Some(val) = t.strip_prefix("path:") {
            path_hint = Some(val.trim().to_string());
        } else if let Some(val) = t.strip_prefix("support:") {
            support.push(val.trim().to_string());
        }
    }

    Ok(SmartQuery {
        subject: subject.ok_or(ParseError::MissingSubject)?,
        relation: relation.ok_or(ParseError::MissingRelation)?,
        support,
        kind,
        path_hint,
    })
}

// ─── Harness context types (unchanged) ───

#[derive(Debug, Serialize, Default)]
struct AgentGrepHarnessContext {
    version: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    known_regions: Vec<AgentGrepKnownRegion>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    known_files: Vec<AgentGrepKnownFile>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    known_symbols: Vec<AgentGrepKnownSymbol>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    focus_files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AgentGrepKnownRegion {
    path: String,
    start_line: usize,
    end_line: usize,
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
    reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AgentGrepKnownFile {
    path: String,
    structure_confidence: f32,
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
    reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AgentGrepKnownSymbol {
    path: String,
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<&'static str>,
    structure_confidence: f32,
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
    reasons: Vec<&'static str>,
}

// ─── Additional types for submodules ───

#[derive(Debug, Clone)]
pub(super) struct ToolExposureObservation {
    tool: ToolCall,
    content: String,
    timestamp: Option<DateTime<Utc>>,
    message_index: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ExposureDescriptor {
    timestamp: Option<DateTime<Utc>>,
    message_index: usize,
    total_messages: usize,
    compaction_cutoff: Option<usize>,
}

// ─── Tool implementation ───

pub struct AgentGrepTool;

impl AgentGrepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AgentGrepTool {
    fn name(&self) -> &str {
        "agentgrep"
    }

    fn description(&self) -> &str {
        "Relation-aware code trace search. Defaults to trace mode when mode is omitted."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "mode": {
                    "type": "string",
                    "enum": ["trace", "smart"],
                    "description": "Relation-aware code trace search. Uses FFS engine under the hood."
                },
                "query": {
                    "type": "string",
                    "description": "Search query for grep/find modes."
                },
                "terms": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "DSL terms for trace/smart mode: subject:X relation:Y kind:Z path:P support:W"
                },
                "file": {
                    "type": "string",
                    "description": "File path for outline mode."
                },
                "regex": {
                    "type": "boolean",
                    "description": "Treat query as regex."
                },
                "path": {
                    "type": "string",
                    "description": "Search root path or specific file."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob filter for files to search."
                },
                "type": {
                    "type": "string",
                    "description": "File type filter (e.g. rs, ts, py)."
                },
                "max_files": {
                    "type": "integer",
                    "description": "Maximum number of files to return."
                },
                "max_regions": {
                    "type": "integer",
                    "description": "Maximum number of matching regions to return."
                },
                "paths_only": {
                    "type": "boolean",
                    "description": "Return only matching paths instead of match excerpts where supported."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: AgentGrepInput = serde_json::from_value(input)?;
        let context_path = maybe_write_context_json(&params, &ctx)?;
        let request = summarize_agentgrep_request(&params, &ctx, context_path.as_deref());
        let started_at = std::time::Instant::now();
        let outcome = execute_linked_agentgrep(&params, &ctx, context_path.as_deref());
        let elapsed_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

        if let Some(path) = context_path {
            let _ = std::fs::remove_file(path);
        }

        match outcome {
            Ok(output) => {
                if elapsed_ms >= 2_000 {
                    logging::warn(&format!(
                        "agentgrep slow mode={} elapsed_ms={} request={}",
                        params.mode, elapsed_ms, request
                    ));
                }
                Ok(output)
            }
            Err(err) => {
                let detail = err.to_string();
                let detail = util::truncate_str(detail.trim(), 600);
                logging::warn(&format!(
                    "agentgrep failure mode={} elapsed_ms={} request={} error={}",
                    params.mode, elapsed_ms, request, detail
                ));
                Err(anyhow::anyhow!(
                    "agentgrep {} failed after {}ms: {}",
                    params.mode,
                    elapsed_ms,
                    err
                ))
            }
        }
    }
}

fn execute_linked_agentgrep(
    params: &AgentGrepInput,
    ctx: &ToolContext,
    _context_json_path: Option<&Path>,
) -> Result<ToolOutput> {
    match params.mode.as_str() {
        "trace" | "smart" => {
            let (smart_args, query) = build_smart_args_and_query(params, ctx, None)?;
            let root = resolve_search_root(ctx, smart_args.path.as_deref());
            let result = run_smart_ffs(&root, &query, &smart_args)?;
            Ok(ToolOutput::new(render_smart_output(&result, &smart_args))
                .with_title(format!("agentgrep {}", params.mode)))
        }
        _ => Err(anyhow::anyhow!(
            "agentgrep only supports trace/smart mode. For grep, find, or outline, use the grep, glob, or outline tools instead."
        )),
    }
}

// ─── Core: run_smart using FFS engine ───

/// Run a smart/trace search using FFS engine API.
pub fn run_smart_ffs(root: &Path, query: &SmartQuery, args: &SmartArgs) -> Result<SmartResult> {
    let subject = &query.subject;
    let relation = query.relation.as_str();
    let max_files = args.max_files.min(30);
    let max_regions = args.max_regions.min(20);

    // 1. Search for the subject using FFS find
    let find_opts = ffs_engine::api::FindOptions {
        max_files,
        score_threshold: 1,
    };
    let find_result = ffs_engine::api::find(root, subject, &find_opts);

    // 2. Search for the subject in content using FFS grep
    let grep_opts = ffs_engine::api::GrepOptions {
        regex: false,
        case_sensitive: false,
        max_matches: max_regions * 5,
        max_files,
    };
    let grep_result = ffs_engine::api::grep(root, subject, &grep_opts);

    // 3. Build SmartResult from combined data
    let mut smart_files: Vec<SmartFile> = Vec::new();

    // Add files from grep results (they have content matches)
    for gf in grep_result.files.iter().take(max_files) {
        let mut regions: Vec<SmartRegion> = Vec::new();
        for group in gf.groups.iter() {
            for m in group.matches.iter().take(max_regions / 2) {
                regions.push(SmartRegion {
                    kind: group.kind.clone(),
                    label: format!("{} {}", group.kind, group.name),
                    start_line: m.line as usize,
                    end_line: m.line as usize + 1,
                    line_count: 1,
                    score: 50,
                    body: m.text.clone(),
                    full_region: false,
                    why: vec![format!("matched subject: {}", subject)],
                    context_applied: None,
                });
            }
        }
        if !regions.is_empty() {
            let role = ffs_search::role::detect_role(Path::new(&gf.path));
            let outline = ffs_engine::api::outline(Path::new(&gf.path));
            smart_files.push(SmartFile {
                path: gf.path.clone(),
                role: role.as_str().to_string(),
                language: gf.language.clone(),
                score: 50,
                why: vec![format!("content match for: {}", subject)],
                context_applied: None,
                structure: SmartStructure {
                    items: outline
                        .as_ref()
                        .map(|o| {
                            o.entries
                                .iter()
                                .map(|e| OutlineItem {
                                    kind: format!("{:?}", e.kind),
                                    label: e.name.clone(),
                                    start_line: e.start_line as usize,
                                    end_line: e.end_line as usize,
                                    line_count: (e.end_line - e.start_line + 1) as usize,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    omitted_count: 0,
                },
                regions,
            });
        }
    }

    // Add files from find results (path matches, if not already in grep results)
    let existing_paths: std::collections::HashSet<String> =
        smart_files.iter().map(|f| f.path.clone()).collect();
    for ff in find_result.files.iter() {
        if existing_paths.contains(&ff.path) {
            continue;
        }
        if smart_files.len() >= max_files {
            break;
        }
        let outline = ffs_engine::api::outline(Path::new(&ff.path));
        let role = ffs_search::role::detect_role(Path::new(&ff.path));
        // Grep for the subject in this file too
        let file_grep = ffs_engine::api::grep(
            root,
            subject,
            &ffs_engine::api::GrepOptions {
                regex: false,
                case_sensitive: false,
                max_matches: 10,
                max_files: 1,
            },
        );
        // Narrow grep results to just this file
        let mut regions: Vec<SmartRegion> = Vec::new();
        for gf in file_grep.files.iter() {
            if gf.path == ff.path {
                for group in gf.groups.iter() {
                    for m in group.matches.iter().take(max_regions / 2) {
                        regions.push(SmartRegion {
                            kind: group.kind.clone(),
                            label: format!("{} {}", group.kind, group.name),
                            start_line: m.line as usize,
                            end_line: m.line as usize + 1,
                            line_count: 1,
                            score: 50,
                            body: m.text.clone(),
                            full_region: false,
                            why: vec![format!("matched subject: {}", subject)],
                            context_applied: None,
                        });
                    }
                }
            }
        }

        smart_files.push(SmartFile {
            path: ff.path.clone(),
            role: role.as_str().to_string(),
            language: outline
                .as_ref()
                .map(|o| o.language.clone())
                .unwrap_or_default(),
            score: ff.score,
            why: vec![format!("path matched: {}", subject)],
            context_applied: None,
            structure: SmartStructure {
                items: outline
                    .as_ref()
                    .map(|o| {
                        o.entries
                            .iter()
                            .map(|e| OutlineItem {
                                kind: format!("{:?}", e.kind),
                                label: e.name.clone(),
                                start_line: e.start_line as usize,
                                end_line: e.end_line as usize,
                                line_count: (e.end_line - e.start_line + 1) as usize,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                omitted_count: 0,
            },
            regions,
        });
    }

    // Sort by score descending
    smart_files.sort_by(|a, b| b.score.cmp(&a.score));

    let total_regions: usize = smart_files.iter().map(|f| f.regions.len()).sum();
    let best_file = smart_files.first().map(|f| f.path.clone());
    let best_region = smart_files
        .first()
        .and_then(|f| f.regions.first())
        .map(|r| r.label.clone());

    Ok(SmartResult {
        query: query.clone(),
        root: root.to_string_lossy().to_string(),
        summary: SmartSummary {
            total_files: smart_files.len(),
            total_regions,
            best_file,
        },
        files: smart_files,
        context_applied: None,
    })
}

fn resolve_path_arg(ctx: &ToolContext, path: &str) -> PathBuf {
    ctx.resolve_path(Path::new(path))
}

fn normalized_agentgrep_glob(glob: Option<&str>) -> Option<&str> {
    let glob = glob?.trim();
    if glob.is_empty() || is_match_all_glob(glob) {
        return None;
    }
    Some(glob)
}

fn normalized_agentgrep_glob_owned(glob: Option<&str>) -> Option<String> {
    normalized_agentgrep_glob(glob).map(ToOwned::to_owned)
}

fn is_match_all_glob(glob: &str) -> bool {
    matches!(glob, "*" | "**" | "**/*" | "./*" | "./**" | "./**/*")
}

#[cfg(test)]
#[path = "agentgrep_tests.rs"]
mod tests;
