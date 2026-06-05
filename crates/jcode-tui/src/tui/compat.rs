//! Compatibility shims for the TUI rendering layer.
//!
//! The TUI is in the middle of a ratatui → frankentui migration (see
//! `feature/ratatui-to-frankentui`). The five `ui_*.rs` view files are
//! being touched by that work and expect a small set of constructor
//! helpers (`line_from_spans`, `text_from_lines`, ...) that work with
//! whatever the underlying TUI library is at the time of import.
//!
//! On the ratatui branch (this one) the helpers are simple pass-throughs
//! that just construct a `ratatui::text::Line` / `Text` from a `Vec<Span>`.
//! When frankentui lands the body of these helpers will swap to the
//! equivalent frankentui types, but the call sites in `ui_*.rs` stay
//! identical (which is the whole point of the shim).
//!
//! Module-level organisation mirrors `feature/ratatui-to-frankentui`'s
//! `src/tui/compat.rs` so the migration can be a single-file replace.

use ratatui::text::{Line, Span, Text};

/// Build a `Line` from a vector of `Span`s. Equivalent to
/// `ratatui::text::Line::from(spans)` but explicit for call sites that
/// need the long form.
pub fn line_from_spans<'a>(spans: Vec<Span<'a>>) -> Line<'a> {
    Line::from(spans)
}

/// Build a `Text` from a vector of `Line`s.
pub fn text_from_lines<'a>(lines: Vec<Line<'a>>) -> Text<'a> {
    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn line_from_spans_preserves_content_and_styles() {
        let spans = vec![
            Span::styled("hello", Style::default()),
            Span::raw(" "),
            Span::styled("world", Style::default()),
        ];
        let line = line_from_spans(spans);
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].content, "hello");
        assert_eq!(line.spans[2].content, "world");
    }

    #[test]
    fn text_from_lines_preserves_line_count() {
        let lines = vec![
            line_from_spans(vec![Span::raw("a")]),
            line_from_spans(vec![Span::raw("b")]),
            line_from_spans(vec![Span::raw("c")]),
        ];
        let text = text_from_lines(lines);
        assert_eq!(text.lines.len(), 3);
        assert_eq!(text.lines[1].spans[0].content, "b");
    }

    #[test]
    fn empty_inputs_produce_empty_containers() {
        let line = line_from_spans(vec![]);
        assert!(line.spans.is_empty());
        let text = text_from_lines(vec![]);
        assert!(text.lines.is_empty());
    }
}
