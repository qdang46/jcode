//! Session integrity: scan `~/.jcode/sessions/<id>.json`, flag transcripts that
//! no longer parse, and (with `--fix`) quarantine corrupt files to a `.bak` and
//! remove orphan temp files left by interrupted atomic writes.

use super::super::fix::quarantine;
use super::super::types::{CheckCategory, DoctorOptions, Finding};
use std::path::{Path, PathBuf};

/// Files larger than this are not deeply parsed (avoids reading a multi-GB
/// transcript fully into memory just to validate it).
const MAX_VALIDATE_BYTES: u64 = 64 * 1024 * 1024;

pub fn check_sessions(opts: &DoctorOptions, out: &mut Vec<Finding>) {
    let dir = match crate::storage::jcode_dir() {
        Ok(h) => h.join("sessions"),
        Err(_) => return,
    };
    if !dir.is_dir() {
        out.push(Finding::ok(
            CheckCategory::Sessions,
            "no sessions directory yet",
        ));
        return;
    }

    let mut total = 0usize;
    let mut skipped = 0usize;
    let mut corrupt: Vec<PathBuf> = Vec::new();
    let mut orphan_tmp: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if name.ends_with(".json") && !name.ends_with(".journal.json") {
                total += 1;
                match validate_json(&p) {
                    Some(true) => {}
                    Some(false) => corrupt.push(p),
                    None => skipped += 1, // too large to cheaply validate
                }
            } else if name.contains(".tmp") {
                orphan_tmp.push(p);
            }
        }
    }

    let mut summary = format!("{total} session file(s), {} corrupt", corrupt.len());
    if skipped > 0 {
        summary.push_str(&format!(" ({skipped} too large to validate)"));
    }
    out.push(Finding::ok(CheckCategory::Sessions, summary));

    for path in &corrupt {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let f = Finding::fail(CheckCategory::Sessions, format!("corrupt session: {name}"))
            .with_remediation("run `jcode doctor --fix` to quarantine (.bak), or delete it");
        match quarantine(opts, path, "Quarantine corrupt session") {
            Ok(Some(backup)) => out.push(f.fixed(format!("moved to {}", backup.display()))),
            Ok(None) => out.push(f),
            Err(e) => out.push(f.fix_failed(e.to_string())),
        }
    }

    if !orphan_tmp.is_empty() {
        let f = Finding::warn(
            CheckCategory::Sessions,
            format!(
                "{} orphan temp file(s) from interrupted writes",
                orphan_tmp.len()
            ),
        )
        .with_remediation("run `jcode doctor --fix` to remove them");
        if opts.fix
            && (opts.assume_yes || {
                // Interactive temp removal requires tty confirmation
                // (same discipline as quarantine in fix.rs).
                let prompt = format!(
                    "Remove {} orphan temp file(s) (safe garbage)? [y/N] ",
                    orphan_tmp.len()
                );
                super::super::fix::confirm(&prompt)
            })
        {
            let mut removed = 0usize;
            let mut errors: Vec<String> = Vec::new();
            for p in &orphan_tmp {
                match std::fs::remove_file(p) {
                    Ok(()) => removed += 1,
                    Err(e) => errors.push(e.to_string()),
                }
            }
            if errors.is_empty() {
                out.push(f.fixed(format!("removed {removed} orphan temp file(s)")));
            } else {
                out.push(f.fix_failed(format!(
                    "removed {removed}, {} failed: {}",
                    errors.len(),
                    errors.join("; ")
                )));
            }
        } else {
            out.push(f.auto_fixable());
        }
    }
}

/// Validate that a file is parseable JSON without building a DOM or reading the
/// whole file into a `String`. Returns `None` for files too large to cheaply
/// validate (left untouched).
fn validate_json(path: &Path) -> Option<bool> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_VALIDATE_BYTES {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    Some(serde_json::from_reader::<_, serde::de::IgnoredAny>(reader).is_ok())
}
