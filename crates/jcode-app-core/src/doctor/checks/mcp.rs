//! MCP config checks: validate global + project `mcp.json` parse and report the
//! project-local trust posture.

use super::super::types::{CheckCategory, DoctorOptions, Finding};
use super::env_bool;

pub fn check_mcp(opts: &DoctorOptions, out: &mut Vec<Finding>) {
    let global = crate::storage::jcode_dir().ok().map(|h| h.join("mcp.json"));
    let project = opts.cwd.join(".jcode").join("mcp.json");

    for (label, path) in [("global", global), ("project", Some(project.clone()))] {
        let Some(path) = path else { continue };
        if !path.is_file() {
            continue;
        }
        match std::fs::read_to_string(&path).map(|s| serde_json::from_str::<serde_json::Value>(&s))
        {
            Ok(Ok(_)) => out.push(Finding::ok(
                CheckCategory::Mcp,
                format!("{label} mcp.json valid"),
            )),
            _ => out.push(
                Finding::fail(
                    CheckCategory::Mcp,
                    format!("{label} mcp.json is invalid JSON"),
                )
                .with_remediation(format!("fix or remove {}", path.display())),
            ),
        }
    }

    // Trust posture for project-local servers (mirrors the MVP hint).
    if project.is_file() && !env_bool("JCODE_REQUIRE_MCP_TRUST") {
        out.push(
            Finding::warn(
                CheckCategory::Mcp,
                "project-local .jcode/mcp.json loads without trust gating",
            )
            .with_remediation("set JCODE_REQUIRE_MCP_TRUST=1 to require explicit trust"),
        );
    }

    // mcp_trust.json presence (MVP parity).
    if let Ok(home) = crate::storage::jcode_dir()
        && home.join("mcp_trust.json").is_file()
    {
        out.push(Finding::ok(
            CheckCategory::Mcp,
            "mcp_trust.json present (trust decisions recorded)",
        ));
    }
}
