use super::{RunningItem, RunningItemStatus};
use crate::tui::color_support::rgb;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

pub(super) fn draw_running_items(frame: &mut Frame, app: &dyn super::TuiState, area: Rect) {
    let items_state = app.running_items();
    if !items_state.visible || items_state.items.is_empty() {
        return;
    }
    if area.height == 0 {
        return;
    }

    let inner_w = area.width as usize;

    // Header line
    let header_style = Style::default().fg(rgb(100, 100, 110));
    let hint = if items_state.detail_open {
        " Esc to close "
    } else {
        " ↑/↓ to select · Enter to view "
    };
    let header = format!(
        "{:width$}{}",
        "  ⏺ main",
        hint,
        width = inner_w.saturating_sub(hint.chars().count())
    );

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(Span::styled(header, header_style)));

    let selected = items_state.selected;
    let max_visible = area.height.saturating_sub(1) as usize;

    let scroll_offset = if items_state.items.len() > max_visible {
        selected.saturating_sub(max_visible.saturating_sub(1) / 2)
    } else {
        0
    };

    for idx in (scroll_offset..items_state.items.len()).take(max_visible) {
        let item = &items_state.items[idx];
        let is_selected = idx == selected;

        let (icon, icon_color) = item_icon_and_color(item);

        let mut spans: Vec<Span<'static>> = Vec::new();

        if is_selected {
            spans.push(Span::styled(
                "❯ ",
                Style::default().fg(rgb(80, 160, 255)).bold(),
            ));
        } else {
            spans.push(Span::styled("  ", Style::default()));
        }

        spans.push(Span::styled(
            format!("{} ", icon),
            Style::default().fg(icon_color),
        ));

        let label_style = if is_selected {
            Style::default().fg(rgb(220, 220, 230)).bold()
        } else {
            Style::default().fg(rgb(180, 180, 190))
        };
        spans.push(Span::styled(item.label.clone(), label_style));

        if let Some(detail) = &item.detail {
            let available = inner_w.saturating_sub(UnicodeWidthStr::width(item.label.as_str()) + 6);
            if available > 4 {
                let truncated = truncate_to_width(detail, available);
                spans.push(Span::styled(
                    format!(" {}", truncated),
                    Style::default().fg(rgb(120, 120, 130)),
                ));
            }
        }

        if let Some(elapsed) = item.elapsed {
            let elapsed_str = format_elapsed(elapsed);
            let line_w: usize = spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            let padding = inner_w.saturating_sub(line_w + elapsed_str.chars().count());
            if padding > 1 {
                spans.push(Span::raw(" ".repeat(padding)));
            }
            spans.push(Span::styled(
                elapsed_str,
                Style::default().fg(rgb(120, 120, 130)),
            ));
        }

        lines.push(Line::from(spans));
    }

    let list = Paragraph::new(lines);
    frame.render_widget(list, area);
}

/// Draw a detail overlay showing LIVE info about the selected running item.
/// Content is rebuilt every frame so status/elapsed update in real-time.
pub(super) fn draw_running_item_detail(frame: &mut Frame, app: &dyn super::TuiState, area: Rect) {
    let items_state = app.running_items();
    if !items_state.detail_open {
        return;
    }
    if area.width < 20 || area.height < 3 {
        return;
    }

    let selected = items_state.selected;
    let item = match items_state.items.get(selected) {
        Some(i) => i,
        None => return,
    };

    let (icon, icon_color) = item_icon_and_color(item);
    let kind_label = match item.kind {
        super::RunningItemKind::BatchSubcall => "batch tool",
        super::RunningItemKind::BackgroundTask => "background task",
        super::RunningItemKind::Subagent => "subagent",
        super::RunningItemKind::SwarmMember => "swarm member",
    };
    let status_label = match item.status {
        RunningItemStatus::Running => "running",
        RunningItemStatus::Completed => "completed",
        RunningItemStatus::Failed => "failed",
        RunningItemStatus::Stopped => "stopped",
    };
    let status_color = match item.status {
        RunningItemStatus::Running => rgb(80, 220, 100),
        RunningItemStatus::Completed => rgb(100, 180, 100),
        RunningItemStatus::Failed => rgb(255, 100, 100),
        RunningItemStatus::Stopped => rgb(200, 180, 80),
    };

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Title
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
        Span::styled(
            item.label.clone(),
            Style::default().fg(rgb(220, 220, 230)).bold(),
        ),
    ]));

    // Status row (built every frame = real-time)
    let mut status_row = format!("  status: {} · kind: {}", status_label, kind_label);
    if let Some(elapsed) = item.elapsed {
        status_row.push_str(&format!(" · elapsed: {}", format_elapsed(elapsed)));
    }
    lines.push(Line::from(Span::styled(
        status_row,
        Style::default().fg(status_color),
    )));

    // Session ID
    if let Some(sid) = &item.session_id {
        lines.push(Line::from(Span::styled(
            format!("  session: {}", sid),
            Style::default().fg(rgb(120, 120, 130)),
        )));
    }

    // Detail text (live status detail from the item)
    if let Some(detail) = &item.detail {
        for line in detail.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(rgb(200, 200, 210)),
            )));
        }
    }

    // Separator
    lines.push(Line::from(Span::styled(
        "  ─────────────────",
        Style::default().fg(rgb(60, 65, 75)),
    )));

    // Action hints

    // Session action hint
    if item.session_id.is_some() {
        lines.push(Line::from(Span::styled(
            "  Enter to open session · Esc to close",
            Style::default().fg(rgb(80, 80, 90)),
        )));
    }
    if matches!(item.status, RunningItemStatus::Running) {
        lines.push(Line::from(Span::styled(
            "  Ctrl+C or Backspace to cancel · Esc to close",
            Style::default().fg(rgb(80, 80, 90)),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "  Esc to close",
            Style::default().fg(rgb(80, 80, 90)),
        )));
    }

    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(rgb(70, 70, 80)))
        .title(" Item Detail ")
        .title_alignment(ratatui::layout::Alignment::Center);

    let inner = block.inner(area);
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(block, area);

    let content_height = lines.len() as u16;
    let start_line = content_height.saturating_sub(inner.height);
    let visible: Vec<Line<'static>> = lines
        .into_iter()
        .skip(start_line as usize)
        .take(inner.height as usize)
        .collect();
    let para = Paragraph::new(visible);
    frame.render_widget(para, inner);
}

fn item_icon_and_color(item: &RunningItem) -> (&'static str, Color) {
    match item.status {
        RunningItemStatus::Running => ("◯", rgb(80, 220, 100)),
        RunningItemStatus::Completed => ("✓", rgb(100, 180, 100)),
        RunningItemStatus::Failed => ("✗", rgb(255, 100, 100)),
        RunningItemStatus::Stopped => ("■", rgb(200, 180, 80)),
    }
}

fn format_elapsed(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn truncate_to_width(s: &str, max_width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w <= max_width {
        s.to_string()
    } else {
        let mut out = String::with_capacity(max_width);
        for c in s.chars() {
            if UnicodeWidthStr::width(out.as_str()) + UnicodeWidthStr::width(c.to_string().as_str())
                > max_width.saturating_sub(1)
            {
                break;
            }
            out.push(c);
        }
        out.push('…');
        out
    }
}
