use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use ignore::WalkBuilder;

#[derive(Clone, Debug)]
pub struct GrepHit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

static RG_OK: OnceLock<bool> = OnceLock::new();

pub fn rg_available() -> bool {
    *RG_OK.get_or_init(|| {
        Command::new("rg")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

pub fn grep_ripgrep(base: &Path, pattern: &str, limit: usize) -> Result<Vec<GrepHit>> {
    let output = Command::new("rg")
        .args([
            "--line-number",
            "--no-heading",
            "--color",
            "never",
            "--max-count",
            &limit.to_string(),
        ])
        .arg(pattern)
        .arg(base)
        .output()
        .context("failed to spawn rg")?;

    if !output.status.success() && output.status.code() != Some(1) {
        anyhow::bail!(
            "rg failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let mut hits = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if hits.len() >= limit {
            break;
        }
        let Some((rest, text)) = line.split_once(':') else {
            continue;
        };
        let Some((path, line_no)) = rest.rsplit_once(':') else {
            continue;
        };
        let Ok(line_num) = line_no.parse::<usize>() else {
            continue;
        };
        hits.push(GrepHit {
            path: path.to_string(),
            line: line_num,
            text: text.to_string(),
        });
    }
    Ok(hits)
}

pub fn glob_ripgrep(base: &Path, pattern: &str, limit: usize) -> Result<Vec<String>> {
    let output = Command::new("rg")
        .args(["--files", "-g", pattern])
        .arg(base)
        .output()
        .context("failed to spawn rg --files")?;

    if !output.status.success() {
        anyhow::bail!(
            "rg --files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .take(limit)
        .map(str::to_string)
        .collect())
}

/// Walk + substring match when neither ffs nor rg is usable.
pub fn grep_walkdir(base: &Path, needle: &str, limit: usize) -> Result<Vec<GrepHit>> {
    let needle_lower = needle.to_lowercase();
    let mut hits = Vec::new();
    for entry in WalkBuilder::new(base).standard_filters(true).build() {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.into_path();
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let path_str = path.display().to_string();
        for (idx, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&needle_lower) {
                hits.push(GrepHit {
                    path: path_str.clone(),
                    line: idx + 1,
                    text: line.to_string(),
                });
                if hits.len() >= limit {
                    return Ok(hits);
                }
            }
        }
    }
    Ok(hits)
}

pub fn find_fuzzy_walkdir(base: &Path, query: &str, limit: usize) -> Result<Vec<String>> {
    let q = query.to_lowercase();
    let mut scored: Vec<(usize, String)> = Vec::new();
    for entry in WalkBuilder::new(base).standard_filters(true).build() {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.into_path();
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .display()
            .to_string()
            .replace('\\', "/");
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        let rel_lower = rel.to_lowercase();
        let score = if name.contains(&q) {
            100
        } else if rel_lower.contains(&q) {
            50
        } else {
            0
        };
        if score > 0 {
            scored.push((score, rel));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
    Ok(scored.into_iter().take(limit).map(|(_, p)| p).collect())
}

pub fn format_grep_hits(hits: &[GrepHit], label: &str) -> String {
    let mut out = format!("Found {} matches ({label})\n\n", hits.len());
    let mut current = String::new();
    for h in hits {
        if h.path != current {
            if !current.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("{}:\n", h.path));
            current = h.path.clone();
        }
        out.push_str(&format!("  {:>4}: {}\n", h.line, h.text));
    }
    out
}

pub fn glob_crate(base: &Path, pattern: &str, limit: usize) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in WalkBuilder::new(base).standard_filters(true).build() {
        let entry = entry?;
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.into_path();
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .display()
            .to_string()
            .replace('\\', "/");
        if glob::Pattern::new(pattern)
            .map(|p| p.matches(&rel))
            .unwrap_or(false)
        {
            out.push(rel);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}
