//! `jcode doctor` MVP — structured environment report (issue #8).
//!
//! Emits a single human-readable text block (or `--json` payload) summarizing
//! the most common things support / users want to confirm before filing a bug:
//!
//!   - jcode build identity (version, git hash, build profile)
//!   - platform basics (os, arch, terminal, $TERM_PROGRAM)
//!   - storage paths (JCODE_HOME)
//!   - on-disk artifacts (auth.json, sessions/, mcp.json, prompts/, themes/)
//!   - active env flags (JCODE_OFFLINE, JCODE_NO_TELEMETRY, JCODE_SAFE_EVAL,
//!     JCODE_AMBIENT_DISABLED, JCODE_REQUIRE_MCP_TRUST, JCODE_TRACE,
//!     JCODE_NO_UPDATE, JCODE_QUIET, JCODE_SCOPED_MODELS, JCODE_SESSION_NAME,
//!     JCODE_NO_CONTEXT_FILES)
//!   - quick health checks (does jcode_dir exist, is it writable, sessions dir
//!     present and traversable, prompts/themes/skills dirs present)
//!
//! Out of scope for this MVP: provider auth-test (covered by the existing
//! `jcode auth-test`), swarm pre-flight, MCP server probes, and structural
//! feature-area checks. Those land in follow-on PRs that build on the same
//! report shape.

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub enum DoctorFormat {
    Text,
    Json,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub build: BuildInfo,
    pub platform: PlatformInfo,
    pub storage: StorageInfo,
    pub flags: FlagsInfo,
    pub health: Vec<HealthCheck>,
}

#[derive(Debug, Serialize)]
pub struct BuildInfo {
    pub version: String,
    pub git_hash: String,
    pub release_build: bool,
}

#[derive(Debug, Serialize)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub shell: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StorageInfo {
    pub jcode_home: Option<String>,
    pub auth_json_present: bool,
    pub sessions_dir_present: bool,
    pub session_count: usize,
    pub prompts_dir_present: bool,
    pub themes_dir_present: bool,
    pub skills_dir_present: bool,
    pub mcp_json_present: bool,
    pub mcp_trust_json_present: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct FlagsInfo {
    pub offline: bool,
    pub no_telemetry: bool,
    pub safe_eval: bool,
    pub ambient_disabled: bool,
    pub require_mcp_trust: bool,
    pub trace: bool,
    pub no_update: bool,
    pub no_context_files: bool,
    pub scoped_models: Option<String>,
    pub session_name: Option<String>,
    pub system_prompt_set: bool,
    pub append_system_prompt_set: bool,
}

#[derive(Debug, Serialize)]
pub struct HealthCheck {
    pub area: &'static str,
    pub status: HealthStatus,
    pub detail: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Warn,
    Fail,
}

fn env_bool(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref().map(str::trim),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn dir_present(path: &Path) -> bool {
    path.is_dir()
}

fn file_present(path: &Path) -> bool {
    path.is_file()
}

fn count_dir_entries(path: &Path) -> usize {
    std::fs::read_dir(path)
        .map(|it| it.filter_map(Result::ok).count())
        .unwrap_or(0)
}

pub fn collect_report() -> DoctorReport {
    let jcode_home = crate::storage::jcode_dir().ok();
    let storage = collect_storage(jcode_home.as_deref());
    let flags = collect_flags();
    let health = collect_health(jcode_home.as_deref(), &storage);
    DoctorReport {
        build: BuildInfo {
            version: env!("JCODE_VERSION").to_string(),
            git_hash: env!("JCODE_GIT_HASH").to_string(),
            release_build: !cfg!(debug_assertions),
        },
        platform: PlatformInfo {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            term: env_string("TERM"),
            term_program: env_string("TERM_PROGRAM"),
            shell: env_string("SHELL"),
        },
        storage,
        flags,
        health,
    }
}

fn collect_storage(jcode_home: Option<&Path>) -> StorageInfo {
    let mut info = StorageInfo {
        jcode_home: jcode_home.map(|p| p.display().to_string()),
        auth_json_present: false,
        sessions_dir_present: false,
        session_count: 0,
        prompts_dir_present: false,
        themes_dir_present: false,
        skills_dir_present: false,
        mcp_json_present: false,
        mcp_trust_json_present: false,
    };
    let Some(home) = jcode_home else {
        return info;
    };
    info.auth_json_present = file_present(&home.join("auth.json"));
    let sessions = home.join("sessions");
    info.sessions_dir_present = dir_present(&sessions);
    if info.sessions_dir_present {
        info.session_count = count_dir_entries(&sessions);
    }
    info.prompts_dir_present = dir_present(&home.join("prompts"));
    info.themes_dir_present = dir_present(&home.join("themes"));
    info.skills_dir_present = dir_present(&home.join("skills"));
    info.mcp_json_present = file_present(&home.join("mcp.json"));
    info.mcp_trust_json_present = file_present(&home.join("mcp_trust.json"));
    info
}

fn collect_flags() -> FlagsInfo {
    FlagsInfo {
        offline: env_bool("JCODE_OFFLINE"),
        no_telemetry: env_bool("JCODE_NO_TELEMETRY") || env_bool("DO_NOT_TRACK"),
        safe_eval: env_bool("JCODE_SAFE_EVAL"),
        ambient_disabled: env_bool("JCODE_AMBIENT_DISABLED"),
        require_mcp_trust: env_bool("JCODE_REQUIRE_MCP_TRUST"),
        trace: env_bool("JCODE_TRACE"),
        no_update: env_bool("JCODE_NO_UPDATE"),
        no_context_files: env_bool("JCODE_NO_CONTEXT_FILES") || env_bool("JCODE_NC"),
        scoped_models: env_string("JCODE_SCOPED_MODELS"),
        session_name: env_string("JCODE_SESSION_NAME"),
        system_prompt_set: env_string("JCODE_SYSTEM_PROMPT").is_some(),
        append_system_prompt_set: env_string("JCODE_APPEND_SYSTEM_PROMPT").is_some(),
    }
}

fn collect_health(jcode_home: Option<&Path>, storage: &StorageInfo) -> Vec<HealthCheck> {
    let mut checks = Vec::new();
    let home = jcode_home;

    // home dir exists + writable
    match home {
        Some(home) => {
            if !home.exists() {
                checks.push(HealthCheck {
                    area: "home",
                    status: HealthStatus::Warn,
                    detail: format!(
                        "JCODE_HOME does not exist yet: {} (will be created on first write)",
                        home.display()
                    ),
                });
            } else {
                let probe = home.join(".doctor-probe");
                let writable = std::fs::write(&probe, b"ok")
                    .and_then(|_| std::fs::remove_file(&probe))
                    .is_ok();
                if writable {
                    checks.push(HealthCheck {
                        area: "home",
                        status: HealthStatus::Ok,
                        detail: format!("JCODE_HOME is writable: {}", home.display()),
                    });
                } else {
                    checks.push(HealthCheck {
                        area: "home",
                        status: HealthStatus::Fail,
                        detail: format!("JCODE_HOME not writable: {}", home.display()),
                    });
                }
            }
        }
        None => checks.push(HealthCheck {
            area: "home",
            status: HealthStatus::Fail,
            detail: "could not resolve JCODE_HOME — check $HOME / $JCODE_HOME".to_string(),
        }),
    }

    // auth state hint
    if !storage.auth_json_present {
        checks.push(HealthCheck {
            area: "auth",
            status: HealthStatus::Warn,
            detail: "no auth.json — run `jcode login --provider <name>` to set up a provider"
                .to_string(),
        });
    } else {
        checks.push(HealthCheck {
            area: "auth",
            status: HealthStatus::Ok,
            detail: "auth.json present — run `jcode auth-test --all-configured` to validate"
                .to_string(),
        });
    }

    // session storage
    if storage.session_count > 0 {
        checks.push(HealthCheck {
            area: "sessions",
            status: HealthStatus::Ok,
            detail: format!(
                "{} session entries found in {}/sessions",
                storage.session_count,
                home.map(|p| p.display().to_string())
                    .unwrap_or_else(|| "JCODE_HOME".to_string()),
            ),
        });
    }

    // safe-eval banner
    if storage.jcode_home.as_deref().is_some_and(|p| {
        PathBuf::from(p)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == ".jcode-safe-eval")
    }) {
        checks.push(HealthCheck {
            area: "safe-eval",
            status: HealthStatus::Ok,
            detail: "running in --safe-eval profile (isolated home)".to_string(),
        });
    }

    // mcp trust posture
    if storage.mcp_json_present {
        checks.push(HealthCheck {
            area: "mcp",
            status: HealthStatus::Ok,
            detail: "global ~/.jcode/mcp.json present".to_string(),
        });
    }
    let project_local_mcp = std::path::Path::new(".jcode/mcp.json");
    if project_local_mcp.exists() {
        let detail = if env_bool("JCODE_REQUIRE_MCP_TRUST") {
            "project-local .jcode/mcp.json present; trust gate is enabled (see `jcode mcp list` when wired by PR #209)".to_string()
        } else {
            "project-local .jcode/mcp.json will load without trust gating (set JCODE_REQUIRE_MCP_TRUST=1 to enforce)".to_string()
        };
        checks.push(HealthCheck {
            area: "mcp",
            status: HealthStatus::Ok,
            detail,
        });
    }

    checks
}

pub fn run(format: DoctorFormat) -> Result<()> {
    let report = collect_report();
    match format {
        DoctorFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        DoctorFormat::Text => render_text(&report),
    }
    Ok(())
}

fn render_text(r: &DoctorReport) {
    println!("# jcode doctor\n");

    println!("## build");
    println!("  version       : {}", r.build.version);
    println!("  git_hash      : {}", r.build.git_hash);
    println!(
        "  build_profile : {}",
        if r.build.release_build {
            "release"
        } else {
            "debug"
        }
    );

    println!("\n## platform");
    println!("  os            : {}", r.platform.os);
    println!("  arch          : {}", r.platform.arch);
    println!(
        "  TERM          : {}",
        r.platform.term.as_deref().unwrap_or("(unset)")
    );
    println!(
        "  TERM_PROGRAM  : {}",
        r.platform.term_program.as_deref().unwrap_or("(unset)")
    );
    println!(
        "  SHELL         : {}",
        r.platform.shell.as_deref().unwrap_or("(unset)")
    );

    println!("\n## storage");
    println!(
        "  JCODE_HOME    : {}",
        r.storage.jcode_home.as_deref().unwrap_or("(unresolved)")
    );
    println!("  auth.json     : {}", yesno(r.storage.auth_json_present));
    println!(
        "  sessions/     : {} ({} entries)",
        yesno(r.storage.sessions_dir_present),
        r.storage.session_count
    );
    println!("  prompts/      : {}", yesno(r.storage.prompts_dir_present));
    println!("  themes/       : {}", yesno(r.storage.themes_dir_present));
    println!("  skills/       : {}", yesno(r.storage.skills_dir_present));
    println!("  mcp.json      : {}", yesno(r.storage.mcp_json_present));
    println!(
        "  mcp_trust.json: {}",
        yesno(r.storage.mcp_trust_json_present)
    );

    println!("\n## flags");
    println!("  offline           : {}", yesno(r.flags.offline));
    println!("  no_telemetry      : {}", yesno(r.flags.no_telemetry));
    println!("  safe_eval         : {}", yesno(r.flags.safe_eval));
    println!("  ambient_disabled  : {}", yesno(r.flags.ambient_disabled));
    println!("  require_mcp_trust : {}", yesno(r.flags.require_mcp_trust));
    println!("  trace             : {}", yesno(r.flags.trace));
    println!("  no_update         : {}", yesno(r.flags.no_update));
    println!("  no_context_files  : {}", yesno(r.flags.no_context_files));
    if let Some(scoped) = &r.flags.scoped_models {
        println!("  scoped_models     : {scoped}");
    }
    if let Some(name) = &r.flags.session_name {
        println!("  session_name      : {name}");
    }
    println!("  system_prompt set : {}", yesno(r.flags.system_prompt_set));
    println!(
        "  append_system_prompt set: {}",
        yesno(r.flags.append_system_prompt_set)
    );

    println!("\n## health");
    if r.health.is_empty() {
        println!("  (no checks)");
    } else {
        for h in &r.health {
            let badge = match h.status {
                HealthStatus::Ok => "[ ok ]",
                HealthStatus::Warn => "[warn]",
                HealthStatus::Fail => "[FAIL]",
            };
            println!("  {badge} {:<10} {}", h.area, h.detail);
        }
    }
}

fn yesno(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_flags_reads_truthy_env_vars() {
        let _lock = crate::storage::lock_test_env();
        let prev_offline = std::env::var_os("JCODE_OFFLINE");
        let prev_safe = std::env::var_os("JCODE_SAFE_EVAL");
        crate::env::set_var("JCODE_OFFLINE", "1");
        crate::env::set_var("JCODE_SAFE_EVAL", "true");

        let flags = collect_flags();

        if let Some(p) = prev_offline {
            crate::env::set_var("JCODE_OFFLINE", p);
        } else {
            crate::env::remove_var("JCODE_OFFLINE");
        }
        if let Some(p) = prev_safe {
            crate::env::set_var("JCODE_SAFE_EVAL", p);
        } else {
            crate::env::remove_var("JCODE_SAFE_EVAL");
        }

        assert!(flags.offline);
        assert!(flags.safe_eval);
    }

    #[test]
    fn report_serializes_to_valid_json() {
        let report = collect_report();
        let json = serde_json::to_string(&report).expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parses back");
        assert!(value.get("build").is_some());
        assert!(value.get("platform").is_some());
        assert!(value.get("storage").is_some());
        assert!(value.get("flags").is_some());
        assert!(value.get("health").is_some());
    }

    #[test]
    fn yesno_helper() {
        assert_eq!(yesno(true), "yes");
        assert_eq!(yesno(false), "no");
    }
}
