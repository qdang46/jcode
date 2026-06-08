//! Resource preflight (re-interpretation of pi's "resource governor"): free
//! disk for `~/.jcode`, and session count vs the picker cap. Read-only.

use super::super::types::{CheckCategory, Finding};
use super::env_string;

pub fn check_resource(out: &mut Vec<Finding>) {
    let home = match crate::storage::jcode_dir() {
        Ok(h) => h,
        Err(_) => return,
    };

    #[cfg(unix)]
    match free_disk_mb(&home) {
        Some(free_mb) => {
            let f = if free_mb < 50 {
                Finding::fail(
                    CheckCategory::Resource,
                    format!("low disk: {free_mb} MB free for {}", home.display()),
                )
                .with_remediation("free up disk space before running jcode")
            } else if free_mb < 500 {
                Finding::warn(
                    CheckCategory::Resource,
                    format!("disk getting low: {free_mb} MB free for {}", home.display()),
                )
            } else {
                Finding::ok(
                    CheckCategory::Resource,
                    format!("{free_mb} MB free for {}", home.display()),
                )
            };
            out.push(f);
        }
        None => out.push(Finding::warn(
            CheckCategory::Resource,
            format!("could not check free disk space for {}", home.display()),
        )),
    }

    let sessions = home.join("sessions");
    if sessions.is_dir() {
        // Count session files the same way the sessions check does (non-journal
        // *.json), so the two findings agree and the cap comparison is accurate.
        let count = std::fs::read_dir(&sessions)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| {
                        e.file_name()
                            .to_str()
                            .map(|n| n.ends_with(".json") && !n.ends_with(".journal.json"))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        match env_string("JCODE_SESSION_PICKER_MAX_SESSIONS").and_then(|v| v.parse::<usize>().ok())
        {
            Some(cap) if count > cap => out.push(
                Finding::warn(
                    CheckCategory::Resource,
                    format!(
                        "{count} session file(s) exceed JCODE_SESSION_PICKER_MAX_SESSIONS={cap}"
                    ),
                )
                .with_remediation("raise the cap or archive old sessions"),
            ),
            Some(cap) => out.push(Finding::ok(
                CheckCategory::Resource,
                format!("{count} session file(s), under cap {cap}"),
            )),
            None => out.push(Finding::ok(
                CheckCategory::Resource,
                format!("{count} session file(s)"),
            )),
        }
    }
}

/// Free space in MB available to an unprivileged user at `path` (unix only).
#[cfg(unix)]
fn free_disk_mb(path: &std::path::Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` fills a zeroed struct; we only read scalar fields after
    // checking the return code.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    let block = stat.f_frsize as u64;
    let avail = stat.f_bavail as u64;
    Some(block.saturating_mul(avail) / (1024 * 1024))
}
