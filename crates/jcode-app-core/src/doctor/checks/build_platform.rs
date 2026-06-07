//! Build identity + platform + active env flags (migrated from the doctor MVP).

use super::super::types::{CheckCategory, Finding};
use super::{env_bool, env_string};

pub fn check_build(out: &mut Vec<Finding>) {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    out.push(Finding::ok(
        CheckCategory::Build,
        format!("jcode {} [{profile}]", jcode_build_meta::VERSION),
    ));
}

pub fn check_platform(out: &mut Vec<Finding>) {
    out.push(
        Finding::ok(
            CheckCategory::Platform,
            format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
        )
        .with_detail(format!(
            "TERM={} TERM_PROGRAM={} SHELL={}",
            env_string("TERM").unwrap_or_else(|| "(unset)".into()),
            env_string("TERM_PROGRAM").unwrap_or_else(|| "(unset)".into()),
            env_string("SHELL").unwrap_or_else(|| "(unset)".into()),
        )),
    );

    // Active env flags (informational; mirrors the original MVP report).
    let flags = [
        ("JCODE_OFFLINE", "offline"),
        ("JCODE_SAFE_EVAL", "safe-eval"),
        ("JCODE_NO_TELEMETRY", "no-telemetry"),
        ("JCODE_AMBIENT_DISABLED", "ambient-disabled"),
        ("JCODE_REQUIRE_MCP_TRUST", "require-mcp-trust"),
        ("JCODE_NO_UPDATE", "no-update"),
        ("JCODE_NO_CONTEXT_FILES", "no-context-files"),
    ];
    let active: Vec<&str> = flags
        .iter()
        .filter(|(env, _)| env_bool(env))
        .map(|(_, label)| *label)
        .collect();
    if !active.is_empty() {
        out.push(
            Finding::ok(CheckCategory::Platform, "active env flags").with_detail(active.join(", ")),
        );
    }
}
