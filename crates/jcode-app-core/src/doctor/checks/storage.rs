//! Storage checks: `JCODE_HOME` existence/writability (+ `--fix` mkdir) and
//! standard subdirectory presence.

use super::super::fix::try_autofix;
use super::super::types::{CheckCategory, DoctorOptions, Finding};
use std::path::Path;

pub fn check_storage(opts: &DoctorOptions, out: &mut Vec<Finding>) {
    let home = match crate::storage::jcode_dir() {
        Ok(h) => h,
        Err(e) => {
            out.push(
                Finding::fail(CheckCategory::Storage, "cannot resolve JCODE_HOME")
                    .with_detail(e.to_string())
                    .with_remediation("check $HOME / $JCODE_HOME"),
            );
            return;
        }
    };

    if !home.exists() {
        let f = Finding::warn(
            CheckCategory::Storage,
            format!("JCODE_HOME does not exist: {}", home.display()),
        )
        .with_remediation(format!("mkdir -p {}", home.display()));
        let home2 = home.clone();
        out.push(try_autofix(opts, f, move || {
            std::fs::create_dir_all(&home2)?;
            Ok(format!("created {}", home2.display()))
        }));
    } else {
        match probe_writable(&home) {
            Ok(()) => out.push(Finding::ok(
                CheckCategory::Storage,
                format!("JCODE_HOME writable: {}", home.display()),
            )),
            Err(e) => out.push(
                Finding::fail(
                    CheckCategory::Storage,
                    format!("JCODE_HOME not writable: {}", home.display()),
                )
                .with_detail(e.to_string())
                .with_remediation(format!("chmod u+w {}", home.display())),
            ),
        }
    }

    for sub in ["sessions", "prompts", "themes", "skills"] {
        if home.join(sub).is_dir() {
            out.push(Finding::ok(
                CheckCategory::Storage,
                format!("{sub}/ present"),
            ));
        }
    }
}

fn probe_writable(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(".doctor-probe");
    std::fs::write(&probe, b"ok")?;
    // Ignore remove failure: the write already proved we can create files, and
    // a transient NFS/permission issue on cleanup should not fail the check.
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
