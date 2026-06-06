//! Input sanitization — normalize whitespace, strip ANSI, lowercase for matching.

/// Sanitize user input for keyword detection.
///
/// - Strips ANSI escape sequences
/// - Normalizes whitespace (collapse runs, trim)
/// - Preserves original positions for highlight mapping
pub fn sanitize(input: &str) -> String {
    let stripped = strip_ansi(input);
    normalize_whitespace(&stripped)
}

/// Strip ANSI escape sequences from text.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_escape = false;
    for ch in input.chars() {
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if ch.is_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        out.push(ch);
    }
    out
}

/// Normalize whitespace: collapse runs of whitespace into single spaces, trim.
fn normalize_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_was_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                out.push(' ');
            }
            prev_was_space = true;
        } else {
            out.push(ch);
            prev_was_space = false;
        }
    }
    out.trim().to_string()
}

/// Lowercase a string for case-insensitive matching.
pub fn to_lower(input: &str) -> String {
    input.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn normalize_whitespace_collapses_runs() {
        assert_eq!(normalize_whitespace("  hello   world  "), "hello world");
        assert_eq!(normalize_whitespace("single"), "single");
        assert_eq!(normalize_whitespace(""), "");
    }

    #[test]
    fn sanitize_full_pipeline() {
        assert_eq!(sanitize("\x1b[1m  $ultrawork  \x1b[0m"), "$ultrawork");
    }

    #[test]
    fn to_lower_converts() {
        assert_eq!(to_lower("Hello WORLD"), "hello world");
    }
}
