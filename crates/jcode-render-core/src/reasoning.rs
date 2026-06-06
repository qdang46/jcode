//! Reasoning-line markdown formatting.
//!
//! Pure string helpers shared by the server/streaming path and the TUI renderer
//! so the wrapping/escaping rules stay in lockstep with the renderer that
//! consumes them. These live in `jcode-render-core` (a backend-neutral, pure
//! crate) rather than in `jcode-tui-markdown` so the foundation/streaming layer
//! can format reasoning lines without depending on any `jcode-tui-*` crate.

/// Invisible separator placed just inside both ends of an emphasis run so the
/// flanking `*` are always adjacent to non-whitespace (see
/// [`reasoning_line_markup`]).
pub const REASONING_SENTINEL: &str = "\u{2063}";

/// Escape the characters that would otherwise be interpreted as inline markdown
/// inside a reasoning line, so the body renders literally inside the dim/italic
/// emphasis run.
fn escape_reasoning_inline_markdown(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    for ch in line.chars() {
        match ch {
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '&' | '~' | '|' | '$' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Wrap a completed reasoning line as dim+italic markdown.
///
/// Empty lines become a bare newline (no empty emphasis run). The result always
/// ends in a CommonMark hard break (`"  \n"`).
///
/// The trailing two spaces are a CommonMark *hard break*: without them,
/// consecutive reasoning lines (each terminated by a single `\n`) collapse into
/// one paragraph where the line breaks render as spaces, so multi-line thinking
/// shows up as a single run-on line. The hard break keeps each reasoning line on
/// its own visual row, matching the model's line structure.
///
/// The sentinel must wrap both ends because CommonMark's emphasis flanking rules
/// require the opening `*` to not be followed by whitespace and the closing `*`
/// to not be preceded by whitespace. A reasoning line that starts or ends with
/// whitespace (or is whitespace-only) would otherwise leave the asterisks as
/// literal text and break the dim/italic styling. The zero-width sentinels
/// guarantee both asterisks are flanked by non-whitespace regardless of the body.
pub fn reasoning_line_markup(line: &str) -> String {
    if line.is_empty() {
        "\n".to_string()
    } else {
        format!(
            "*{0}{1}{0}*  \n",
            REASONING_SENTINEL,
            escape_reasoning_inline_markdown(line)
        )
    }
}

/// Wrap the in-progress (not yet newline-terminated) reasoning line as dim+italic
/// markdown, identical to [`reasoning_line_markup`] but *without* the trailing
/// newline so it renders as the live tail of the streaming buffer. Callers
/// truncate and re-emit this tail on each streamed delta so reasoning trickles in
/// token-by-token instead of one whole line at a time. An empty line yields an
/// empty string (nothing to render yet).
pub fn reasoning_partial_markup(line: &str) -> String {
    if line.is_empty() {
        String::new()
    } else {
        format!(
            "*{0}{1}{0}*",
            REASONING_SENTINEL,
            escape_reasoning_inline_markdown(line)
        )
    }
}

/// One-line collapsed reasoning summary markup (e.g. `▸ thought (3 lines)`),
/// styled dim+italic like the live reasoning lines. Used to fold a persisted
/// reasoning block down to a single trace line when the transcript is
/// re-rendered from history in `current` reasoning-display mode (so reloaded /
/// resumed sessions match the live collapse instead of replaying every line).
///
/// Lives here (a backend-neutral, pure crate) rather than in `jcode-tui-markdown`
/// so the foundation/streaming layer can format the summary without depending on
/// any `jcode-tui-*` crate. Re-exported from `jcode-tui-markdown` for the
/// existing `jcode_tui_markdown::reasoning_summary_line_markup` path.
pub fn reasoning_summary_line_markup(line_count: usize) -> String {
    let label = match line_count {
        0 | 1 => "▸ thought".to_string(),
        n => format!("▸ thought ({} lines)", n),
    };
    reasoning_line_markup(&label)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── escape_reasoning_inline_markdown ──────────────────────────────

    #[test]
    fn test_escape_backslash() {
        assert_eq!(escape_reasoning_inline_markdown("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_escape_asterisk() {
        assert_eq!(escape_reasoning_inline_markdown("a*b"), "a\\*b");
    }

    #[test]
    fn test_escape_underscore() {
        assert_eq!(escape_reasoning_inline_markdown("a_b"), "a\\_b");
    }

    #[test]
    fn test_escape_backtick() {
        assert_eq!(escape_reasoning_inline_markdown("a`b"), "a\\`b");
    }

    #[test]
    fn test_escape_bracket() {
        assert_eq!(escape_reasoning_inline_markdown("a[b]c"), "a\\[b\\]c");
    }

    #[test]
    fn test_escape_angle_bracket() {
        assert_eq!(escape_reasoning_inline_markdown("a<b>c"), "a\\<b\\>c");
    }

    #[test]
    fn test_escape_ampersand() {
        assert_eq!(escape_reasoning_inline_markdown("a&b"), "a\\&b");
    }

    #[test]
    fn test_escape_tilde_pipe_dollar() {
        assert_eq!(escape_reasoning_inline_markdown("~|$"), "\\~\\|\\$");
    }

    #[test]
    fn test_plain_text_passes_through_unchanged() {
        assert_eq!(
            escape_reasoning_inline_markdown("hello world 123"),
            "hello world 123"
        );
    }

    #[test]
    fn test_empty_string_returns_empty() {
        assert_eq!(escape_reasoning_inline_markdown(""), "");
    }

    // ── reasoning_line_markup ─────────────────────────────────────────

    #[test]
    fn test_line_markup_empty_returns_newline() {
        assert_eq!(reasoning_line_markup(""), "\n");
    }

    #[test]
    fn test_line_markup_wraps_with_sentinel_and_hard_break() {
        let result = reasoning_line_markup("hello");
        assert!(result.starts_with(&format!("*{}", REASONING_SENTINEL)));
        assert!(result.contains("hello"));
        assert!(result.ends_with("  \n"));
    }

    #[test]
    fn test_line_markup_special_chars_are_escaped() {
        let result = reasoning_line_markup("a*b");
        assert!(result.contains("a\\*b"));
        assert!(!result.contains("a*b"));
    }

    // ── reasoning_partial_markup ───────────────────────────────────────

    #[test]
    fn test_partial_markup_empty_returns_empty_string() {
        assert_eq!(reasoning_partial_markup(""), "");
    }

    #[test]
    fn test_partial_markup_wraps_with_sentinel_no_newline() {
        let result = reasoning_partial_markup("partial");
        assert!(result.starts_with(&format!("*{}", REASONING_SENTINEL)));
        assert!(result.contains("partial"));
        assert!(!result.ends_with("\n"));
    }

    // ── reasoning_summary_line_markup ──────────────────────────────────

    #[test]
    fn test_summary_zero_lines() {
        let result = reasoning_summary_line_markup(0);
        assert!(result.contains("▸ thought"));
        assert!(!result.contains("lines"));
    }

    #[test]
    fn test_summary_one_line() {
        let result = reasoning_summary_line_markup(1);
        assert!(result.contains("▸ thought"));
        assert!(!result.contains("lines"));
    }

    #[test]
    fn test_summary_two_lines_includes_count() {
        let result = reasoning_summary_line_markup(2);
        assert!(result.contains("▸ thought (2 lines)"));
    }

    #[test]
    fn test_summary_many_lines() {
        let result = reasoning_summary_line_markup(5);
        assert!(result.contains("▸ thought (5 lines)"));
    }
}
