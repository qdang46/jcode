//! Engine-backed ffs tools: find, dispatch, callers, callees, refs, flow.
//! All use `ffs-engine` / `ffs-search` crate APIs — no CLI subprocess.

use super::ffs_support::{
    self, DEFAULT_ENGINE_TOKEN_BUDGET, collect_definitions, collect_usages, engine_holder,
    find_callee_sites, find_call_sites, format_dispatch, format_flow_card, format_refs,
    format_call_hits,
};
use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use ffs_engine::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn workspace_root(ctx: &ToolContext, path: Option<&str>) -> PathBuf {
    ffs_support::workspace_root(
        ctx.working_dir.as_ref(),
        |p| ctx.resolve_path(p),
        path.map(Path::new),
    )
}

fn engine_for(ctx: &ToolContext, path: Option<&str>, budget: u64) -> (PathBuf, Arc<Engine>) {
    let root = workspace_root(ctx, path);
    let engine = engine_holder(&root, budget);
    (root, engine)
}

// ── find ────────────────────────────────────────────────────────────────────

pub struct FfsFindTool;

impl FfsFindTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct FindInput {
    needle: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait]
impl Tool for FfsFindTool {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> &str {
        "Fuzzy-find files by path substring. Uses ffs-search FilePicker when available; falls back to walkdir substring match."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["needle"],
            "properties": {
                "intent": super::intent_schema_property(),
                "needle": { "type": "string", "description": "Path substring or fuzzy needle." },
                "path": { "type": "string", "description": "Workspace root (default: cwd)." },
                "limit": { "type": "integer", "description": "Max results (default 50)." }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: FindInput = serde_json::from_value(input)?;
        let limit = params.limit.unwrap_or(50);
        let base = workspace_root(ctx, params.path.as_deref());

        let (matches, label) = tokio::task::spawn_blocking(move || {
            if ffs_support::ffs_preferred() {
                match ffs_support::find_files(&base, &params.needle, limit) {
                    Ok(m) if !m.is_empty() => return (m, "ffs-search"),
                    Ok(m) => return (m, "ffs-search"),
                    Err(_) => {}
                }
            }
            let m = ffs_support::find_fuzzy_walkdir(&base, &params.needle, limit)
                .unwrap_or_default();
            (m, "walkdir-fallback")
        })
        .await?;

        let mut out = format!(
            "Find '{}' in {}: {} matches ({label})\n\n",
            params.needle,
            base.display(),
            matches.len()
        );
        for m in &matches {
            out.push_str(m);
            out.push('\n');
        }
        Ok(ToolOutput::new(out))
    }
}

// ── dispatch ────────────────────────────────────────────────────────────────

pub struct FfsDispatchTool;

impl FfsDispatchTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct DispatchInput {
    query: String,
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl Tool for FfsDispatchTool {
    fn name(&self) -> &str {
        "dispatch"
    }

    fn description(&self) -> &str {
        "Auto-classify a free-form query into file-path / glob / symbol / concept routing via ffs-engine."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "intent": super::intent_schema_property(),
                "query": { "type": "string" },
                "path": { "type": "string" }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: DispatchInput = serde_json::from_value(input)?;
        let (root, engine) = engine_for(&ctx, params.path.as_deref(), DEFAULT_ENGINE_TOKEN_BUDGET);
        let query = params.query.clone();

        let out = tokio::task::spawn_blocking(move || {
            if !ffs_support::ffs_preferred() {
                return format!(
                    "[ffs disabled] query '{}' — use grep/glob/symbol tools directly\n",
                    query
                );
            }
            let result = engine.dispatch(&query, &root);
            format_dispatch(&result)
        })
        .await?;

        Ok(ToolOutput::new(out))
    }
}

// ── callers / callees ───────────────────────────────────────────────────────

pub struct FfsCallersTool;

impl FfsCallersTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct CallGraphInput {
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait]
impl Tool for FfsCallersTool {
    fn name(&self) -> &str {
        "callers"
    }

    fn description(&self) -> &str {
        "Find call sites that reference a symbol (single-hop callers)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": { "type": "string" },
                "path": { "type": "string" },
                "limit": { "type": "integer", "description": "Max hits (default 100)." }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: CallGraphInput = serde_json::from_value(input)?;
        let limit = params.limit.unwrap_or(100);
        let (root, engine) = engine_for(&ctx, params.path.as_deref(), DEFAULT_ENGINE_TOKEN_BUDGET);
        let name = params.name.clone();

        let out = tokio::task::spawn_blocking(move || {
            let hits = find_call_sites(&engine, &root, &name, limit);
            let mut s = format!("Callers of '{name}':\n");
            s.push_str(&format_call_hits(&hits, "callers"));
            s
        })
        .await?;

        Ok(ToolOutput::new(out))
    }
}

pub struct FfsCalleesTool;

impl FfsCalleesTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FfsCalleesTool {
    fn name(&self) -> &str {
        "callees"
    }

    fn description(&self) -> &str {
        "Find symbols referenced inside a symbol's definition body."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": { "type": "string" },
                "path": { "type": "string" },
                "limit": { "type": "integer", "description": "Max hits (default 100)." }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: CallGraphInput = serde_json::from_value(input)?;
        let limit = params.limit.unwrap_or(100);
        let (root, engine) = engine_for(&ctx, params.path.as_deref(), DEFAULT_ENGINE_TOKEN_BUDGET);
        let name = params.name.clone();

        let out = tokio::task::spawn_blocking(move || {
            let hits = find_callee_sites(&engine, &root, &name, limit);
            let mut s = format!("Callees of '{name}':\n");
            s.push_str(&format_call_hits(&hits, "callees"));
            s
        })
        .await?;

        Ok(ToolOutput::new(out))
    }
}

// ── refs ────────────────────────────────────────────────────────────────────

pub struct FfsRefsTool;

impl FfsRefsTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct RefsInput {
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

#[async_trait]
impl Tool for FfsRefsTool {
    fn name(&self) -> &str {
        "refs"
    }

    fn description(&self) -> &str {
        "Find symbol definitions plus single-hop usages in one shot."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": { "type": "string" },
                "path": { "type": "string" },
                "limit": { "type": "integer", "description": "Max usages (default 100)." },
                "offset": { "type": "integer", "description": "Usage pagination offset (default 0)." }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: RefsInput = serde_json::from_value(input)?;
        let limit = params.limit.unwrap_or(100);
        let offset = params.offset.unwrap_or(0);
        let (root, engine) = engine_for(&ctx, params.path.as_deref(), DEFAULT_ENGINE_TOKEN_BUDGET);
        let name = params.name.clone();

        let out = tokio::task::spawn_blocking(move || {
            let definitions = collect_definitions(&engine, &name);
            let usages_all = collect_usages(&engine, &name, &root);
            let total = usages_all.len();
            let page: Vec<_> = usages_all.into_iter().skip(offset).take(limit).collect();
            let has_more = offset + page.len() < total;
            format_refs(&name, &definitions, &page, total, offset, has_more)
        })
        .await?;

        Ok(ToolOutput::new(out))
    }
}

// ── flow ────────────────────────────────────────────────────────────────────

pub struct FfsFlowTool;

impl FfsFlowTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct FlowInput {
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    callees_top: Option<usize>,
    #[serde(default)]
    callers_top: Option<usize>,
}

#[async_trait]
impl Tool for FfsFlowTool {
    fn name(&self) -> &str {
        "flow"
    }

    fn description(&self) -> &str {
        "Drill-down card per definition: body excerpt + top callees + top callers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "intent": super::intent_schema_property(),
                "name": { "type": "string" },
                "path": { "type": "string" },
                "limit": { "type": "integer", "description": "Max cards (default 10)." },
                "offset": { "type": "integer" },
                "callees_top": { "type": "integer", "description": "Callees per card (default 5)." },
                "callers_top": { "type": "integer", "description": "Callers per card (default 5)." }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: FlowInput = serde_json::from_value(input)?;
        let limit = params.limit.unwrap_or(10);
        let offset = params.offset.unwrap_or(0);
        let callees_top = params.callees_top.unwrap_or(5);
        let callers_top = params.callers_top.unwrap_or(5);
        let (root, engine) = engine_for(&ctx, params.path.as_deref(), DEFAULT_ENGINE_TOKEN_BUDGET);
        let name = params.name.clone();

        let out = tokio::task::spawn_blocking(move || {
            let defs = collect_definitions(&engine, &name);
            if defs.is_empty() {
                return format!("[no definitions found for '{name}']\n");
            }
            let total = defs.len();
            let page: Vec<_> = defs.into_iter().skip(offset).take(limit).collect();
            let mut out = format!("Flow for '{name}' ({} defs, showing {})\n\n", total, page.len());
            for def in &page {
                let path = PathBuf::from(&def.path);
                let body = engine.read(&path).body;
                let callees = find_callee_sites(&engine, &root, &name, callees_top);
                let callers = find_call_sites(&engine, &root, &name, callers_top);
                out.push_str(&format_flow_card(def, &body, &callees, &callers));
                out.push('\n');
            }
            if offset + page.len() < total {
                out.push_str(&format!(
                    "... {} more cards (offset {})\n",
                    total - offset - page.len(),
                    offset
                ));
            }
            out
        })
        .await?;

        Ok(ToolOutput::new(out))
    }
}
