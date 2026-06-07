//! `jcode doctor` — comprehensive, offline environment health check with `--fix`.
//!
//! A registry of independent checks (one per [`CheckCategory`]), each returning
//! [`Finding`]s that aggregate into a [`DoctorReport`] rendered as grouped text
//! or `--json`. `--fix` performs non-destructive repairs inline (create missing
//! dirs, tighten `auth.json` permissions); destructive repairs (quarantining a
//! corrupt session) require an interactive confirm or `--yes` and always back
//! up first.
//!
//! This command is OFFLINE and never spends provider balance. For live
//! provider/credential verification use `jcode provider-doctor`,
//! `jcode auth doctor`, and `jcode auth-test`.

mod checks;
mod fix;
mod render;
mod types;

pub use types::{CheckCategory, DoctorOptions, DoctorReport, Finding, Severity};

use anyhow::Result;

/// Run every selected check and build the aggregated report.
pub fn run_doctor(opts: &DoctorOptions) -> DoctorReport {
    let mut f: Vec<Finding> = Vec::new();
    if opts.runs(CheckCategory::Build) {
        checks::build_platform::check_build(&mut f);
    }
    if opts.runs(CheckCategory::Platform) {
        checks::build_platform::check_platform(&mut f);
    }
    if opts.runs(CheckCategory::Storage) {
        checks::storage::check_storage(opts, &mut f);
    }
    if opts.runs(CheckCategory::Config) {
        checks::config::check_config(opts, &mut f);
    }
    if opts.runs(CheckCategory::Auth) {
        checks::auth::check_auth(opts, &mut f);
    }
    if opts.runs(CheckCategory::Shell) {
        checks::shell::check_shell(&mut f);
    }
    if opts.runs(CheckCategory::Sessions) {
        checks::sessions::check_sessions(opts, &mut f);
    }
    if opts.runs(CheckCategory::Mcp) {
        checks::mcp::check_mcp(opts, &mut f);
    }
    if opts.runs(CheckCategory::Resource) {
        checks::resource::check_resource(&mut f);
    }
    if opts.runs(CheckCategory::Swarm) {
        checks::swarm::check_swarm(opts, &mut f);
    }
    DoctorReport::from_findings(f)
}

/// CLI entry: run, render, and return the process exit code (1 when an unfixed
/// failure remains, else 0).
pub fn run(opts: DoctorOptions) -> Result<i32> {
    let report = run_doctor(&opts);
    if opts.json {
        render::print_json(&report)?;
    } else {
        render::print_text(&report);
    }
    Ok(if report.has_unfixed_fail() { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::types::{CheckCategory, Finding, Fixability, Severity};
    use super::*;

    fn opts_for(cwd: std::path::PathBuf) -> DoctorOptions {
        DoctorOptions {
            cwd,
            fix: false,
            assume_yes: false,
            only: Vec::new(),
            json: false,
        }
    }

    #[test]
    fn severity_orders_fail_gt_warn_gt_ok() {
        assert!(Severity::Fail > Severity::Warn);
        assert!(Severity::Warn > Severity::Ok);
    }

    #[test]
    fn category_parse_accepts_aliases_and_rejects_unknown() {
        assert_eq!(
            CheckCategory::parse("authentication"),
            Some(CheckCategory::Auth)
        );
        assert_eq!(CheckCategory::parse("DIRS"), Some(CheckCategory::Storage));
        assert_eq!(CheckCategory::parse("bogus"), None);
    }

    #[test]
    fn report_aggregates_counts_and_overall() {
        let findings = vec![
            Finding::ok(CheckCategory::Build, "a"),
            Finding::warn(CheckCategory::Auth, "b"),
            Finding::fail(CheckCategory::Config, "c"),
        ];
        let report = DoctorReport::from_findings(findings);
        assert_eq!(report.counts.ok, 1);
        assert_eq!(report.counts.warn, 1);
        assert_eq!(report.counts.fail, 1);
        assert_eq!(report.overall, Severity::Fail);
        assert!(report.has_unfixed_fail());
    }

    #[test]
    fn fixed_finding_is_not_an_unfixed_fail() {
        let f = Finding::fail(CheckCategory::Storage, "x");
        assert!(f.is_unfixed_fail());
        let f = Finding::fail(CheckCategory::Storage, "x").fixed("repaired");
        assert!(!f.is_unfixed_fail());
        assert_eq!(f.fixability, Fixability::Fixed);
        assert_eq!(f.status, Severity::Ok);
    }

    #[test]
    fn config_check_flags_bad_toml_in_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".jcode")).unwrap();
        std::fs::write(tmp.path().join(".jcode/config.toml"), "this = = broken").unwrap();
        let mut out = Vec::new();
        checks::config::check_config(&opts_for(tmp.path().to_path_buf()), &mut out);
        assert!(
            out.iter().any(|f| f.category == CheckCategory::Config
                && f.status == Severity::Fail
                && f.summary.contains("project")),
            "expected a failing project config finding, got: {out:?}"
        );
    }

    #[test]
    fn warn_only_report_exits_zero() {
        let report = DoctorReport::from_findings(vec![
            Finding::ok(CheckCategory::Build, "a"),
            Finding::warn(CheckCategory::Auth, "b"),
        ]);
        assert!(
            !report.has_unfixed_fail(),
            "warnings must not cause a nonzero exit"
        );
    }

    #[test]
    fn report_serializes_to_json_with_schema_version() {
        let report = DoctorReport::from_findings(vec![Finding::ok(CheckCategory::Build, "x")]);
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&report).unwrap()).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert!(v.get("findings").is_some());
    }
}
