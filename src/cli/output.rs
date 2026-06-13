pub const QUIET_ENV: &str = "JCODE_QUIET";

/// Output format for structured JSON/TOON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Regular human-readable output (handled by caller)
    Plain,
    /// Pretty-printed JSON
    Json,
    /// Token-efficient JSON (TOON)
    Toon,
}

/// Emit a serializable report in JSON or TOON format.
///
/// # Panics
/// Panics if `format` is [`OutputFormat::Plain`] — use `Plain` to select the
/// human-readable path before calling this helper.
pub fn emit_json_or_toon<T: serde::Serialize>(report: &T, format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?);
        }
        OutputFormat::Toon => {
            let json = serde_json::to_string(report)?;
            println!("{}", toon::json_to_toon(&json)?);
        }
        OutputFormat::Plain => {
            unreachable!("emit_json_or_toon called with Plain format")
        }
    }
    Ok(())
}

pub fn set_quiet_enabled(enabled: bool) {
    if enabled {
        crate::env::set_var(QUIET_ENV, "1");
    } else {
        crate::env::remove_var(QUIET_ENV);
    }
}

pub fn quiet_enabled() -> bool {
    std::env::var(QUIET_ENV)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn stderr_info(message: impl AsRef<str>) {
    if !quiet_enabled() {
        eprintln!("{}", message.as_ref());
    }
}

pub fn stderr_blank_line() {
    if !quiet_enabled() {
        eprintln!();
    }
}
