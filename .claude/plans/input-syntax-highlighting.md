# Implementation Plan: Input Syntax Highlighting for /slash and $skill

> Generated from research across 9 repos + jcode codebase analysis
> Goal: Add real-time syntax highlighting for `/slash` commands and `$skill` references in the TUI input area

---

## 1. Executive Summary

Currently, jcode's TUI input area renders all user-typed text as plain `Span::raw()` with zero syntax highlighting. This plan adds position-based, priority-resolved highlighting for `/slash` commands and `$skill` references, inspired by claude-code's proven architecture and codex's ratatui-based element overlay approach. The feature will color recognized slash commands in blue and valid skill references in accent color, while invalid/unknown tokens remain unstyled — giving users instant visual feedback as they type.

---

## 2. Architecture Decision

### Chosen Approach: Position-based Priority-resolved Segments

Inspired by **claude-code** (production-grade) and adapted for **ratatui** (same framework as codex).

```
Input: "fix auth /review the $my-skill config"

Segments:
  "fix auth "     → Span::raw (default)
  "/review"       → Span::styled (blue/suggestion color)
  " the "         → Span::raw (default)
  "$my-skill"     → Span::styled (accent color, if valid skill)
  " config"       → Span::raw (default)
```

### Alternatives Considered

| Approach | Source | Pros | Cons | Decision |
|----------|--------|------|------|----------|
| Position-based segments | claude-code | Clean, priority-aware, ANSI-safe | Needs segmenter refactor | **CHOSEN** |
| Extmarks (Neovim-style) | opencode | Very flexible | Requires new abstraction layer in ratatui | Rejected — overkill |
| decorateText hook | oh-my-pi | Simple `(text) => text` | Can't handle multi-span styling easily | Rejected — too limited |
| Element-based overlay | codex | Clean layer separation | All elements same color | Partial — borrow 3-layer idea |

---

## 3. Data Structures & Types

```rust
/// A highlighted range in the input text
#[derive(Clone, Debug, PartialEq, Eq)]
struct InputHighlight {
    /// Start character index (inclusive) in the original input
    start: usize,
    /// End character index (exclusive) in the original input
    end: usize,
    /// Foreground color for this highlight
    color: Color,
    /// Priority for overlap resolution (higher wins)
    priority: u8,
}

/// Categories of highlights we detect
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HighlightKind {
    /// /slash command (valid, known command)
    SlashCommand,
    /// $skill reference (valid, known skill)
    SkillReference,
    /// Unknown /command (not in REGISTERED_COMMANDS)
    UnknownSlash,
    /// Unknown $token (not a known skill)
    UnknownDollar,
}
```

---

## 4. Pseudocode — Core Algorithm

```
FUNCTION find_highlights(input: str, registered_commands, known_skills) -> Vec<InputHighlight>:
    highlights = []

    // --- Find /slash commands ---
    IF input.trim_start().starts_with('/'):
        // Whole-input slash command: "/help foo bar"
        cmd_end = find_end_of_first_word(input.trim_start())
        cmd_name = input.trim_start()[0..cmd_end]
        IF cmd_name in registered_commands:
            highlights.push(InputHighlight {
                start: index_of('/') in input,
                end: index_of('/') + cmd_end,
                color: suggestion_color(),  // blue
                priority: 5,
            })
    ELSE:
        // Mid-input slash commands: "foo /help bar"
        FOR each regex_match of /(^|[\s])(\/[a-zA-Z][a-zA-Z0-9:\-_]*)/ in input:
            cmd_name = match.group(2)
            IF cmd_name in registered_commands:
                highlights.push(InputHighlight {
                    start: match.start + len(match.group(1)),
                    end: match.end,
                    color: suggestion_color(),
                    priority: 5,
                })

    // --- Find $skill references ---
    FOR each regex_match of /(?:^|\s)(\$[a-zA-Z0-9_-]+)/ in input:
        skill_name = match.group(1)[1..]  // strip $
        IF skill_name in known_skills:
            highlights.push(InputHighlight {
                start: match.start + offset_to_$,
                end: match.end,
                color: accent_color(),     // accent/green
                priority: 5,
            })

    RETURN highlights

FUNCTION styled_spans_for_segment(segment: WrappedInputSegment, highlights: &[InputHighlight]) -> Vec<Span>:
    // Split segment text into sub-spans based on highlight overlaps
    spans = []
    seg_start = segment.start_char
    seg_end = segment.end_char
    pos = seg_start

    // Find highlights that overlap with this segment
    overlapping = highlights.filter(h => h.end > seg_start && h.start < seg_end)
    // Sort by start position
    overlapping.sort_by(|a, b| a.start.cmp(b.start))

    FOR each highlight in overlapping:
        hl_start = max(highlight.start, seg_start)
        hl_end = min(highlight.end, seg_end)

        // Default text before highlight
        IF pos < hl_start:
            text = segment.char_range(pos..hl_start)
            spans.push(Span::raw(text))

        // Highlighted text
        text = segment.char_range(hl_start..hl_end)
        spans.push(Span::styled(text, Style::default().fg(highlight.color)))

        pos = hl_end

    // Remaining default text
    IF pos < seg_end:
        text = segment.char_range(pos..seg_end)
        spans.push(Span::raw(text))

    RETURN spans
```

---

## 5. Implementation Code

### File: `crates/jcode-tui/src/tui/ui_input.rs`

#### 5a. Add highlight types (after line 26)

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
struct InputHighlight {
    start: usize,
    end: usize,
    color: Color,
    priority: u8,
}
```

#### 5b. Add `find_input_highlights()` function

```rust
fn find_input_highlights(
    input: &str,
    registered_commands: &[crate::tui::app::state_ui_input_helpers::RegisteredCommand],
    known_skills: &[String],
) -> Vec<InputHighlight> {
    let mut highlights = Vec::new();

    // --- Slash commands ---
    // Whole-input: "/help args" (first word starts with /)
    let trimmed = input.trim_start();
    if trimmed.starts_with('/') {
        let cmd_end = trimmed[1..]
            .find(|c: char| c.is_whitespace())
            .unwrap_or(trimmed.len() - 1);
        let cmd_name = &trimmed[..=cmd_end]; // includes '/'
        let offset = input.len() - trimmed.len();
        if registered_commands.iter().any(|c| c.name == cmd_name) {
            highlights.push(InputHighlight {
                start: offset,
                end: offset + cmd_name.len(),
                color: rgb(100, 180, 255), // suggestion blue
                priority: 5,
            });
        }
    } else {
        // Mid-input: "foo /help bar"
        let re = regex::Regex::new(r"(^|[\s])(/[a-zA-Z][a-zA-Z0-9:\-_]*)").unwrap();
        for cap in re.captures_iter(input) {
            let preceding = cap.get(1).unwrap();
            let cmd = cap.get(2).unwrap();
            if registered_commands.iter().any(|c| c.name == cmd.as_str()) {
                highlights.push(InputHighlight {
                    start: preceding.end(),
                    end: cmd.end(),
                    color: rgb(100, 180, 255),
                    priority: 5,
                });
            }
        }
    }

    // --- $skill references ---
    let re = regex::Regex::new(r"(?:^|\s)(\$[a-zA-Z0-9_-]+)").unwrap();
    for cap in re.captures_iter(input) {
        let token = cap.get(1).unwrap();
        let skill_name = &token.as_str()[1..]; // strip '$'
        if known_skills.iter().any(|s| s == skill_name) {
            highlights.push(InputHighlight {
                start: token.start(),
                end: token.end(),
                color: accent_color(), // accent green
                priority: 5,
            });
        }
    }

    highlights
}
```

#### 5c. Add `styled_segment_spans()` function

```rust
fn styled_segment_spans<'a>(
    segment: &WrappedInputSegment,
    highlights: &[InputHighlight],
) -> Vec<Span<'a>> {
    let seg_start = segment.start_char;
    let seg_end = segment.end_char;

    // Find overlapping highlights, sorted by start
    let mut overlapping: Vec<&InputHighlight> = highlights
        .iter()
        .filter(|h| h.end > seg_start && h.start < seg_end)
        .collect();
    overlapping.sort_by_key(|h| h.start);

    if overlapping.is_empty() {
        return vec![Span::raw(segment.text.clone())];
    }

    let mut spans = Vec::new();
    let mut pos = seg_start;

    for hl in &overlapping {
        let hl_start = hl.start.max(seg_start);
        let hl_end = hl.end.min(seg_end);

        // Default text before highlight
        if pos < hl_start {
            let text = char_slice(&segment.text, pos - seg_start, hl_start - seg_start);
            spans.push(Span::raw(text.to_string()));
        }

        // Highlighted text
        let text = char_slice(&segment.text, hl_start - seg_start, hl_end - seg_start);
        spans.push(Span::styled(
            text.to_string(),
            Style::default().fg(hl.color),
        ));

        pos = hl_end;
    }

    // Remaining default text
    if pos < seg_end {
        let text = char_slice(&segment.text, pos - seg_start, seg_end - seg_start);
        spans.push(Span::raw(text.to_string()));
    }

    spans
}

/// Extract a substring by character indices
fn char_slice(s: &str, start: usize, end: usize) -> &str {
    let byte_start = s.char_indices().nth(start).map(|(i, _)| i).unwrap_or(s.len());
    let byte_end = s.char_indices().nth(end).map(|(i, _)| i).unwrap_or(s.len());
    &s[byte_start..byte_end]
}
```

#### 5d. Modify `wrap_input_text()` — inject highlights

**Before (current):**
```rust
// line 2113
Span::raw(segment.text.clone()),
// line 2118
Span::raw(segment.text.clone()),
```

**After:**
```rust
// Pass highlights through the function signature
pub(crate) fn wrap_input_text<'a>(
    input: &str,
    cursor_pos: usize,
    line_width: usize,
    num_str: &str,
    prompt_char: &'a str,
    caret_color: Color,
    prompt_len: usize,
    highlights: &[InputHighlight],  // NEW parameter
) -> (Vec<Line<'a>>, usize, usize) {
    // ... existing segment creation ...

    for (idx, segment) in wrapped_segments.iter().enumerate() {
        // ... existing cursor tracking ...

        let styled_spans = styled_segment_spans(segment, highlights);

        if idx == 0 {
            let num_color = rainbow_prompt_color(0);
            let mut line_spans = vec![
                Span::styled(num_str.to_string(), Style::default().fg(num_color)),
                Span::styled(prompt_char.to_string(), Style::default().fg(caret_color)),
            ];
            line_spans.extend(styled_spans);
            lines.push(Line::from(line_spans));
        } else {
            let mut line_spans = vec![
                Span::raw(" ".repeat(prompt_len)),
            ];
            line_spans.extend(styled_spans);
            lines.push(Line::from(line_spans));
        }
    }
    // ... rest unchanged ...
}
```

#### 5e. Modify `draw_input()` — compute and pass highlights

```rust
pub(super) fn draw_input(
    frame: &mut Frame,
    app: &dyn TuiState,
    area: Rect,
    next_prompt: usize,
    debug_capture: &mut Option<FrameCaptureBuilder>,
) {
    let input_text = app.input();
    let cursor_pos = app.cursor_pos();

    // NEW: Compute highlights
    let highlights = find_input_highlights(
        input_text,
        REGISTERED_COMMANDS,
        &app.available_skills(),  // need to expose this
    );

    // ... existing code ...

    let (all_lines, cursor_line, cursor_col) = wrap_input_text(
        input_text,
        cursor_pos,
        line_width,
        &num_str,
        prompt_char,
        caret_color,
        prompt_len,
        &highlights,  // NEW
    );

    // ... rest unchanged ...
}
```

---

## 6. Configuration & Wiring

### New dependencies needed
- `regex` crate (likely already in workspace — check Cargo.toml)

### Files to modify
| File | Change |
|------|--------|
| `crates/jcode-tui/src/tui/ui_input.rs` | Add types, highlight finder, styled spans, modify `wrap_input_text` and `draw_input` |
| `crates/jcode-tui/src/tui/mod.rs` | Expose `available_skills()` on `TuiState` trait if not already available |

### Files to reference (read-only)
| File | What to reuse |
|------|---------------|
| `crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs` | `REGISTERED_COMMANDS`, `RegisteredCommand` |
| `crates/jcode-base/src/skill.rs` | `SkillRegistry::parse_invocation()`, `active_dollar_token()` |
| `crates/jcode-tui/src/tui/app/input.rs` | `current_skills_snapshot()` |

### No config/env changes needed
This is a pure rendering enhancement — no new config flags, env vars, or user-facing settings.

---

## 7. Repo References

| Feature Aspect | Repo | Pattern |
|----------------|------|---------|
| Position-based highlight aggregation | claude-code | `src/components/PromptInput/PromptInput.tsx` lines 715-857 |
| Priority-resolved segmenter | claude-code | `src/utils/textHighlighting.ts` — `segmentTextByHighlights()` |
| Slash command regex finder | claude-code | `src/utils/suggestions/commandSuggestions.ts` — `findSlashCommandPositions()` |
| ratatui element overlay | codex | `codex-rs/tui/src/bottom_pane/textarea.rs` line 1994 — 3-layer rendering |
| decorateText ANSI injection | oh-my-pi | `packages/tui/src/components/editor.ts` line 336 |
| SyntaxStyle from theme rules | opencode | `packages/opencode/src/cli/cmd/tui/context/theme.tsx` |

---

## 8. Test Cases

### Happy Path Tests

```rust
#[test]
fn highlight_slash_command_in_input() {
    let highlights = find_input_highlights("/help foo", &COMMANDS, &[]);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].start, 0);
    assert_eq!(highlights[0].end, 5); // "/help"
}

#[test]
fn highlight_mid_input_slash_command() {
    let highlights = find_input_highlights("fix auth /review code", &COMMANDS, &[]);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].start, 9);
    assert_eq!(highlights[0].end, 16); // "/review"
}

#[test]
fn highlight_valid_skill_reference() {
    let skills = vec!["my-skill".to_string()];
    let highlights = find_input_highlights("run $my-skill now", &[], &skills);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].start, 4);
    assert_eq!(highlights[0].end, 13); // "$my-skill"
}

#[test]
fn no_highlight_unknown_slash_command() {
    let highlights = find_input_highlights("/unknown arg", &COMMANDS, &[]);
    assert!(highlights.is_empty());
}

#[test]
fn no_highlight_unknown_skill() {
    let highlights = find_input_highlights("$noskill", &[], &[]);
    assert!(highlights.is_empty());
}
```

### Edge Cases

```rust
#[test]
fn highlight_empty_input() {
    let highlights = find_input_highlights("", &COMMANDS, &[]);
    assert!(highlights.is_empty());
}

#[test]
fn highlight_only_slash() {
    let highlights = find_input_highlights("/", &COMMANDS, &[]);
    assert!(highlights.is_empty()); // bare "/" is not a command
}

#[test]
fn highlight_only_dollar() {
    let highlights = find_input_highlights("$", &[], &[]);
    assert!(highlights.is_empty()); // bare "$" is not a skill
}

#[test]
fn highlight_multiple_tokens() {
    let skills = vec!["foo".to_string()];
    let highlights = find_input_highlights("/help and $foo bar", &COMMANDS, &skills);
    assert_eq!(highlights.len(), 2);
}

#[test]
fn highlight_multiline_input() {
    let highlights = find_input_highlights("line1\n/help foo", &COMMANDS, &[]);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].start, 6); // after \n
}

#[test]
fn styled_segment_spans_splits_correctly() {
    let segment = WrappedInputSegment {
        text: "hello /help world".to_string(),
        start_char: 0,
        end_char: 17,
        display_width: 17,
    };
    let highlights = vec![InputHighlight {
        start: 6,
        end: 11,
        color: Color::Blue,
        priority: 5,
    }];
    let spans = styled_segment_spans(&segment, &highlights);
    assert_eq!(spans.len(), 3); // "hello ", "/help", " world"
}
```

### Integration Tests

```rust
#[test]
fn wrap_input_text_applies_highlights() {
    let highlights = vec![InputHighlight {
        start: 0, end: 5, color: Color::Blue, priority: 5,
    }];
    let (lines, _, _) = wrap_input_text(
        "/help foo", 0, 80, "1", ">", Color::White, 2, &highlights,
    );
    let first_line = &lines[0];
    // Should have: num_span, prompt_span, styled_span("/help"), raw_span(" foo")
    assert!(first_line.spans.len() >= 4);
}
```

---

## 9. Benchmarks

### What to Measure

| Metric | Baseline | Target | How to Measure |
|--------|----------|--------|----------------|
| Input render latency | ~0ms (Span::raw) | <0.5ms | `std::time::Instant` around `find_input_highlights` |
| Regex match time | N/A | <0.1ms per keystroke | Benchmark with 100-char input |
| Memory delta | 0 | <1KB per frame | Vec<InputHighlight> allocation |

### Notes
- `find_input_highlights()` runs on every frame (60fps). Must be fast.
- Regex compilation should be cached (use `once_cell::sync::Lazy` or `std::sync::OnceLock`).
- The highlight Vec is small (typically 1-3 entries) — no performance concern.

---

## 10. Migration / Rollout

- **No breaking changes** — this is a pure visual enhancement.
- **No feature flag needed** — highlighting is always on for recognized tokens.
- **Fallback** — if regex or skill lookup fails, `Span::raw()` is used (existing behavior).

---

## 11. Known Limitations & Future Work

- [ ] Only highlights valid/known commands and skills (unknown tokens stay plain)
- [ ] No ghost text / inline autocomplete (separate feature)
- [ ] No shimmer/animation effects (could add later for special keywords like claude-code's ultrathink)
- [ ] No highlighting of `@file` references (future enhancement)
- [ ] Regex patterns are basic — may need refinement for edge cases like `/cmd:subcmd`

---

## 12. Success Criteria Checklist

- [ ] `/help`, `/exit`, `/quit`, and all `REGISTERED_COMMANDS` are highlighted in blue when typed
- [ ] Valid `$skill` references are highlighted in accent color
- [ ] Unknown `/foo` and `$bar` remain unstyled (plain text)
- [ ] Mid-input tokens are highlighted (not just first-word)
- [ ] Multi-line input works correctly
- [ ] Cursor positioning is unaffected
- [ ] No performance regression (input stays responsive at 60fps)
- [ ] All existing tests pass
- [ ] New unit tests for `find_input_highlights()` and `styled_segment_spans()`
