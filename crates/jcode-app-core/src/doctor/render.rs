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
            if f.status != Severity::Ok
                && f.fixability != Fixability::Fixed
                && let Some(rem) = &f.remediation
            {
                println!("         -> {}", redact(rem));
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
/// Redact secret-looking values before output (defense-in-depth). Handles
/// `key: value`, `key = value`, quoted JSON/TOML values (`"token": "..."`,
/// `api_key = "..."`), env-style keys (`OPENAI_API_KEY=...`), and
/// `Authorization: Bearer <token>`. The value class covers base64 (`+ / =`).
fn redact(s: &str) -> String {
    use std::sync::OnceLock;
    static SCHEME_RE: OnceLock<regex::Regex> = OnceLock::new();
    static KV_RE: OnceLock<regex::Regex> = OnceLock::new();
    // `Bearer <token>` / `Basic <token>`: redact the credential, not the scheme word.
    let scheme_re = SCHEME_RE.get_or_init(|| {
        regex::Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/\-]{8,}={0,2}")
            .expect("valid scheme redaction regex")
    });
    // A key whose name contains a sensitive token, an optional-quote separator,
    // then a value. Matches quoted and unquoted JSON/TOML/env forms.
    let kv_re = KV_RE.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)([A-Za-z0-9_]*(?:api[_-]?key|secret|token|password|authorization|auth[_-]?token|access[_-]?key|client[_-]?secret|credential)[A-Za-z0-9_]*)(\s*["']?\s*[:=]\s*["']?)([A-Za-z0-9._~+/\-]{6,}={0,2})"#,
        )
        .expect("valid key-value redaction regex")
    });
    let scrubbed = scheme_re.replace_all(s, |c: &regex::Captures| format!("{} <redacted>", &c[1]));
    kv_re
        .replace_all(scrubbed.as_ref(), "${1}${2}<redacted>")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_quoted_json_value() {
        let out = redact(r#"{"token": "abcdef123456"}"#);
        assert!(!out.contains("abcdef123456"), "leaked: {out}");
        assert!(out.contains("<redacted>"));
    }

    #[test]
    fn redacts_quoted_toml_value() {
        let out = redact(r#"api_key = "sk-proj-abcdef123""#);
        assert!(!out.contains("sk-proj-abcdef123"), "leaked: {out}");
    }

    #[test]
    fn redacts_env_style_key() {
        let out = redact("OPENAI_API_KEY=sk-proj-aBcDeFgH12345");
        assert!(!out.contains("sk-proj-aBcDeFgH12345"), "leaked: {out}");
    }

    #[test]
    fn redacts_bearer_token_not_just_scheme() {
        let out = redact("authorization: Bearer eyJhbGc.abc_def-12345");
        assert!(!out.contains("eyJhbGc.abc_def-12345"), "leaked: {out}");
    }

    #[test]
    fn leaves_non_secret_text_untouched() {
        let out = redact("auth.json is group/world accessible (mode 644)");
        assert_eq!(out, "auth.json is group/world accessible (mode 644)");
    }
}
