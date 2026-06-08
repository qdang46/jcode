//! Doctor check modules. Each `check_*` pushes `Finding`s for its category.

pub mod auth;
pub mod build_platform;
pub mod config;
pub mod mcp;
pub mod resource;
pub mod sessions;
pub mod shell;
pub mod storage;
pub mod swarm;

use std::path::{Path, PathBuf};

/// Read a boolean-ish env var (`1`/`true`/`yes`/`on`).
pub(crate) fn env_bool(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref().map(str::trim),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// Read a non-empty, trimmed env var.
pub(crate) fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve an executable on `PATH` (first match). Checks the executable bit on
/// unix; honors `PATHEXT` on Windows.
pub(crate) fn which_tool(tool: &str) -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if let Some(found) = resolve_in_dir(&dir, tool) {
            return Some(found.display().to_string());
        }
    }
    None
}

fn resolve_in_dir(dir: &Path, tool: &str) -> Option<PathBuf> {
    let direct = dir.join(tool);
    if is_executable(&direct) {
        return Some(direct);
    }
    #[cfg(windows)]
    {
        if let Some(exts) = std::env::var_os("PATHEXT") {
            for ext in std::env::split_paths(&exts) {
                let cand = dir.join(format!("{tool}{}", ext.to_string_lossy()));
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
