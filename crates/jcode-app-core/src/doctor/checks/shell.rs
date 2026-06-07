//! Shell-tool detection: verify the external binaries jcode shells out to are
//! present on `PATH`.

use super::super::types::{CheckCategory, Finding};
use super::which_tool;

/// (binary, remediation hint). Missing tools are warnings, not hard failures.
const TOOLS: &[(&str, &str)] = &[
    (
        "git",
        "install git to enable VCS-aware features and swarm checks",
    ),
    ("rg", "install ripgrep (rg) for fast code search"),
];

pub fn check_shell(out: &mut Vec<Finding>) {
    for (tool, hint) in TOOLS {
        match which_tool(tool) {
            Some(path) => out
                .push(Finding::ok(CheckCategory::Shell, format!("{tool} found")).with_detail(path)),
            None => out.push(
                Finding::warn(CheckCategory::Shell, format!("{tool} not found on PATH"))
                    .with_remediation(*hint),
            ),
        }
    }
}
