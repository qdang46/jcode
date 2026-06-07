//! Text + JSON rendering for doctor reports, with secret redaction.

use super::types::{CheckCategory, DoctorReport, Fixability, Severity};

/// Render the report as pretty JSON (secrets redacted).
pub fn print_json(report: &DoctorReport) -> anyhow::Result<()> {
    let redacted = redact_report(report);
    println!("{}", serde_json::to_string_pretty(&redacted)?);
    Ok(())
}

/// Render the report as grouped human-readable text.
pub fn print_text(report: &DoctorReport) {
    println!("# jcode doctor\n");
    for category in CheckCategory::ALL {
        let group: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == category)
            .collect();
        if group.is_empty() {
            continue;
        }
        println!("## {}", category.label());
        for f in group {
            let badge = if f.fixability == Fixability::Fixed {
                "[fixed]"
            } else {
                f.status.badge()
            };
            println!("  {badge} {}", redact(&f.summary));
            if let Some(detail) = &f.detail {
                println!("         {}", redact(detail));
            }
            if f.status != Severity::Ok && f.fixability != Fixability::Fixed {
                if let Some(rem) = &f.remediation {
                    println!("         -> {}", redact(rem));
                }
            }
        }
        println!();
    }
    let c = report.counts;
    println!(
        "summary: {} ok | {} warn | {} fail | {} fixed",
        c.ok, c.warn, c.fail, c.fixed
    );
    if report.has_unfixed_fail() {
        println!("Run `jcode doctor --fix` to repair auto-fixable issues.");
    }
}

fn redact_report(report: &DoctorReport) -> DoctorReport {
    let mut r = report.clone();
    for f in &mut r.findings {
        f.summary = redact(&f.summary);
        f.detail = f.detail.as_deref().map(redact);
        f.remediation = f.remediation.as_deref().map(redact);
    }
    r
}

/// Redact secret-looking `key: value` / `key=value` fragments before output.
fn redact(s: &str) -> String {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)(token|secret|api[_-]?key|authorization|bearer|password)(\s*[:=]\s*)([A-Za-z0-9._\-]{6,})",
        )
        .expect("valid redaction regex")
    });
    re.replace_all(s, "${1}${2}<redacted>").into_owned()
}
