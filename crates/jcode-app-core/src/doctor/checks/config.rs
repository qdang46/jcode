//! Config validation: locate global + project `config.toml` and validate TOML
//! syntax. (Strict unknown-key detection can follow once `jcode-config-types`
//! derives `serde(deny_unknown_fields)`.)

use super::super::types::{CheckCategory, DoctorOptions, Finding};

pub fn check_config(opts: &DoctorOptions, out: &mut Vec<Finding>) {
    let global = crate::storage::jcode_dir()
        .ok()
        .map(|h| h.join("config.toml"));
    let project = opts.cwd.join(".jcode").join("config.toml");

    let mut found_any = false;
    for (label, path) in [("global", global), ("project", Some(project))] {
        let Some(path) = path else { continue };
        if !path.is_file() {
            continue;
        }
        found_any = true;
        match std::fs::read_to_string(&path) {
            Err(e) => out.push(
                Finding::fail(
                    CheckCategory::Config,
                    format!("{label} config.toml unreadable"),
                )
                .with_detail(e.to_string())
                .with_remediation(format!("check permissions on {}", path.display())),
            ),
            Ok(text) => match toml::from_str::<toml::Value>(&text) {
                Err(e) => {
                    // Only surface the location line ("TOML parse error at line N,
                    // column C"); never the source snippet, which can echo secrets.
                    let location = e
                        .to_string()
                        .lines()
                        .next()
                        .unwrap_or("syntax error")
                        .to_string();
                    out.push(
                        Finding::fail(
                            CheckCategory::Config,
                            format!("{label} config.toml has a syntax error"),
                        )
                        .with_detail(location)
                        .with_remediation(format!("fix the TOML syntax in {}", path.display())),
                    )
                }
                Ok(_) => out.push(Finding::ok(
                    CheckCategory::Config,
                    format!("{label} config.toml valid"),
                )),
            },
        }
    }

    if !found_any {
        out.push(Finding::ok(
            CheckCategory::Config,
            "no config.toml (using built-in defaults)",
        ));
    }
}
