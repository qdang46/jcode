use super::{RunningItem, RunningItemKind, RunningItemStatus, RunningItemsState};
use crate::tui::color_support::rgb;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;
use std::time::Duration;

pub(super) fn draw_running_items(
    frame: &mut Frame,
    app: &dyn super::TuiState,
    area: Rect,
) {
    let items_state = app.running_items();
    if !items_state.visible || items_state.items.is_empty() {
        return;
    }

    let area = if area.height > 0 { area } else { return };
    let inner_w = area.width as usize;

    // Header line
    let header_style = Style::default().fg(rgb(100, 100, 110));
    let hint = " ↑/↓ to select · Enter to view ";
    let header = format!(
        "{:width$}{}",
        "  ⏺ main",
        hint,
        width = inner_w.saturating_sub(hint.chars().count())
    );

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(&header, header_style)));

    let selected = items_state.selected;
    let max_visible = area.height.saturating_sub(1) as usize;

    // Show items with scroll offset if needed
    let scroll_offset = if items_state.items.len() > max_visible {
        selected.saturating_sub(max_visible.saturating_sub(1) / 2)
    } else {
        0
    };

    for (display_idx, idx) in (scroll_offset..items_state.items.len())
        .take(max_visible)
        .enumerate()
    {
        let item = &items_state.items[idx];
        let is_selected = idx == selected;

        let (icon, icon_color) = item_icon_and_color(item);
        let status_color = item_status_color(item.status);

        let mut spans: Vec<Span> = Vec::new();

        // Selection indicator
        if is_selected {
            spans.push(Span::styled(
                "❯ ",
                Style::default().fg(rgb(80, 160, 255)).bold(),
            ));
        } else {
            spans.push(Span::styled(
                "  ",
                Style::default(),
            ));
        }

        // Status icon
        spans.push(Span::styled(
            format!("{} ", icon),
            Style::default().fg(icon_color),
        ));

        // Label
        let label_style = if is_selected {
            Style::default().fg(rgb(220, 220, 230)).bold()
        } else {
            Style::default().fg(rgb(180, 180, 190))
        };
        spans.push(Span::styled(&item.label, label_style));

        // Detail text (truncated)
        if let Some(detail) = &item.detail {
            let available = inner_w.saturating_sub(item.label.width() + 6);
            if available > 4 {
                let truncated = truncate_to_width(detail, available);
                spans.push(Span::styled(
                    format!(" {}", truncated),
                    Style::default().fg(rgb(120, 120, 130)),
                ));
            }
        }

        // Elapsed time (right-aligned)
        if let Some(elapsed) = item.elapsed {
            let elapsed_str = format_elapsed(elapsed);
            let line_w: usize = spans.iter().map(|s| s.content.width()).sum();
            let padding = inner_w.saturating_sub(line_w + elapsed_str.chars().count());
            if padding > 1 {
                spans.push(Span::raw(" ".repeat(padding)));
            }
            spans.push(Span::styled(
                elapsed_str,
                Style::default().fg(status_color),
            ));
        }

        lines.push(Line::from(spans));
    }

    let list = Paragraph::new(Vec::from_iter(lines))
        .block(Block::default().borders(Borders::NONE));
    frame.render_widget(list, area);
}

/// Draw the detail overlay for a selected running item.
pub(super) fn draw_running_item_detail(
    frame: &mut Frame,
    app: &dyn super::TuiState,
    area: Rect,
) {
    let items_state = app.running_items();
    let detail = match &items_state.detail {
        Some(d) => d,
        None => return,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(rgb(70, 70, 80)))
        .title(" Running Item Detail ")
        .title_alignment(Alignment::Center);

    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = detail.lines().map(|line| {
        Line::from(Span::styled(line.to_string(), Style::default().fg(rgb(200, 200, 210))))
    }).collect();

    let para = Paragraph::new(lines);
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

fn item_status_color(status: RunningItemStatus) -> Color {
    match status {
        RunningItemStatus::Running => rgb(80, 220, 100),
        RunningItemStatus::Completed => rgb(100, 180, 100),
        RunningItemStatus::Failed => rgb(255, 100, 100),
        RunningItemStatus::Stopped => rgb(200, 180, 80),
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
    let w = s.width();
    if w <= max_width {
        s.to_string()
    } else {
        let mut out = String::with_capacity(max_width);
        for c in s.chars() {
            if out.width() + c.width() > max_width.saturating_sub(1) {
                break;
            }
            out.push(c);
        }
        out.push('…');
        out
    }
}

impl RunningItem {
    /// Extract a rough text summary from the tool call arguments
    fn args_to_str(&self) -> String {
        // BatchSubcall items include args in id; strip leading mental-model noise.
        // The actual args are stored in the ToolCall - we just use the label as-is.
        String::new()
    }
}
