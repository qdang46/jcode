//! Call-graph helpers ported from `ffs-mcp/src/engine_tools.rs` (crate API only).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ffs_engine::dispatch::DispatchResult;
use ffs_engine::{Engine, PreFilterStack};
use ffs_symbol::lang::detect_file_type;
use ffs_symbol::symbol_index::SymbolLocation;
use ffs_symbol::types::{FileType, OutlineEntry};
use ignore::WalkBuilder;

#[derive(Clone, Debug)]
pub struct CallHit {
    pub path: String,
    pub line: u32,
    pub text: String,
}

pub fn find_call_sites(engine: &Engine, root: &Path, symbol: &str, limit: usize) -> Vec<CallHit> {
    let definitions = engine.handles.symbols.lookup_exact(symbol);
    let definition_lines: Vec<(PathBuf, u32)> = definitions
        .iter()
        .map(|d| (d.path.clone(), d.line))
        .collect();

    let stack = PreFilterStack::new(engine.handles.bloom.clone());
    let mut candidates: Vec<(PathBuf, SystemTime, String)> = Vec::new();
    for entry in WalkBuilder::new(root)
        .standard_filters(true)
        .follow_links(false)
        .build()
        .flatten()
    {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.into_path();
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        candidates.push((path, mtime, content));
    }

    let survivors = stack.confirm_symbol(
        &candidates
            .iter()
            .map(|(p, m, c)| (p.clone(), *m, c.clone()))
            .collect::<Vec<_>>(),
        symbol,
    );
    let survivor_set: HashSet<PathBuf> = survivors.into_iter().map(|s| s.path).collect();

    let mut hits = Vec::new();
    for (path, _mtime, content) in &candidates {
        if !survivor_set.contains(path) {
            continue;
        }
        let path_str = path.display().to_string();
        for (lineno, line) in content.lines().enumerate() {
            let lineno = (lineno + 1) as u32;
            if !line.contains(symbol) {
                continue;
            }
            if definition_lines
                .iter()
                .any(|(p, l)| p == path && *l == lineno)
            {
                continue;
            }
            hits.push(CallHit {
                path: path_str.clone(),
                line: lineno,
                text: line.to_string(),
            });
            if hits.len() >= limit {
                return hits;
            }
        }
    }
    hits
}

pub fn find_callee_sites(
    engine: &Engine,
    _root: &Path,
    symbol: &str,
    limit: usize,
) -> Vec<CallHit> {
    let definitions = engine.handles.symbols.lookup_exact(symbol);
    if definitions.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for def in definitions {
        let Ok(content) = std::fs::read_to_string(&def.path) else {
            continue;
        };
        let path_str = def.path.display().to_string();
        for (idx, line) in content.lines().enumerate() {
            let lineno = (idx + 1) as u32;
            if lineno < def.line || lineno > def.end_line {
                continue;
            }
            for tok in line.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if tok.is_empty() || tok == symbol {
                    continue;
                }
                let candidates = engine.handles.symbols.lookup_exact(tok);
                if candidates.is_empty() {
                    continue;
                }
                hits.push(CallHit {
                    path: path_str.clone(),
                    line: lineno,
                    text: format!("{tok} ({})", candidates[0].kind),
                });
                if hits.len() >= limit {
                    return hits;
                }
            }
        }
    }
    hits
}

pub fn format_symbol_hits(hits: &[SymbolLocation], name: &str) -> String {
    if hits.is_empty() {
        return format!("[no definitions found for '{name}']\n");
    }
    let mut out = String::new();
    for hit in hits {
        out.push_str(&format!(
            "{}:{}: [{}] (weight {})\n",
            hit.path.display(),
            hit.line,
            hit.kind,
            hit.weight,
        ));
    }
    out
}

pub fn format_call_hits(hits: &[CallHit], header: &str) -> String {
    if hits.is_empty() {
        return format!("[no {header} found]\n");
    }
    let mut out = String::new();
    for h in hits {
        out.push_str(&format!("{}:{}: {}\n", h.path, h.line, h.text));
    }
    out
}

pub fn format_dispatch(result: &DispatchResult) -> String {
    match result {
        DispatchResult::Symbol { hits, classified } => {
            let mut out = format!("[symbol] '{}' -> {} hits\n", classified.raw, hits.len());
            for h in hits.iter().take(50) {
                out.push_str(&format!("{}:{}: [{}]\n", h.path.display(), h.line, h.kind));
            }
            out
        }
        DispatchResult::SymbolGlob { hits, classified } => {
            let mut out = format!(
                "[symbol-glob] '{}' -> {} hits\n",
                classified.raw,
                hits.len()
            );
            for (name, h) in hits.iter().take(50) {
                out.push_str(&format!("{name}\t{}:{}\n", h.path.display(), h.line));
            }
            out
        }
        DispatchResult::Glob {
            classified,
            pattern,
        } => format!(
            "[glob] '{}' (pattern={pattern}) — use glob for full results\n",
            classified.raw,
        ),
        DispatchResult::FilePath { classified, path } => {
            format!("[file-path] '{}' -> {}\n", classified.raw, path.display())
        }
        DispatchResult::ContentFallback { classified } => format!(
            "[concept] '{}' — fall back to grep for content search\n",
            classified.raw,
        ),
    }
}

pub fn enclosing_symbol(entries: &[OutlineEntry], line: u32) -> Option<String> {
    fn walk(entries: &[OutlineEntry], line: u32) -> Option<&OutlineEntry> {
        for e in entries {
            if line < e.start_line || line > e.end_line {
                continue;
            }
            if let Some(child) = walk(&e.children, line) {
                return Some(child);
            }
            return Some(e);
        }
        None
    }
    walk(entries, line).map(|e| e.name.clone())
}

struct CandidateFile {
    path: PathBuf,
    mtime: SystemTime,
    content: String,
    lang: Option<ffs_symbol::types::Lang>,
}

fn walk_code_files(root: &Path) -> Vec<CandidateFile> {
    let mut out = Vec::new();
    for entry in WalkBuilder::new(root)
        .standard_filters(true)
        .follow_links(false)
        .build()
        .flatten()
    {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.into_path();
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lang = match detect_file_type(&path) {
            FileType::Code(l) => Some(l),
            _ => None,
        };
        out.push(CandidateFile {
            path,
            mtime,
            content,
            lang,
        });
    }
    out
}

pub struct RefDefinition {
    pub path: String,
    pub line: u32,
    pub end_line: u32,
    pub kind: String,
    pub weight: u16,
}

pub struct RefUsage {
    pub path: String,
    pub line: u32,
    pub text: String,
    pub enclosing: Option<String>,
}

pub fn collect_definitions(engine: &Engine, name: &str) -> Vec<RefDefinition> {
    engine
        .handles
        .symbols
        .lookup_exact(name)
        .into_iter()
        .map(|loc| RefDefinition {
            path: loc.path.to_string_lossy().to_string(),
            line: loc.line,
            end_line: loc.end_line,
            kind: loc.kind,
            weight: loc.weight,
        })
        .collect()
}

pub fn collect_usages(engine: &Engine, name: &str, root: &Path) -> Vec<RefUsage> {
    let candidates = walk_code_files(root);
    let definitions = engine.handles.symbols.lookup_exact(name);
    let definition_paths: Vec<String> = definitions
        .iter()
        .map(|d| d.path.to_string_lossy().to_string())
        .collect();

    let stack = PreFilterStack::new(engine.handles.bloom.clone());
    let confirm_input: Vec<(PathBuf, SystemTime, String)> = candidates
        .iter()
        .map(|c| (c.path.clone(), c.mtime, c.content.clone()))
        .collect();
    let survivors = stack.confirm_symbol(&confirm_input, name);
    let survivor_set: HashSet<&Path> = survivors.iter().map(|s| s.path.as_path()).collect();

    let mut usages = Vec::new();
    for cf in &candidates {
        if !survivor_set.contains(cf.path.as_path()) {
            continue;
        }
        let path_str = cf.path.to_string_lossy().to_string();
        let in_defn_file = definition_paths.contains(&path_str);
        let definition_lines: Vec<u32> = if in_defn_file {
            definitions
                .iter()
                .filter(|d| d.path.to_string_lossy() == path_str)
                .map(|d| d.line)
                .collect()
        } else {
            Vec::new()
        };

        let outline = if let Some(lang) = cf.lang {
            engine
                .handles
                .outlines
                .get_or_compute(&cf.path, cf.mtime, &cf.content, lang)
        } else {
            Vec::new()
        };

        for (lineno, line) in cf.content.lines().enumerate() {
            let lineno = (lineno + 1) as u32;
            if !line.contains(name) {
                continue;
            }
            if definition_lines.contains(&lineno) {
                continue;
            }
            usages.push(RefUsage {
                path: path_str.clone(),
                line: lineno,
                text: line.to_string(),
                enclosing: enclosing_symbol(&outline, lineno),
            });
        }
    }
    usages
}

pub fn format_refs(
    name: &str,
    definitions: &[RefDefinition],
    usages: &[RefUsage],
    total_usages: usize,
    offset: usize,
    has_more: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("Definitions ({}):\n", definitions.len()));
    if definitions.is_empty() {
        out.push_str("  [none]\n");
    } else {
        for d in definitions {
            out.push_str(&format!(
                "  {}:{} ({}, w={})\n",
                d.path, d.line, d.kind, d.weight
            ));
        }
    }

    out.push_str(&format!("\nUsages ({total_usages}):\n"));
    if total_usages == 0 {
        out.push_str("  [none]\n");
    } else {
        for u in usages {
            let encl = u.enclosing.as_deref().unwrap_or("?");
            out.push_str(&format!(
                "  {}:{} (in {}): {}\n",
                u.path, u.line, encl, u.text
            ));
        }
        if has_more {
            out.push_str(&format!(
                "\n... showing {} of {} (offset {offset})\n",
                usages.len(),
                total_usages
            ));
        }
    }

    if definitions.is_empty() && total_usages == 0 {
        out.push_str("\n[no references found]\n");
    }
    out
}

pub fn format_flow_card(
    def: &RefDefinition,
    body: &str,
    callees: &[CallHit],
    callers: &[CallHit],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "--- {} @ {}:{} ({}) ---\n",
        def.kind, def.path, def.line, def.weight
    ));
    if !body.is_empty() {
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str("\nCallees:\n");
    out.push_str(&format_call_hits(callees, "callees"));
    out.push_str("Callers:\n");
    out.push_str(&format_call_hits(callers, "callers"));
    out
}
