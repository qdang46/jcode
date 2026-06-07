//! Session integrity: scan `~/.jcode/sessions/<id>.json`, flag transcripts that
//! no longer parse, and (with `--fix`) quarantine corrupt files to a `.bak`.

use super::super::fix::quarantine;
use super::super::types::{CheckCategory, DoctorOptions, Finding};

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
    let mut corrupt: Vec<std::path::PathBuf> = Vec::new();
    let mut orphan_tmp = 0usize;
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
                let valid = std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .is_some();
                if !valid {
                    corrupt.push(p);
                }
            } else if name.contains(".tmp") {
                orphan_tmp += 1;
            }
        }
    }

    out.push(Finding::ok(
        CheckCategory::Sessions,
        format!("{total} session file(s), {} corrupt", corrupt.len()),
    ));

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

    if orphan_tmp > 0 {
        out.push(
            Finding::warn(
                CheckCategory::Sessions,
                format!("{orphan_tmp} orphan temp file(s) from interrupted writes"),
            )
            .auto_fixable(),
        );
    }
}
