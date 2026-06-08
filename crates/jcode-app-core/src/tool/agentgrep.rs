use super::{Tool, ToolContext, ToolOutput};
use crate::message::{ContentBlock, ToolCall};
use crate::session::Session;
use crate::storage;
use crate::{logging, util};
use ::agentgrep::cli::{FindArgs, FullRegionMode, GrepArgs, OutlineArgs, SmartArgs};
use ::agentgrep::find::{FindFile, FindResult};
use ::agentgrep::outline::OutlineResult;
use ::agentgrep::search::{FileMatches, GrepResult};
use ::agentgrep::smart_dsl::{Relation, SmartQuery, parse_smart_query};
use ::agentgrep::smart_engine::{SmartFile, SmartRegion, SmartResult, run_smart};
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
use self::args::{
    build_smart_args_and_query, resolve_search_root, summarize_agentgrep_request,
};
use self::context::maybe_write_context_json;
#[cfg(test)]
use self::context::{
    collect_bash_exposure, collect_trace_exposure, tune_known_file, tune_known_region,
};
use self::render::render_smart_output;

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

#[derive(Debug, Clone, Copy)]
struct RegionConfidenceProfile {
    body_confidence: f32,
    current_version_confidence: f32,
    prune_confidence: f32,
    source_strength: &'static str,
}

#[derive(Debug, Clone)]
struct PendingTraceRegion {
    path: String,
    kind: Option<&'static str>,
    start_line: usize,
    end_line: usize,
}

#[derive(Debug, Clone)]
struct ToolExposureObservation {
    tool: ToolCall,
    content: String,
    timestamp: Option<DateTime<Utc>>,
    message_index: usize,
}

#[derive(Debug, Clone, Copy)]
struct ExposureDescriptor {
    timestamp: Option<DateTime<Utc>>,
    message_index: usize,
    total_messages: usize,
    compaction_cutoff: Option<usize>,
}

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
                    "description": "Relation-aware code trace search. Use to find how symbols connect — for example, what renders auth_status, what calls a function, etc. For normal code search or file finding, use grep or glob instead."
                },
                "query": {
                    "type": "string",
                    "description": "Optional query string. In smart mode, query can be used as fallback for terms when terms is not set (for example, 'subject:auth_status relation:rendered')."
                },
                "terms": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Trace DSL terms, for example [\"subject:auth_status\", \"relation:rendered\", \"support:ui\"]. Required for trace mode. In smart mode, query may be used instead."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search, relative to the workspace unless absolute. If this is a file, agentgrep searches only that file. Omit to search the workspace."
                },
                "glob": {
                    "type": "string",
                    "description": "Optional file glob filter such as **/*.rs. Do not set glob to **/* just to search everything; omit it instead."
                },
                "type": {
                    "type": "string",
                    "description": "Optional ripgrep file type filter, such as rs, py, js, ts, or md."
                },
                "max_files": {
                    "type": "integer",
                    "description": "Maximum number of files to return for find/trace-style modes."
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
    context_json_path: Option<&Path>,
) -> Result<ToolOutput> {
    let exact_file = exact_search_file_path(ctx, params.path.as_deref());
    match params.mode.as_str() {
        "trace" | "smart" => {
            let (args, query) = build_smart_args_and_query(params, ctx, context_json_path)?;
            let root = resolve_search_root(ctx, args.path.as_deref());
            let result = filter_smart_result_to_exact_file(
                run_smart(&root, &query, &args).map_err(anyhow::Error::msg)?,
                exact_file.as_deref(),
            );
            Ok(ToolOutput::new(render_smart_output(&result, &args))
                .with_title(format!("agentgrep {}", params.mode)))
        }
        _ => Err(anyhow::anyhow!(
            "agentgrep only supports trace/smart mode. For grep, find, or outline, use the grep, glob, or outline tools instead."
        )),
    }
}

fn resolve_path_arg(ctx: &ToolContext, path: &str) -> PathBuf {
    ctx.resolve_path(Path::new(path))
}

fn exact_search_file_path(ctx: &ToolContext, path: Option<&str>) -> Option<String> {
    let path = path?;
    let resolved = resolve_path_arg(ctx, path);
    if !resolved.is_file() {
        return None;
    }
    resolved
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

#[allow(dead_code)]
fn filter_grep_result_to_exact_file(
    mut result: GrepResult,
    exact_file: Option<&str>,
) -> GrepResult {
    let Some(exact_file) = exact_file else {
        return result;
    };

    result.files.retain(|file| file.path == exact_file);
    result.total_files = result.files.len();
    result.total_matches = result.files.iter().map(|file| file.matches.len()).sum();
    result
}

#[allow(dead_code)]
fn filter_find_result_to_exact_file(
    mut result: FindResult,
    exact_file: Option<&str>,
) -> FindResult {
    let Some(exact_file) = exact_file else {
        return result;
    };

    result.files.retain(|file| file.path == exact_file);
    result
}

fn filter_smart_result_to_exact_file(
    mut result: SmartResult,
    exact_file: Option<&str>,
) -> SmartResult {
    let Some(exact_file) = exact_file else {
        return result;
    };

    result.files.retain(|file| file.path == exact_file);
    result.summary.total_files = result.files.len();
    result.summary.total_regions = result.files.iter().map(|file| file.regions.len()).sum();
    result.summary.best_file = result.files.first().map(|file| file.path.clone());
    result
}

fn normalized_agentgrep_glob(glob: Option<&str>) -> Option<&str> {
    let glob = glob?.trim();
    if glob.is_empty() {
        return None;
    }

    if is_match_all_glob(glob) {
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
