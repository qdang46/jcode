export const meta = {
  name: 'implement-input-highlighting',
  description: 'Implement syntax highlighting for /slash and $skill in jcode TUI input area',
  phases: [
    { title: 'Read', detail: 'Read current source files to understand exact insertion points' },
    { title: 'Implement', detail: 'Add highlight types, finder functions, and modify rendering pipeline' },
    { title: 'Test', detail: 'Add unit tests for highlight logic' },
    { title: 'Verify', detail: 'Build and run tests to verify' },
  ],
}

phase('Read')

const uiInputCode = await agent(
  `Read crates/jcode-tui/src/tui/ui_input.rs. Report:
1. Exact code lines 1-14 (imports)
2. Exact code lines 20-32 (ComposerMode enum + impl)
3. Exact code lines 1892-1968 (WrappedInputSegment + wrap_input_segments)
4. Exact code lines 2082-2132 (wrap_input_text full function)
5. Exact code lines 2108-2121 (the Span::raw lines inside wrap_input_text)
6. Exact code lines 1760-1770 (wrap_input_text call site in draw_input)
7. Check if regex is in Cargo.toml of jcode-tui
8. Check what methods exist on app: &dyn TuiState for getting skill names`,
  { label: 'read-ui-input', phase: 'Read' }
)

const stateHelpers = await agent(
  `Read crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs. Report:
1. RegisteredCommand struct definition
2. First 20 entries of REGISTERED_COMMANDS
3. active_dollar_token() function
4. Visibility (pub/pub(crate)/pub(super)) of REGISTERED_COMMANDS`,
  { label: 'read-state-helpers', phase: 'Read' }
)

const tuiMod = await agent(
  `Read crates/jcode-tui/src/tui/mod.rs. Report:
1. TuiState trait — all method signatures
2. Is there available_skill_names() or similar?
3. How accent_color() is defined`,
  { label: 'read-tui-mod', phase: 'Read' }
)

const cargoToml = await agent(
  `Check if regex is a dependency in crates/jcode-tui/Cargo.toml and workspace root Cargo.toml`,
  { label: 'read-cargo', phase: 'Read' }
)

phase('Implement')

log('Step 1: Add InputHighlight type')

await agent(
  `Edit crates/jcode-tui/src/tui/ui_input.rs.

Add this struct AFTER the closing brace of "impl ComposerMode" (around line 32):

#[derive(Clone, Debug, PartialEq, Eq)]
struct InputHighlight {
    start: usize,
    end: usize,
    color: Color,
    priority: u8,
}

Do NOT change any other code. Only add this struct.`,
  { label: 'add-type', phase: 'Implement' }
)

log('Step 2: Add helper functions')

await agent(
  `Edit crates/jcode-tui/src/tui/ui_input.rs.

Add these three functions BEFORE the wrap_input_text function (before line 2082). Place them after cursor_col_for_segment function:

fn char_slice(s: &str, start: usize, end: usize) -> &str {
    let byte_start = s.char_indices().nth(start).map(|(i, _)| i).unwrap_or(s.len());
    let byte_end = s.char_indices().nth(end).map(|(i, _)| i).unwrap_or(s.len());
    &s[byte_start..byte_end]
}

fn find_input_highlights(
    input: &str,
    registered_commands: &[crate::tui::app::state_ui_input_helpers::RegisteredCommand],
    known_skills: &[String],
) -> Vec<InputHighlight> {
    use std::sync::OnceLock;
    static SLASH_RE: OnceLock<regex::Regex> = OnceLock::new();
    static DOLLAR_RE: OnceLock<regex::Regex> = OnceLock::new();

    let slash_re = SLASH_RE.get_or_init(|| {
        regex::Regex::new(r"(^|[\s])(/[a-zA-Z][a-zA-Z0-9:\-_]*)").unwrap()
    });
    let dollar_re = DOLLAR_RE.get_or_init(|| {
        regex::Regex::new(r"(?:^|\s)(\$[a-zA-Z0-9_-]+)").unwrap()
    });

    let mut highlights = Vec::new();
    let suggestion_color = rgb(100, 180, 255);

    let trimmed = input.trim_start();
    let offset = input.len() - trimmed.len();

    if trimmed.starts_with('/') {
        let cmd_end = trimmed[1..]
            .find(|c: char| c.is_whitespace())
            .unwrap_or(trimmed.len() - 1);
        let cmd_name = &trimmed[..=cmd_end];
        if registered_commands.iter().any(|c| c.name == cmd_name) {
            highlights.push(InputHighlight {
                start: offset,
                end: offset + cmd_name.len(),
                color: suggestion_color,
                priority: 5,
            });
        }
    } else {
        for cap in slash_re.captures_iter(input) {
            let preceding = cap.get(1).unwrap();
            let cmd = cap.get(2).unwrap();
            if registered_commands.iter().any(|c| c.name == cmd.as_str()) {
                highlights.push(InputHighlight {
                    start: preceding.end(),
                    end: cmd.end(),
                    color: suggestion_color,
                    priority: 5,
                });
            }
        }
    }

    for cap in dollar_re.captures_iter(input) {
        let token = cap.get(1).unwrap();
        let skill_name = &token.as_str()[1..];
        if known_skills.iter().any(|s| s == skill_name) {
            highlights.push(InputHighlight {
                start: token.start(),
                end: token.end(),
                color: accent_color(),
                priority: 5,
            });
        }
    }

    highlights
}

fn styled_segment_spans<'a>(
    segment: &WrappedInputSegment,
    highlights: &[InputHighlight],
) -> Vec<Span<'a>> {
    let seg_start = segment.start_char;
    let seg_end = segment.end_char;

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

        if pos < hl_start {
            let text = char_slice(&segment.text, pos - seg_start, hl_start - seg_start);
            spans.push(Span::raw(text.to_string()));
        }

        let text = char_slice(&segment.text, hl_start - seg_start, hl_end - seg_start);
        spans.push(Span::styled(text.to_string(), Style::default().fg(hl.color)));

        pos = hl_end;
    }

    if pos < seg_end {
        let text = char_slice(&segment.text, pos - seg_start, seg_end - seg_start);
        spans.push(Span::raw(text.to_string()));
    }

    spans
}

Do NOT modify any existing functions.`,
  { label: 'add-helpers', phase: 'Implement' }
)

log('Step 3: Modify wrap_input_text')

await agent(
  `Edit crates/jcode-tui/src/tui/ui_input.rs to modify wrap_input_text.

1. Change its signature to add highlights parameter. Find:
pub(crate) fn wrap_input_text<'a>(
    input: &str,
    cursor_pos: usize,
    line_width: usize,
    num_str: &str,
    prompt_char: &'a str,
    caret_color: Color,
    prompt_len: usize,
) -> (Vec<Line<'a>>, usize, usize) {

Replace with:
pub(crate) fn wrap_input_text<'a>(
    input: &str,
    cursor_pos: usize,
    line_width: usize,
    num_str: &str,
    prompt_char: &'a str,
    caret_color: Color,
    prompt_len: usize,
    highlights: &[InputHighlight],
) -> (Vec<Line<'a>>, usize, usize) {

2. Inside the function, find:
        if idx == 0 {
            let num_color = rainbow_prompt_color(0);
            lines.push(Line::from(vec![
                Span::styled(num_str.to_string(), Style::default().fg(num_color)),
                Span::styled(prompt_char.to_string(), Style::default().fg(caret_color)),
                Span::raw(segment.text.clone()),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(prompt_len)),
                Span::raw(segment.text.clone()),
            ]));
        }

Replace with:
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
        }`,
  { label: 'modify-wrap-fn', phase: 'Implement' }
)

log('Step 4: Modify draw_input')

await agent(
  `Edit crates/jcode-tui/src/tui/ui_input.rs to modify draw_input.

Find the wrap_input_text call in draw_input (around line 1760):
    let (all_lines, cursor_line, cursor_col) = wrap_input_text(
        input_text,
        cursor_pos,
        line_width,
        &num_str,
        prompt_char,
        caret_color,
        prompt_len,
    );

BEFORE it, add:
    let highlights = {
        let commands = crate::tui::app::state_ui_input_helpers::REGISTERED_COMMANDS;
        let skills = app.available_skill_names().unwrap_or_default();
        find_input_highlights(input_text, commands, &skills)
    };

Then change the wrap_input_text call to:
    let (all_lines, cursor_line, cursor_col) = wrap_input_text(
        input_text,
        cursor_pos,
        line_width,
        &num_str,
        prompt_char,
        caret_color,
        prompt_len,
        &highlights,
    );

NOTE: If available_skill_names() does not exist on TuiState, check what method is available. You may need to use app.current_skills_snapshot() and get keys from it, or add available_skill_names() to the TuiState trait.`,
  { label: 'modify-draw', phase: 'Implement' }
)

log('Step 5: Ensure regex dependency')

await agent(
  `Check if regex is in crates/jcode-tui/Cargo.toml. If not, add "regex.workspace = true" under [dependencies].
Check if regex is in workspace root Cargo.toml. If not, add 'regex = "1"' under [workspace.dependencies].`,
  { label: 'add-regex', phase: 'Implement' }
)

phase('Test')

await agent(
  `Add unit tests to the existing mod tests block in crates/jcode-tui/src/tui/ui_input.rs.

Add these tests inside the existing mod tests:

#[test]
fn char_slice_extracts_substring() {
    assert_eq!(char_slice("hello world", 0, 5), "hello");
    assert_eq!(char_slice("hello world", 6, 11), "world");
    assert_eq!(char_slice("hello", 0, 0), "");
}

#[test]
fn find_highlights_slash_whole_input() {
    let cmds = crate::tui::app::state_ui_input_helpers::REGISTERED_COMMANDS;
    let highlights = find_input_highlights("/help foo", cmds, &[]);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].start, 0);
    assert_eq!(highlights[0].end, 5);
}

#[test]
fn find_highlights_mid_input_slash() {
    let cmds = crate::tui::app::state_ui_input_helpers::REGISTERED_COMMANDS;
    let highlights = find_input_highlights("fix auth /exit now", cmds, &[]);
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].start, 9);
    assert_eq!(highlights[0].end, 14);
}

#[test]
fn find_highlights_unknown_not_highlighted() {
    let cmds = crate::tui::app::state_ui_input_helpers::REGISTERED_COMMANDS;
    let highlights = find_input_highlights("/notacommand", cmds, &[]);
    assert!(highlights.is_empty());
}

#[test]
fn find_highlights_empty_input() {
    let highlights = find_input_highlights("", &[], &[]);
    assert!(highlights.is_empty());
}

#[test]
fn find_highlights_bare_slash() {
    let highlights = find_input_highlights("/", &[], &[]);
    assert!(highlights.is_empty());
}

#[test]
fn styled_segment_no_highlights() {
    let seg = WrappedInputSegment {
        text: "hello".to_string(),
        start_char: 0,
        end_char: 5,
        display_width: 5,
    };
    let spans = styled_segment_spans(&seg, &[]);
    assert_eq!(spans.len(), 1);
}

#[test]
fn styled_segment_splits_on_highlight() {
    let seg = WrappedInputSegment {
        text: "hello /help world".to_string(),
        start_char: 0,
        end_char: 17,
        display_width: 17,
    };
    let hl = vec![InputHighlight { start: 6, end: 11, color: Color::Blue, priority: 5 }];
    let spans = styled_segment_spans(&seg, &hl);
    assert_eq!(spans.len(), 3);
}

Make sure all tests compile. Fix any issues with RegisteredCommand field access or visibility.`,
  { label: 'add-tests', phase: 'Test' }
)

phase('Verify')

await agent(
  `Run cargo build -p jcode-tui to verify compilation. If errors occur, fix them.

Common issues to check:
1. available_skill_names() method — if it doesn't exist on TuiState, look for alternative methods
2. REGISTERED_COMMANDS visibility — may need to change to pub(crate)
3. Regex dependency missing
4. Test construction of RegisteredCommand — fields may be private

After build succeeds, run cargo test -p jcode-tui to verify all tests pass.

Report the final build and test output.`,
  { label: 'build-verify', phase: 'Verify' }
)

log('Done! Input syntax highlighting for /slash and $skill is implemented.')
