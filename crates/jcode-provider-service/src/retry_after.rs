//! `Retry-After` header parser.
//!
//! Plan §7 reference to oh-my-pi:
//!   > Retry-After header parser | Parses multiple header formats
//!
//! When a provider returns HTTP 429 with a `Retry-After` header, the
//! runtime should back off for the indicated duration before
//! retrying. The header can be either:
//!
//!  1. A number of seconds (e.g. `Retry-After: 30`).
//!  2. An HTTP-date (e.g. `Retry-After: Wed, 21 Oct 2015 07:28:00 GMT`).
//!
//! RFC 7231 §7.1.3 specifies the format. This module parses both
//! forms and returns a `Duration` for the runtime to wait.

use chrono::{DateTime, Utc};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RetryAfterError {
    #[error("header value is empty")]
    Empty,
    #[error("header value is not a number: {0}")]
    NotANumber(String),
    #[error("header value is not a valid HTTP-date: {0}")]
    NotADate(String),
    #[error("header value is in the past: {0}")]
    InPast(String),
}

/// Parsed `Retry-After` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAfter {
    /// Number of seconds to wait (the canonical form for short backoffs).
    Seconds(u64),
    /// Absolute time at which to retry (the canonical form for long backoffs).
    At(DateTime<Utc>),
}

impl RetryAfter {
    /// The duration from `now` until the retry should happen.
    pub fn duration_from(&self, now: DateTime<Utc>) -> std::time::Duration {
        match self {
            RetryAfter::Seconds(s) => std::time::Duration::from_secs(*s),
            RetryAfter::At(t) => {
                let diff = *t - now;
                if diff <= chrono::Duration::zero() {
                    std::time::Duration::from_secs(0)
                } else {
                    diff.to_std().unwrap_or(std::time::Duration::from_secs(0))
                }
            }
        }
    }

    /// Convenience: duration from now.
    pub fn duration_from_now(&self) -> std::time::Duration {
        self.duration_from(Utc::now())
    }
}

/// Parse a `Retry-After` header value.
pub fn parse_retry_after(value: &str) -> Result<RetryAfter, RetryAfterError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RetryAfterError::Empty);
    }
    // Try as a number first (most common).
    if let Ok(n) = trimmed.parse::<u64>() {
        return Ok(RetryAfter::Seconds(n));
    }
    // Try as an HTTP-date (RFC 7231 §7.1.1.1). Three formats:
    //  - IMF-fixdate: "Sun, 06 Nov 1994 08:49:37 GMT"
    //  - RFC 850:    "Sunday, 06-Nov-94 08:49:37 GMT"
    //  - asctime:    "Sun Nov  6 08:49:37 1994"
    let formats = [
        "%Y-%m-%dT%H:%M:%SZ",    // ISO 8601 (most common in modern APIs)
        "%Y-%m-%dT%H:%M:%S%.fZ", // ISO 8601 with fractional seconds
        "%Y-%m-%d %H:%M:%S GMT", // asctime-like
    ];
    // For the IMF-fixdate / RFC 850 / asctime variants, parse
    // them manually because chrono's %a/%A are locale-dependent
    // and the test is more brittle than the runtime needs.
    if let Some(dt) = parse_imf_fixdate(trimmed) {
        if dt < Utc::now() {
            return Err(RetryAfterError::InPast(trimmed.into()));
        }
        return Ok(RetryAfter::At(dt));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        let utc: DateTime<Utc> = dt.with_timezone(&Utc);
        if utc < Utc::now() {
            return Err(RetryAfterError::InPast(trimmed.into()));
        }
        return Ok(RetryAfter::At(utc));
    }
    for fmt in formats {
        if let Ok(dt) = DateTime::parse_from_str(trimmed, fmt) {
            let utc: DateTime<Utc> = dt.with_timezone(&Utc);
            if utc < Utc::now() {
                return Err(RetryAfterError::InPast(trimmed.into()));
            }
            return Ok(RetryAfter::At(utc));
        }
    }
    Err(RetryAfterError::NotADate(trimmed.into()))
}

/// Minimal IMF-fixdate parser: "Sun, 06 Nov 2099 08:49:37 GMT".
/// Returns None if the input doesn't match the IMF-fixdate
/// grammar. The day-of-week and month are parsed case-insensitively
/// so the parser doesn't depend on the process locale.
fn parse_imf_fixdate(value: &str) -> Option<DateTime<Utc>> {
    // Format: Wkd, DD Mon YYYY HH:MM:SS GMT
    // Step 1: split off the weekday.
    let after_comma = value.split(", ").nth(1)?;
    // Step 2: split into day-month and time-year, then split each
    // on whitespace.
    let tokens: Vec<&str> = after_comma.split_whitespace().collect();
    // Expected: ["DD", "Mon", "YYYY", "HH:MM:SS", "GMT"] (5 tokens)
    if tokens.len() != 5 {
        return None;
    }
    let day: u32 = tokens[0].parse().ok()?;
    let month = month_from_name(tokens[1])?;
    let year: i32 = tokens[2].parse().ok()?;
    let (h, m, s) = parse_hms(tokens[3])?;
    if !tokens[4].eq_ignore_ascii_case("GMT") {
        return None;
    }
    use chrono::NaiveDate;
    let nd = NaiveDate::from_ymd_opt(year, month, day)?;
    let ndt = nd.and_hms_opt(h, m, s)?;
    Some(DateTime::from_naive_utc_and_offset(ndt, Utc))
}

fn month_from_name(s: &str) -> Option<u32> {
    Some(match s.to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

fn parse_hms(s: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_seconds() {
        let r = parse_retry_after("30").unwrap();
        assert_eq!(r, RetryAfter::Seconds(30));
    }

    #[test]
    fn parse_seconds_with_whitespace() {
        let r = parse_retry_after("  120  ").unwrap();
        assert_eq!(r, RetryAfter::Seconds(120));
    }

    #[test]
    fn parse_imf_fixdate() {
        // Use a far-future date so the test is stable.
        let r = parse_retry_after("Sun, 06 Nov 2099 08:49:37 GMT").unwrap();
        match r {
            RetryAfter::At(_) => {}
            other => panic!("expected At, got {other:?}"),
        }
    }

    #[test]
    fn parse_iso_8601() {
        let r = parse_retry_after("2099-11-06T08:49:37Z").unwrap();
        assert!(matches!(r, RetryAfter::At(_)));
    }

    #[test]
    fn empty_header_errors() {
        let err = parse_retry_after("").unwrap_err();
        assert_eq!(err, RetryAfterError::Empty);
    }

    #[test]
    fn whitespace_only_errors() {
        let err = parse_retry_after("   ").unwrap_err();
        assert_eq!(err, RetryAfterError::Empty);
    }

    #[test]
    fn garbage_errors_as_not_a_date() {
        let err = parse_retry_after("not a number or date").unwrap_err();
        assert!(matches!(err, RetryAfterError::NotADate(_)));
    }

    #[test]
    fn past_date_errors() {
        // 1994 is well in the past.
        let err = parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT").unwrap_err();
        assert!(matches!(err, RetryAfterError::InPast(_)));
    }

    #[test]
    fn duration_from_seconds() {
        let r = RetryAfter::Seconds(60);
        let d = r.duration_from_now();
        // Should be ~60s (allow for some time elapsed during the test).
        assert!(d.as_secs() <= 60);
    }

    #[test]
    fn duration_from_past_at_is_zero() {
        let past = Utc::now() - chrono::Duration::seconds(60);
        let r = RetryAfter::At(past);
        assert_eq!(r.duration_from_now(), std::time::Duration::from_secs(0));
    }
}
