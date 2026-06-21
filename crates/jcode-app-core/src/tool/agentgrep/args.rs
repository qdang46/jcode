use super::*;

struct ResolvedSearchScope {
    root: Option<String>,
    glob: Option<String>,
}

fn resolved_search_scope(
    ctx: &ToolContext,
    path: Option<&str>,
    glob: Option<&str>,
) -> ResolvedSearchScope {
    let Some(path) = path else {
        return ResolvedSearchScope {
            root: None,
            glob: normalized_agentgrep_glob_owned(glob),
        };
    };
    let resolved = resolve_path_arg(ctx, path);
    if resolved.is_file() {
        let root = resolved
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .display()
            .to_string();
        let glob = resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        return ResolvedSearchScope {
            root: Some(root),
            glob,
        };
    }
    ResolvedSearchScope {
        root: Some(resolved.display().to_string()),
        glob: normalized_agentgrep_glob_owned(glob),
    }
}

pub(super) fn build_smart_args_and_query(
    params: &AgentGrepInput,
    ctx: &ToolContext,
    _context_json: Option<&Path>,
) -> Result<(SmartArgs, SmartQuery)> {
    let scope = resolved_search_scope(ctx, params.path.as_deref(), params.glob.as_deref());
    let terms = params.terms.clone().unwrap_or_default();
    let query = parse_smart_query(&terms).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let args = SmartArgs {
        path: scope.root,
        file_type: params.file_type.clone(),
        max_files: params.max_files.unwrap_or(20),
        max_regions: params.max_regions.unwrap_or(15),
        full_region: params.full_region.clone(),
        debug_plan: params.debug_plan.unwrap_or(false),
        debug_score: params.debug_score.unwrap_or(false),
        json: false,
        paths_only: false,
        hidden: params.hidden.unwrap_or(false),
        no_ignore: params.no_ignore.unwrap_or(false),
    };
    Ok((args, query))
}

pub(super) fn resolve_search_root(ctx: &ToolContext, path: Option<&str>) -> PathBuf {
    let Some(path) = path else {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    };
    resolve_path_arg(ctx, path)
}

pub(super) fn summarize_agentgrep_request(
    params: &AgentGrepInput,
    _ctx: &ToolContext,
    _context_path: Option<&Path>,
) -> String {
    format!("mode={} query={:?}", params.mode, params.query)
}

#[cfg(test)]
pub(super) fn trace_or_smart_terms_owned(params: &AgentGrepInput) -> Result<Vec<String>> {
    Ok(params.terms.clone().unwrap_or_default())
}
