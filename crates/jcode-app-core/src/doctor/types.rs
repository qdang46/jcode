//! Core types for `jcode doctor`: severity, categories, findings, report, options.

use serde::Serialize;

/// JSON schema version for `--json` output. Bump on any breaking shape change.
pub const DOCTOR_SCHEMA_VERSION: u32 = 1;

/// Severity of a single finding. Ordered so the report's overall status is the
/// max severity across all findings (`Fail` > `Warn` > `Ok`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warn,
    Fail,
}

impl Severity {
    pub fn badge(self) -> &'static str {
        match self {
            Severity::Ok => "[ ok ]",
            Severity::Warn => "[warn]",
            Severity::Fail => "[FAIL]",
        }
    }
}

/// Which subsystem a check belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCategory {
    Build,
    Platform,
    Storage,
    Config,
    Auth,
    Shell,
    Sessions,
    Mcp,
    Resource,
    Swarm,
}

impl CheckCategory {
    pub const ALL: [CheckCategory; 10] = [
        CheckCategory::Build,
        CheckCategory::Platform,
        CheckCategory::Storage,
        CheckCategory::Config,
        CheckCategory::Auth,
        CheckCategory::Shell,
        CheckCategory::Sessions,
        CheckCategory::Mcp,
        CheckCategory::Resource,
        CheckCategory::Swarm,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CheckCategory::Build => "Build",
            CheckCategory::Platform => "Platform",
            CheckCategory::Storage => "Storage",
            CheckCategory::Config => "Configuration",
            CheckCategory::Auth => "Authentication",
            CheckCategory::Shell => "Shell tools",
            CheckCategory::Sessions => "Sessions",
            CheckCategory::Mcp => "MCP",
            CheckCategory::Resource => "Resources",
            CheckCategory::Swarm => "Swarm",
        }
    }

    /// Parse a category from a `--only` token (case-insensitive, with aliases).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "build" => Some(CheckCategory::Build),
            "platform" | "os" => Some(CheckCategory::Platform),
            "storage" | "dirs" | "directories" => Some(CheckCategory::Storage),
            "config" | "configuration" => Some(CheckCategory::Config),
            "auth" | "authentication" => Some(CheckCategory::Auth),
            "shell" | "tools" => Some(CheckCategory::Shell),
            "sessions" | "session" => Some(CheckCategory::Sessions),
            "mcp" => Some(CheckCategory::Mcp),
            "resource" | "resources" => Some(CheckCategory::Resource),
            "swarm" | "coordination" => Some(CheckCategory::Swarm),
            _ => None,
        }
    }
}

/// How a finding relates to `--fix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fixability {
    /// Nothing to fix (informational / passing).
    NotApplicable,
    /// Could be auto-fixed; `--fix` was not given.
    AutoFixable,
    /// `--fix` ran and repaired this.
    Fixed,
    /// `--fix` tried but failed.
    FixFailed,
}

/// A single check outcome.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub category: CheckCategory,
    pub status: Severity,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    pub fixability: Fixability,
}

impl Finding {
    fn new(category: CheckCategory, status: Severity, summary: impl Into<String>) -> Self {
        Finding {
            category,
            status,
            summary: summary.into(),
            detail: None,
            remediation: None,
            fixability: Fixability::NotApplicable,
        }
    }
    pub fn ok(c: CheckCategory, s: impl Into<String>) -> Self {
        Self::new(c, Severity::Ok, s)
    }
    pub fn warn(c: CheckCategory, s: impl Into<String>) -> Self {
        Self::new(c, Severity::Warn, s)
    }
    pub fn fail(c: CheckCategory, s: impl Into<String>) -> Self {
        Self::new(c, Severity::Fail, s)
    }
    pub fn with_detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
    pub fn with_remediation(mut self, r: impl Into<String>) -> Self {
        self.remediation = Some(r.into());
        self
    }
    /// Mark that `--fix` could repair this (advertised when `--fix` is absent).
    pub fn auto_fixable(mut self) -> Self {
        self.fixability = Fixability::AutoFixable;
        self
    }
    /// Mark that `--fix` repaired this; clears the failing status.
    pub fn fixed(mut self, note: impl Into<String>) -> Self {
        self.fixability = Fixability::Fixed;
        self.status = Severity::Ok;
        self.detail = Some(note.into());
        self
    }
    /// Mark that `--fix` attempted but failed (stays failing).
    pub fn fix_failed(mut self, err: impl Into<String>) -> Self {
        self.fixability = Fixability::FixFailed;
        self.detail = Some(err.into());
        self
    }
    /// A failing finding that was not repaired (drives the process exit code).
    pub fn is_unfixed_fail(&self) -> bool {
        self.status == Severity::Fail && self.fixability != Fixability::Fixed
    }
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub struct Counts {
    pub ok: usize,
    pub warn: usize,
    pub fail: usize,
    pub fixed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub overall: Severity,
    pub counts: Counts,
    pub findings: Vec<Finding>,
}

impl DoctorReport {
    pub fn from_findings(findings: Vec<Finding>) -> Self {
        let mut counts = Counts::default();
        let mut overall = Severity::Ok;
        for f in &findings {
            match f.status {
                Severity::Ok => counts.ok += 1,
                Severity::Warn => counts.warn += 1,
                Severity::Fail => counts.fail += 1,
            }
            if f.fixability == Fixability::Fixed {
                counts.fixed += 1;
            }
            if f.status > overall {
                overall = f.status;
            }
        }
        DoctorReport {
            schema_version: DOCTOR_SCHEMA_VERSION,
            overall,
            counts,
            findings,
        }
    }

    pub fn has_unfixed_fail(&self) -> bool {
        self.findings.iter().any(Finding::is_unfixed_fail)
    }
}

/// Options controlling a doctor run.
#[derive(Debug, Clone)]
pub struct DoctorOptions {
    pub cwd: std::path::PathBuf,
    pub fix: bool,
    pub assume_yes: bool,
    pub only: Vec<CheckCategory>,
    pub json: bool,
}

impl DoctorOptions {
    /// Whether `category` should run given the `--only` filter.
    pub fn runs(&self, category: CheckCategory) -> bool {
        self.only.is_empty() || self.only.contains(&category)
    }
}
