//! Auto-repair helpers for `jcode doctor --fix`.
//!
//! Non-destructive fixes (mkdir, chmod) run inline via [`try_autofix`].
//! Destructive fixes (quarantining a corrupt file) go through [`quarantine`],
//! which is gated behind an interactive confirm prompt or `--yes` and ALWAYS
//! backs up by renaming to a timestamped `.bak-<ts>` file rather than deleting
//! (codex `state_db_recovery` pattern).

use super::types::{DoctorOptions, Finding};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Run a non-destructive repair when `--fix` is set; otherwise mark the finding
/// auto-fixable so the report advertises it. `repair` must be idempotent.
pub fn try_autofix<F>(opts: &DoctorOptions, finding: Finding, repair: F) -> Finding
where
    F: FnOnce() -> anyhow::Result<String>,
{
    if !opts.fix {
        return finding.auto_fixable();
    }
    match repair() {
        Ok(note) => finding.fixed(note),
        Err(e) => finding.fix_failed(e.to_string()),
    }
}

/// Quarantine a file by renaming it to `<path>.bak-<unix_ts>` (never deletes).
/// Requires `--fix` plus either a tty confirmation or `--yes`. Returns the
/// backup path, or `Ok(None)` when the action was skipped.
pub fn quarantine(
    opts: &DoctorOptions,
    path: &Path,
    action: &str,
) -> anyhow::Result<Option<PathBuf>> {
    if !opts.fix {
        return Ok(None);
    }
    if !opts.assume_yes && !confirm(&format!("{action} {}? [y/N] ", path.display())) {
        return Ok(None);
    }
    let ts = chrono::Utc::now().timestamp();
    let mut backup = path.as_os_str().to_owned();
    backup.push(format!(".bak-{ts}"));
    let backup = PathBuf::from(backup);
    std::fs::rename(path, &backup)?;
    Ok(Some(backup))
}

/// Prompt on the controlling terminal. Returns false when stdin/stdout is not a
/// tty (non-interactive/CI without `--yes`).
fn confirm(prompt: &str) -> bool {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return false;
    }
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Set file permissions (unix only).
#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}
