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
    // In --json mode never prompt — it would corrupt the JSON on stdout — so a
    // destructive fix requires an explicit `--yes`.
    if opts.json && !opts.assume_yes {
        return Ok(None);
    }
    if !opts.assume_yes && !confirm(&format!("{action} {}? [y/N] ", path.display())) {
        return Ok(None);
    }
    let backup = unique_backup_path(path);
    std::fs::rename(path, &backup)?;
    Ok(Some(backup))
}

/// Build a `<path>.bak-<ts>-<ns>` path that effectively never collides
/// (TOCTOU-safe: nanosecond-granularity timestamp avoids the stat-then-rename
/// race of the old counter-based approach).
fn unique_backup_path(path: &Path) -> PathBuf {
    let ts = chrono::Utc::now().timestamp();
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".bak-{ts}-{ns:09}"));
    PathBuf::from(name)
}

/// Prompt on stderr (so it never corrupts stdout / `--json`). Returns false
/// when stdin or stderr is not a tty (non-interactive / CI without `--yes`).
pub(crate) fn confirm(prompt: &str) -> bool {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return false;
    }
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
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

/// Set file permissions (non-unix: no-op, filesystem ACLs handle this).
#[cfg(not(unix))]
pub fn chmod(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::types::{CheckCategory, Fixability};
    use super::*;

    fn opts(fix: bool, yes: bool, json: bool) -> DoctorOptions {
        DoctorOptions {
            cwd: std::path::PathBuf::from("."),
            fix,
            assume_yes: yes,
            only: Vec::new(),
            json,
        }
    }

    #[test]
    fn try_autofix_only_advertises_without_fix() {
        let f = Finding::warn(CheckCategory::Storage, "x");
        let r = try_autofix(&opts(false, false, false), f, || Ok("done".into()));
        assert_eq!(r.fixability, Fixability::AutoFixable);
    }

    #[test]
    fn try_autofix_runs_repair_with_fix() {
        let f = Finding::warn(CheckCategory::Storage, "x");
        let r = try_autofix(&opts(true, false, false), f, || Ok("done".into()));
        assert_eq!(r.fixability, Fixability::Fixed);
    }

    #[test]
    fn quarantine_is_noop_without_fix() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("corrupt.json");
        std::fs::write(&p, b"{bad").unwrap();
        let r = quarantine(&opts(false, false, false), &p, "Quarantine").unwrap();
        assert!(r.is_none());
        assert!(p.exists(), "file must be untouched without --fix");
    }

    #[test]
    fn quarantine_is_noop_in_json_without_yes() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("corrupt.json");
        std::fs::write(&p, b"{bad").unwrap();
        let r = quarantine(&opts(true, false, true), &p, "Quarantine").unwrap();
        assert!(
            r.is_none(),
            "json mode must not prompt/mutate without --yes"
        );
        assert!(p.exists());
    }

    #[test]
    fn quarantine_backs_up_with_yes() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("corrupt.json");
        std::fs::write(&p, b"{bad").unwrap();
        let backup = quarantine(&opts(true, true, false), &p, "Quarantine")
            .unwrap()
            .expect("should back up");
        assert!(!p.exists(), "original should be moved, not left in place");
        assert!(backup.exists(), "timestamped .bak should exist");
        assert!(!std::fs::read(&backup).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn chmod_sets_mode_600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("auth.json");
        std::fs::write(&p, b"{}").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        chmod(&p, 0o600).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
