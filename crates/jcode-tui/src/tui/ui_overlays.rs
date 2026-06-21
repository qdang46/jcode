use super::{
    accent_color, ai_color, ai_text, asap_color, clear_area, dim_color, get_grouped_changelog,
    header_icon_color, header_name_color, header_session_color, pending_color, queued_color, rgb,
    tool_color, user_bg, user_color, user_text,
};
use crate::tui::TuiState;
use crate::tui::info_widget::WidgetPlacement;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// Strip ANSI/VT escape sequences and control characters from text before
/// rendering it to the TUI.  Accepts a `&str` and returns an owned `String`
/// with `\x1b` (ESC, the escape character that starts all VT sequences) and
/// other common control characters (BEL, backspace, etc.) removed.
///
/// This is a defense-in-depth measure for strings that originate from
/// dcg-core (tool names, reasons, MCP server names) which are technically
/// trusted by the caller but could contain terminal-injection sequences if
/// influenced by external input (e.g. an MCP server name configured by the
/// user, or a poisoned prompt).
#[must_use]
#[allow(dead_code)]
pub fn sanitize_terminal_text(s: &str) -> String {
    s.chars()
        .filter(|&c| c >= ' ' || c == '\n' || c == '\t')
        .collect()
}

pub(super) fn draw_changelog_overlay(frame: &mut Frame, area: Rect, scroll: usize) {
    clear_area(frame, area);

    let groups = get_grouped_changelog();
    let mut lines: Vec<Line<'static>> = Vec::new();

    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "No changelog entries available.",
            Style::default().fg(dim_color()),
        )));
    } else {
        for group in &groups {
            let heading = match &group.released_at {
                Some(released_at) => format!("  {} · {}", group.version, released_at),
                None => format!("  {}", group.version),
            };
            lines.push(Line::from(Span::styled(
                heading,
                Style::default()
                    .fg(rgb(200, 200, 220))
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            for entry in &group.entries {
                lines.push(Line::from(vec![
                    Span::styled("    • ", Style::default().fg(dim_color())),
                    Span::styled(entry.clone(), Style::default().fg(rgb(170, 170, 185))),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    let total_lines = lines.len();
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = scroll.min(max_scroll);

    let scroll_info = if total_lines > visible_height {
        let pct = if max_scroll > 0 {
            (scroll * 100) / max_scroll
        } else {
            100
        };
        format!(" {}% ", pct)
    } else {
        String::new()
    };

    let title = format!(" Changelog {} ", scroll_info);
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(rgb(200, 200, 220))
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " Esc to close · mouse wheel/j/k scroll · Space/PageUp page ",
            Style::default().fg(dim_color()),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(dim_color()));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

pub(super) fn draw_help_overlay(frame: &mut Frame, area: Rect, scroll: usize, app: &dyn TuiState) {
    clear_area(frame, area);

    let section_style = Style::default()
        .fg(accent_color())
        .add_modifier(Modifier::BOLD);
    let cmd_style = Style::default().fg(rgb(230, 230, 240));
    let desc_style = Style::default().fg(rgb(150, 150, 165));
    let key_style = Style::default().fg(rgb(200, 180, 120));
    let sep_style = Style::default().fg(rgb(50, 50, 55));

    let mut lines: Vec<Line<'static>> = Vec::new();

    let separator = || -> Line<'static> {
        Line::from(Span::styled(
            "  ─────────────────────────────────────────────────",
            sep_style,
        ))
    };

    let help_entry = |cmd: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled("    ", Style::default()),
            Span::styled(cmd.to_string(), cmd_style),
            Span::styled("  ", Style::default()),
            Span::styled(desc.to_string(), desc_style),
        ])
    };

    let key_entry = |key: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled("    ", Style::default()),
            Span::styled(format!("{:<22}", key), key_style),
            Span::styled(desc.to_string(), desc_style),
        ])
    };

    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  Commands", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/help", "Show this help overlay"));
    lines.push(help_entry(
        "/help <command>",
        "Show details for one command",
    ));
    lines.push(help_entry("/model", "List or switch models"));
    lines.push(help_entry("/model <name>", "Switch to a different model"));
    lines.push(help_entry(
        "/provider-test-coverage",
        "Show live-test evidence for the current provider/model",
    ));
    lines.push(help_entry("/agents", "Configure models for agent roles"));
    lines.push(help_entry(
        "/effort <level>",
        "Set reasoning effort (none|low|medium|high|xhigh)",
    ));
    lines.push(help_entry(
        "/fast [on|off|status|default ...]",
        "Toggle fast mode",
    ));
    lines.push(help_entry(
        "/transport <mode>",
        "Set connection transport (auto|https|websocket)",
    ));
    lines.push(help_entry(
        "/alignment [status|centered|left]",
        "Show or persist text alignment preference",
    ));
    lines.push(help_entry("/config", "Show active configuration"));
    lines.push(help_entry("/config init", "Create default config file"));
    lines.push(help_entry("/config edit", "Open config in $EDITOR"));
    lines.push(help_entry("/dictate", "Run configured external dictation"));
    lines.push(help_entry(
        "/git [status]",
        "Show branch and working tree status for the repo",
    ));
    lines.push(help_entry(
        "/context",
        "Show the full session context snapshot",
    ));
    lines.push(help_entry(
        "/skills",
        "Show loaded skills and jcode-endorsed recommendations",
    ));
    lines.push(help_entry("/info", "Show session info and token usage"));
    lines.push(help_entry(
        "/keys",
        "Show keybinding conflicts with your terminal/OS",
    ));
    lines.push(help_entry("/usage", "Show connected provider usage limits"));
    lines.push(help_entry("/version", "Show version and build details"));
    lines.push(help_entry(
        "/changelog",
        "Show recent changes in this build",
    ));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  Session", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/clear", "Clear conversation and start fresh"));
    lines.push(help_entry(
        "/compact",
        "Summarize old messages to free context",
    ));
    lines.push(help_entry(
        "/rewind",
        "Show numbered history, /rewind N to rewind",
    ));
    lines.push(help_entry(
        "/fix",
        "Attempt recovery when model cannot continue",
    ));
    lines.push(help_entry(
        "/poke",
        "Poke model to resume with incomplete todos (on/off/status)",
    ));
    lines.push(help_entry(
        "/plan [goal]",
        "Draft a plan-only proposal in the side panel (no edits)",
    ));
    lines.push(help_entry(
        "/improve",
        "Autonomously improve the repo until returns diminish",
    ));
    lines.push(help_entry(
        "/improve resume",
        "Resume the last saved improve loop/plan",
    ));
    lines.push(help_entry(
        "/refactor",
        "Run a safe refactor loop with independent review",
    ));
    lines.push(help_entry(
        "/refactor resume",
        "Resume the last saved refactor loop/plan",
    ));
    lines.push(help_entry(
        "/splitview [on|off|status]",
        "Mirror the current chat in the side panel",
    ));
    lines.push(help_entry("/split", "Clone session into a new window"));
    lines.push(help_entry(
        "/transfer",
        "Open a fresh session with only compacted context + copied todos",
    ));
    lines.push(help_entry(
        "/workspace [status|on|off|add]",
        "Enable and manage the Niri-style session workspace",
    ));
    lines.push(help_entry(
        "/catchup [next|list]",
        "Jump to finished sessions and open a Catch Up brief",
    ));
    lines.push(help_entry(
        "/back",
        "Return to the previous Catch Up source session",
    ));
    lines.push(help_entry("/resume", "Browse and resume previous sessions"));
    lines.push(help_entry(
        "/catchup [next]",
        "Jump into finished sessions with a side-panel brief",
    ));
    lines.push(help_entry(
        "/back",
        "Return to the previous Catch Up session",
    ));
    lines.push(help_entry("/save [label]", "Bookmark session for /resume"));
    lines.push(help_entry(
        "/rename <name>|--clear",
        "Set or clear current session name",
    ));
    lines.push(help_entry(
        "/unsave",
        "Remove bookmark from current session",
    ));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  Memory & Swarm", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/memory [on|off]", "Toggle memory features"));
    lines.push(help_entry(
        "/test [claim]",
        "Run layered verification and produce proof",
    ));
    lines.push(help_entry(
        "/initiatives",
        "Open initiatives overview / resume an initiative",
    ));
    lines.push(help_entry("/swarm [on|off]", "Toggle swarm features"));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  Auth & Accounts", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/auth", "Show authentication status"));
    lines.push(help_entry(
        "/login [provider]",
        "Interactive or direct login",
    ));
    lines.push(help_entry(
        "/account",
        "Open combined Claude/OpenAI account picker",
    ));
    lines.push(help_entry(
        "/subscription",
        "Inspect jcode subscription scaffold",
    ));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  System", section_style)));
    lines.push(Line::from(""));
    lines.push(help_entry("/reload", "Reload to newer binary if available"));
    lines.push(help_entry(
        "/restart",
        "Restart with current binary (no build)",
    ));
    lines.push(help_entry(
        "/rebuild",
        "Full update (git pull + build + tests)",
    ));
    if app.is_remote_mode() {
        lines.push(help_entry("/client-reload", "Force reload client binary"));
        lines.push(help_entry("/server-reload", "Force reload server binary"));
        lines.push(help_entry(
            "/continue",
            "Continue every interrupted live session that would auto-resume",
        ));
    }
    lines.push(help_entry(
        "/debug-visual",
        "Enable visual debugging for TUI issues",
    ));
    lines.push(help_entry(
        "/permissions",
        "Show DCG permission mode and recent decisions",
    ));
    lines.push(help_entry("/exit", "Exit jcode (opencode TUI slash)"));
    lines.push(help_entry("/quit", "Alias for /exit"));

    let skills = app.available_skills();
    if !skills.is_empty() {
        lines.push(Line::from(""));
        lines.push(separator());
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled("  Skills", section_style)));
        lines.push(Line::from(""));
        for skill in &skills {
            lines.push(help_entry(&format!("${}", skill), "Activate skill"));
        }
    }

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  Navigation", section_style)));
    lines.push(Line::from(""));
    lines.push(key_entry("PageUp / PageDown", "Scroll history"));
    lines.push(key_entry("Up / Down", "Scroll history (when input empty)"));
    lines.push(key_entry("Ctrl+[ / Ctrl+]", "Jump between user prompts"));
    lines.push(key_entry(
        "Cmd/Super+K / J",
        "Jump to previous / next user prompt (macOS)",
    ));
    lines.push(key_entry("Ctrl+1..4", "Resize side panel to 25/50/75/100%"));
    lines.push(key_entry(
        "Ctrl+5..9",
        "Jump by recency (5 = 5th most recent)",
    ));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled(
        "  Diagrams & Diffs",
        section_style,
    )));
    lines.push(Line::from(""));
    lines.push(key_entry(
        crate::tui::keybind::side_panel_toggle_key_label(),
        "Toggle side panel (or diagram pane if empty)",
    ));
    lines.push(key_entry("Alt+T", "Toggle diagram position (side/top)"));
    lines.push(key_entry(
        "Alt+Shift+I",
        "Show/hide inline images (persists)",
    ));
    lines.push(key_entry("Ctrl+H / Ctrl+L", "Focus chat / diagram / diffs"));
    lines.push(key_entry(
        "Ctrl+Left / Right",
        "Cycle diagrams (when diagram focused)",
    ));
    lines.push(key_entry("h/j/k/l / arrows", "Pan diagram (when focused)"));
    lines.push(key_entry("[ / ]", "Zoom diagram (when focused)"));
    lines.push(key_entry("+ / -", "Resize diagram pane"));
    lines.push(key_entry(
        "Alt+G / /diff",
        "Cycle diff mode (Off/Inline/Pinned/File)",
    ));
    lines.push(key_entry("Shift+Tab", "Cycle favorited models"));

    lines.push(Line::from(""));
    lines.push(separator());
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("  Input & Editing", section_style)));
    lines.push(Line::from(""));
    lines.push(key_entry(
        "Ctrl+C / Ctrl+D",
        "Quit (press twice to confirm)",
    ));
    lines.push(key_entry("Ctrl+X", "Cut entire input line to clipboard"));
    lines.push(key_entry("Ctrl+E", "Edit prompt in $EDITOR"));
    lines.push(key_entry(
        "Ctrl+A",
        "Copy visible chat viewport plus nearby context",
    ));
    lines.push(key_entry("Ctrl+U", "Clear input line"));
    lines.push(key_entry("Ctrl+K", "Delete to end of input"));
    lines.push(key_entry(
        "Alt+Backspace / Alt+Delete",
        "Delete previous word in input",
    ));
    lines.push(key_entry(
        "Cmd/Super+Backspace / Delete",
        "Delete previous word in input",
    ));
    lines.push(key_entry(
        "Cmd/Super+Left / Right",
        "Move to start / end of input",
    ));
    lines.push(key_entry("Cmd/Super+Z", "Undo input edit"));
    lines.push(key_entry("Cmd/Super+X / V", "Cut input / paste clipboard"));
    lines.push(key_entry("Ctrl+S", "Stash / pop input (save for later)"));
    lines.push(key_entry("Ctrl+Backspace", "Delete previous word in input"));
    lines.push(key_entry("Ctrl+B / Ctrl+F", "Move by word left / right"));
    lines.push(key_entry("Ctrl+Left / Right", "Move by word left / right"));
    lines.push(key_entry(
        "Shift+Enter / Alt+Enter",
        "Insert newline in input",
    ));
    lines.push(key_entry(
        "Ctrl+Enter",
        "Use opposite send mode while processing",
    ));
    lines.push(key_entry("Ctrl+Up", "Retrieve pending message for editing"));
    lines.push(key_entry("Ctrl+Tab / Ctrl+T", "Toggle queue mode"));
    lines.push(key_entry("Ctrl+R", "Recover from missing tool outputs"));
    lines.push(key_entry(
        "Ctrl+V / Alt+V",
        "Paste clipboard (text or image)",
    ));
    lines.push(key_entry(
        "Alt+A",
        "Quick-copy visible chat viewport plus nearby context",
    ));
    lines.push(key_entry("Alt+Y", "Toggle chat selection/copy mode"));
    lines.push(key_entry("Alt+S", "Toggle typing scroll lock"));
    lines.push(key_entry("Ctrl+P", "Toggle auto-poke for incomplete todos"));
    lines.push(key_entry("Alt+Left / Right", "Cycle reasoning effort"));
    if let Some(label) = app.dictation_key_label() {
        lines.push(key_entry(&label, "Run configured dictation"));
    }

    lines.push(Line::from(""));

    let total_lines = lines.len();
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = scroll.min(max_scroll);

    let scroll_info = if total_lines > visible_height {
        let pct = if max_scroll > 0 {
            (scroll * 100) / max_scroll
        } else {
            100
        };
        format!(" {}% ", pct)
    } else {
        String::new()
    };

    let title = format!(" Help {} ", scroll_info);
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(rgb(200, 200, 220))
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::from(Span::styled(
            " Esc to close · mouse wheel/j/k scroll · Space/PageUp page · /help <cmd> for details ",
            Style::default().fg(dim_color()),
        )))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(dim_color()));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

pub(super) fn draw_model_status_overlay(
    frame: &mut Frame,
    area: Rect,
    scroll: usize,
    content: &str,
) {
    clear_area(frame, area);

    let title_style = Style::default()
        .fg(accent_color())
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(rgb(210, 210, 220));
    let dim_style = Style::default().fg(dim_color());

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled("  Model Status", title_style)));
    lines.push(Line::from(Span::styled(
        "  Live verification evidence for provider/model behavior in jcode",
        dim_style,
    )));
    lines.push(Line::from(""));

    for raw in content.lines() {
        if let Some(title) = raw.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(format!("  {title}"), title_style)));
        } else if let Some(title) = raw.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(format!("  {title}"), title_style)));
        } else if raw.trim().is_empty() {
            lines.push(Line::from(""));
        } else {
            lines.push(Line::from(Span::styled(
                format!("  {raw}"),
                model_status_line_style(raw, text_style),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↑/↓ scroll, PgUp/PgDn page, c copy report, q/Esc close",
        dim_style,
    )));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" /provider-test-coverage "),
        )
        .scroll((scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(paragraph, area);
}

fn model_status_line_style(raw: &str, default: Style) -> Style {
    // Reuse the same semantic classification the CLI uses so the TUI overlay
    // and `jcode provider-test-coverage` stay color-consistent.
    use crate::live_tests::CoverageLineStyle;
    match crate::live_tests::classify_provider_test_coverage_line(raw) {
        CoverageLineStyle::Title => Style::default()
            .fg(accent_color())
            .add_modifier(Modifier::BOLD),
        CoverageLineStyle::Pass => Style::default().fg(rgb(120, 220, 150)),
        CoverageLineStyle::Fail => Style::default().fg(rgb(240, 110, 110)),
        CoverageLineStyle::Warn => Style::default().fg(rgb(235, 190, 105)),
        CoverageLineStyle::Dim => Style::default().fg(dim_color()),
        CoverageLineStyle::Plain => default,
    }
}

pub(super) fn draw_debug_overlay(
    frame: &mut Frame,
    placements: &[WidgetPlacement],
    chunks: &[Rect],
) {
    if chunks.len() < 5 {
        return;
    }
    render_overlay_box(frame, chunks[0], "messages", Color::Red);
    render_overlay_box(frame, chunks[1], "queued", Color::Yellow);
    render_overlay_box(frame, chunks[2], "status", Color::Cyan);
    render_overlay_box(frame, chunks[3], "picker", Color::Magenta);
    render_overlay_box(frame, chunks[4], "input", Color::Green);
    if chunks.len() > 5 && chunks[5].height > 0 {
        render_overlay_box(frame, chunks[5], "donut", Color::Blue);
    }

    for placement in placements {
        let title = format!("widget:{}", placement.kind.as_str());
        render_overlay_box(frame, placement.rect, &title, Color::Magenta);
    }
}

fn render_overlay_box(frame: &mut Frame, area: Rect, title: &str, color: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(Span::styled(title.to_string(), Style::default().fg(color)));
    frame.render_widget(block, area);
}

pub(super) fn debug_palette_json() -> Option<serde_json::Value> {
    Some(serde_json::json!({
        "user_color": color_to_rgb(user_color()),
        "ai_color": color_to_rgb(ai_color()),
        "tool_color": color_to_rgb(tool_color()),
        "dim_color": color_to_rgb(dim_color()),
        "accent_color": color_to_rgb(accent_color()),
        "queued_color": color_to_rgb(queued_color()),
        "asap_color": color_to_rgb(asap_color()),
        "pending_color": color_to_rgb(pending_color()),
        "user_text": color_to_rgb(user_text()),
        "user_bg": color_to_rgb(user_bg()),
        "ai_text": color_to_rgb(ai_text()),
        "header_icon_color": color_to_rgb(header_icon_color()),
        "header_name_color": color_to_rgb(header_name_color()),
        "header_session_color": color_to_rgb(header_session_color()),
    }))
}

fn color_to_rgb(color: Color) -> Option<[u8; 3]> {
    match color {
        Color::Rgb(r, g, b) => Some([r, g, b]),
        Color::Indexed(n) if n >= 16 => {
            let (r, g, b) = crate::tui::color_support::indexed_to_rgb(n);
            Some([r, g, b])
        }
        _ => None,
    }
}

pub(super) fn draw_permission_dialog_overlay(
    frame: &mut Frame,
    area: Rect,
    app: &dyn crate::tui::TuiState,
) {
    // Clear the dialog area so overlay text is visible over the chat.
    let dialog_w = area.width.min(84);
    let dialog_x = area.x + (area.width.saturating_sub(dialog_w)) / 2;
    let dialog_area = Rect::new(dialog_x, area.y + 1, dialog_w, 8);
    clear_area(frame, dialog_area);

    let dim = Style::default().fg(rgb(100, 100, 110));
    let warn = Style::default().fg(rgb(235, 190, 105));
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let hlbg = Style::default().fg(rgb(20, 22, 26)).bg(accent_color());
    let sel = app.pending_permission_selected().unwrap_or(0);
    let tool = app
        .pending_permission_tool()
        .unwrap_or("unknown")
        .to_string();

    // Worker badge: show session_id if different from current
    let current_sid = app.current_session_id().unwrap_or_default();
    let req_sid = app.pending_permission_session_id().unwrap_or("");
    let title_suffix = if !req_sid.is_empty() && req_sid != current_sid {
        format!(" [session: {}]", &req_sid[..req_sid.len().min(8)])
    } else {
        String::new()
    };
    let tool_display = format!("{}{}", tool, title_suffix);
    // Permission explainer: risk level
    let input = app.pending_permission_input();
    let (risk, _explanation) = if let Some(input) = input {
        crate::dcg_bridge::explain_tool_call(&tool, input)
    } else {
        (crate::dcg_bridge::RiskLevel::Low, String::new())
    };
    let risk_str = format!("{}", risk);
    let risk_color = match risk {
        crate::dcg_bridge::RiskLevel::Low => rgb(100, 180, 100),
        crate::dcg_bridge::RiskLevel::Medium => rgb(235, 190, 105),
        crate::dcg_bridge::RiskLevel::High => rgb(255, 100, 100),
    };

    let mut lines = match tool.as_str() {
        "bash" => {
            build_bash_permission_lines(app, dialog_w, tool_display, sel, &dim, &warn, &bold, &hlbg)
        }
        "edit" | "hashline_edit" => {
            build_edit_permission_lines(app, dialog_w, tool_display, sel, &dim, &warn, &bold, &hlbg)
        }
        "write" => build_write_permission_lines(
            app,
            dialog_w,
            tool_display,
            sel,
            &dim,
            &warn,
            &bold,
            &hlbg,
        ),
        _ => build_generic_permission_lines(
            app,
            dialog_w,
            tool_display,
            sel,
            &dim,
            &warn,
            &bold,
            &hlbg,
        ),
    };

    // Render risk badge in dialog
    let risk_text = if risk != crate::dcg_bridge::RiskLevel::Low {
        format!(" Risk: {} ", risk_str)
    } else {
        String::new()
    };
    if !risk_text.is_empty() {
        let risk_badge = Line::from(Span::styled(
            risk_text,
            Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
        ));
        // We insert at the end of the lines (before the option row)
        // Actually the lines already have the option row from build_* functions.
        // Let's just append a risk line before the last 2 lines (option + hint)
        if lines.len() >= 2 {
            lines.insert(lines.len() - 2, risk_badge);
        }
    }

    let pg = Paragraph::new(lines).block(Block::default().borders(Borders::NONE));
    frame.render_widget(pg, dialog_area);
}

/// Generic permission dialog (fallback). Shows tool name + reason.
#[allow(clippy::too_many_arguments)]
fn build_generic_permission_lines(
    app: &dyn crate::tui::TuiState,
    dialog_w: u16,
    tool: String,
    sel: usize,
    dim: &Style,
    warn: &Style,
    bold: &Style,
    hlbg: &Style,
) -> Vec<Line<'static>> {
    let reason = app.pending_permission_reason().unwrap_or("");
    let iw = dialog_w.saturating_sub(4) as usize;
    let title = format!(" Permission request: {} ", tool);
    let sep = "─".repeat(iw.saturating_sub(title.chars().count()) / 2);
    let top = format!(
        "╭{}{}{}╮",
        sep,
        title,
        "─".repeat(iw.saturating_sub(title.chars().count()) - sep.len())
    );

    let mut lines = vec![Line::from(Span::styled(top, *warn))];
    let r_text = if reason.len() > iw {
        format!("{}…", &reason[..iw.saturating_sub(1)])
    } else {
        reason.to_string()
    };
    lines.push(Line::from(vec![
        Span::styled(format!("  {}  ", tool), *bold),
        Span::styled(format!("({})", r_text), *dim),
    ]));
    lines.push(Line::from(""));
    append_option_row(&mut lines, sel, dim, hlbg);
    lines
}

/// Bash permission dialog — shows the actual command being run.
#[allow(clippy::too_many_arguments)]
fn build_bash_permission_lines(
    app: &dyn crate::tui::TuiState,
    dialog_w: u16,
    tool: String,
    sel: usize,
    dim: &Style,
    warn: &Style,
    _bold: &Style,
    hlbg: &Style,
) -> Vec<Line<'static>> {
    let iw = dialog_w.saturating_sub(4) as usize;
    let title = format!(" Permission: {} ", tool);

    // Extract command from input
    let command = app
        .pending_permission_input()
        .and_then(|v| v.get("command"))
        .and_then(|v| v.as_str())
        .unwrap_or("(unknown)");
    // Simplify command display (first word + ... for long commands)
    let display_cmd = if command.len() > iw {
        format!("{}…", &command[..iw.saturating_sub(1)])
    } else {
        command.to_string()
    };

    // Get the reason
    let reason = app.pending_permission_reason().unwrap_or("");

    let sep = "─".repeat(iw.saturating_sub(title.chars().count()) / 2);
    let top = format!(
        "╭{}{}{}╮",
        sep,
        title,
        "─".repeat(iw.saturating_sub(title.chars().count()) - sep.len())
    );

    let mut lines = vec![Line::from(Span::styled(top, *warn))];

    // Command line (monospace-style)
    lines.push(Line::from(vec![Span::styled(
        format!("  $ {}  ", display_cmd),
        Style::default().fg(rgb(80, 220, 100)),
    )]));

    // Reason line
    if !reason.is_empty() {
        let reason_text = if reason.len() > iw {
            format!("{}…", &reason[..iw.saturating_sub(1)])
        } else {
            reason.to_string()
        };
        lines.push(Line::from(Span::styled(
            format!("  ({})", reason_text),
            *dim,
        )));
    }

    lines.push(Line::from(""));
    append_option_row(&mut lines, sel, dim, hlbg);
    lines
}

/// Edit permission dialog — shows file path + inline diff.
#[allow(clippy::too_many_arguments)]
fn build_edit_permission_lines(
    app: &dyn crate::tui::TuiState,
    dialog_w: u16,
    tool: String,
    sel: usize,
    dim: &Style,
    warn: &Style,
    _bold: &Style,
    hlbg: &Style,
) -> Vec<Line<'static>> {
    let iw = dialog_w.saturating_sub(4) as usize;
    let title = format!(" Permission: {} ", tool);
    let sep = "─".repeat(iw.saturating_sub(title.chars().count()) / 2);
    let top = format!(
        "╭{}{}{}╮",
        sep,
        title,
        "─".repeat(iw.saturating_sub(title.chars().count()) - sep.len())
    );

    let input = app.pending_permission_input();
    let file_path = input
        .and_then(|v| v.get("file_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("(unknown)");
    let old_string = input
        .and_then(|v| v.get("old_string"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new_string = input
        .and_then(|v| v.get("new_string"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut lines = vec![Line::from(Span::styled(top, *warn))];

    // File path line
    let display_path = if file_path.len() > iw {
        format!("…{}", &file_path[file_path.len().saturating_sub(iw - 1)..])
    } else {
        file_path.to_string()
    };
    lines.push(Line::from(Span::styled(
        format!("  File: {}", display_path),
        Style::default().fg(rgb(100, 180, 255)),
    )));

    // Inline diff (show old → new)
    if !old_string.is_empty() || !new_string.is_empty() {
        let old_display = if old_string.len() > iw.saturating_sub(4) {
            format!("{}…", &old_string[..iw.saturating_sub(5)])
        } else {
            old_string.to_string()
        };
        let new_display = if new_string.len() > iw.saturating_sub(4) {
            format!("{}…", &new_string[..iw.saturating_sub(5)])
        } else {
            new_string.to_string()
        };
        lines.push(Line::from(vec![Span::styled(
            format!("  ─ {}", old_display),
            Style::default().fg(rgb(255, 100, 100)),
        )]));
        lines.push(Line::from(vec![Span::styled(
            format!("  + {}", new_display),
            Style::default().fg(rgb(80, 220, 100)),
        )]));
    }

    lines.push(Line::from(""));
    append_option_row(&mut lines, sel, dim, hlbg);
    lines
}

/// Write permission dialog — shows file path + content preview.
#[allow(clippy::too_many_arguments)]
fn build_write_permission_lines(
    app: &dyn crate::tui::TuiState,
    dialog_w: u16,
    tool: String,
    sel: usize,
    dim: &Style,
    warn: &Style,
    _bold: &Style,
    hlbg: &Style,
) -> Vec<Line<'static>> {
    let iw = dialog_w.saturating_sub(4) as usize;
    let title = format!(" Permission: {} ", tool);
    let sep = "─".repeat(iw.saturating_sub(title.chars().count()) / 2);
    let top = format!(
        "╭{}{}{}╮",
        sep,
        title,
        "─".repeat(iw.saturating_sub(title.chars().count()) - sep.len())
    );

    let input = app.pending_permission_input();
    let file_path = input
        .and_then(|v| v.get("file_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("(unknown)");
    let content = input
        .and_then(|v| v.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut lines = vec![Line::from(Span::styled(top, *warn))];

    // File path
    let display_path = if file_path.len() > iw {
        format!("…{}", &file_path[file_path.len().saturating_sub(iw - 1)..])
    } else {
        file_path.to_string()
    };
    lines.push(Line::from(Span::styled(
        format!("  File: {}", display_path),
        Style::default().fg(rgb(100, 180, 255)),
    )));

    // Content preview (first line, truncated)
    let preview = content.lines().next().unwrap_or("");
    let preview_truncated = if preview.len() > iw.saturating_sub(6) {
        format!("{}…", &preview[..iw.saturating_sub(7)])
    } else {
        preview.to_string()
    };
    if !preview_truncated.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  ┊ {}", preview_truncated),
            Style::default().fg(rgb(200, 200, 210)),
        )));
    }

    lines.push(Line::from(""));
    append_option_row(&mut lines, sel, dim, hlbg);
    lines
}

/// Append the approve/deny option row to any dialog.
fn append_option_row(lines: &mut Vec<Line<'static>>, sel: usize, dim: &Style, hlbg: &Style) {
    let opts = [
        "\u{2714} Approve",
        "\u{1f513} Approve all",
        "\u{1f4be} Always allow",
        "\u{2716} Deny",
    ];
    let mut opt_line = Vec::new();
    for (i, label) in opts.iter().enumerate() {
        if i == sel {
            opt_line.push(Span::styled(format!(" ❯{} ", label), *hlbg));
        } else {
            opt_line.push(Span::styled(
                format!("  ◯{} ", label),
                Style::default().fg(rgb(180, 180, 190)),
            ));
        }
    }
    lines.push(Line::from(opt_line));
    lines.push(Line::from(Span::styled(
        "  \u{2190}\u{2192} navigate  \u{23ce} Enter  Esc reject",
        *dim,
    )));
}

/// Draw the teammate view panel — shows the teammate's live status, detail,
/// and real-time updates in a dedicated overlay (CCB teammate view style).
pub(super) fn draw_teammate_view_panel(
    frame: &mut Frame,
    area: Rect,
    app: &dyn crate::tui::TuiState,
) {
    let sid = match app.viewing_teammate_session_id() {
        Some(s) => s,
        None => return,
    };
    if area.width < 30 || area.height < 5 {
        return;
    }

    let items = app.running_items();
    let teammate = items
        .items
        .iter()
        .find(|i| i.session_id.as_deref() == Some(sid));
    let (label, detail, status) = match teammate {
        Some(t) => (
            t.label.clone(),
            t.detail.clone().unwrap_or_default(),
            t.status,
        ),
        None => return,
    };

    let dim = Style::default().fg(rgb(100, 100, 110));
    let iw = area.width.saturating_sub(4) as usize;
    let title = format!(" Viewing: {} ", label);
    let top = format!(
        "╭{}\u{2500}{}\u{2500}╮",
        "─".repeat(4),
        "─".repeat(iw.saturating_sub(title.chars().count()))
    );

    let status_label = match status {
        crate::tui::RunningItemStatus::Running => "running",
        crate::tui::RunningItemStatus::Completed => "completed",
        crate::tui::RunningItemStatus::Failed => "failed",
        crate::tui::RunningItemStatus::Stopped => "stopped",
    };
    let status_color = match status {
        crate::tui::RunningItemStatus::Running => rgb(80, 220, 100),
        crate::tui::RunningItemStatus::Completed => rgb(100, 180, 100),
        crate::tui::RunningItemStatus::Failed => rgb(255, 100, 100),
        crate::tui::RunningItemStatus::Stopped => rgb(200, 180, 80),
    };

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(top, dim)),
        Line::from(Span::styled(
            format!("  Status: {}", status_label),
            Style::default().fg(status_color),
        )),
    ];

    if !detail.is_empty() {
        let dt = if detail.len() > iw {
            format!("{}…", &detail[..iw.saturating_sub(1)])
        } else {
            detail
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", dt),
            Style::default().fg(rgb(200, 200, 210)),
        )));
    }

    lines.push(Line::from(Span::styled(
        format!("  session: {}", sid),
        Style::default().fg(rgb(140, 140, 150)),
    )));
    lines.push(Line::from(Span::styled(
        "  ─────────────────",
        Style::default().fg(rgb(60, 65, 75)),
    )));
    lines.push(Line::from(Span::styled(
        "  Esc to exit · Ctrl+C to cancel",
        dim,
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(rgb(80, 160, 255)));

    let inner = block.inner(area);
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(block, area);
    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);
}
