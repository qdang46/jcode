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
    let bool_flags = [
        ("JCODE_OFFLINE", "offline"),
        ("JCODE_SAFE_EVAL", "safe-eval"),
        ("JCODE_AMBIENT_DISABLED", "ambient-disabled"),
        ("JCODE_REQUIRE_MCP_TRUST", "require-mcp-trust"),
        ("JCODE_NO_UPDATE", "no-update"),
        ("JCODE_TRACE", "trace"),
    ];
    let mut active: Vec<String> = bool_flags
        .iter()
        .filter(|(env, _)| env_bool(env))
        .map(|(_, label)| (*label).to_string())
        .collect();
    if env_bool("JCODE_NO_TELEMETRY") || env_bool("DO_NOT_TRACK") {
        active.push("no-telemetry".to_string());
    }
    if env_bool("JCODE_NO_CONTEXT_FILES") || env_bool("JCODE_NC") {
        active.push("no-context-files".to_string());
    }
    if env_string("JCODE_SYSTEM_PROMPT").is_some() {
        active.push("system-prompt-set".to_string());
    }
    if env_string("JCODE_APPEND_SYSTEM_PROMPT").is_some() {
        active.push("append-system-prompt-set".to_string());
    }
    if let Some(v) = env_string("JCODE_SCOPED_MODELS") {
        active.push(format!("scoped-models={v}"));
    }
    if let Some(v) = env_string("JCODE_SESSION_NAME") {
        active.push(format!("session-name={v}"));
    }
    if !active.is_empty() {
        out.push(
            Finding::ok(CheckCategory::Platform, "active env flags").with_detail(active.join(", ")),
        );
    }
}
