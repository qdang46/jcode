use ftui_layout::Flex;
use ftui_style::Rgb;
use ftui_style::{Ansi16, MonoColor};
use crate::tui::compat::StyleCompatExt;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ftui_core::geometry::Rect;
use ftui_layout::Constraint;
use ftui_render::frame::Frame;
use ftui_style::{Color, Style};
use ftui_text::text::{Line, Span, Text};
use ftui_text::wrap::WrapMode;
use ftui_widgets::block::Block;
use ftui_widgets::borders::Borders;
use ftui_widgets::paragraph::Paragraph;
use ftui_widgets::Widget;
pub use jcode_tui_usage_overlay::{UsageOverlayItem, UsageOverlayStatus, UsageOverlaySummary};

const PANEL_BG: Color = Color::Rgb(Rgb { r: 24, g: 28, b: 40 });
const PANEL_BORDER: Color = Color::Rgb(Rgb { r: 90, g: 95, b: 110 });
const PANEL_BORDER_ACTIVE: Color = Color::Rgb(Rgb { r: 120, g: 140, b: 190 });
const SECTION_BORDER: Color = Color::Rgb(Rgb { r: 70, g: 78, b: 94 });
const SELECTED_BG: Color = Color::Rgb(Rgb { r: 38, g: 42, b: 56 });
const MUTED: Color = Color::Rgb(Rgb { r: 140, g: 146, b: 163 });
const MUTED_DARK: Color = Color::Rgb(Rgb { r: 100, g: 106, b: 122 });
const OVERLAY_PERCENT_X: u16 = 88;
const OVERLAY_PERCENT_Y: u16 = 74;

#[derive(Debug, Clone)]
pub struct UsageOverlay {
    title: String,
    items: Vec<UsageOverlayItem>,
    filtered: Vec<usize>,
    selected: usize,
    filter: String,
    summary: UsageOverlaySummary,
}

pub enum OverlayAction {
    Continue,
    Close,
}

impl UsageOverlay {
    pub fn loading() -> Self {
        Self::new(
            " Usage ",
            vec![UsageOverlayItem::new(
                "loading",
                "Refreshing usage",
                "Fetching limits from connected providers",
                UsageOverlayStatus::Loading,
                vec![
                    "Fetching usage limits from all connected providers...".to_string(),
                    "".to_string(),
                    "This view will update automatically when the usage report returns."
                        .to_string(),
                ],
            )],
            UsageOverlaySummary::default(),
        )
    }

    pub fn from_progress(progress: &crate::usage::ProviderUsageProgress) -> Self {
        Self::from_provider_reports(
            &progress.results,
            !progress.done,
            progress.completed,
            progress.total,
            progress.from_cache,
        )
    }

    pub fn from_provider_reports(
        reports: &[crate::usage::ProviderUsage],
        refreshing: bool,
        completed: usize,
        total: usize,
        from_cache: bool,
    ) -> Self {
        let mut items: Vec<UsageOverlayItem> = reports.iter().map(provider_item).collect();

        if refreshing {
            let subtitle = if total > 0 {
                format!("Refreshing providers ({}/{})", completed.min(total), total)
            } else if from_cache {
                "Showing cached usage while refreshing providers".to_string()
            } else {
                "Fetching usage limits from connected providers".to_string()
            };
            items.push(UsageOverlayItem::new(
                "refreshing",
                "Refreshing usage",
                subtitle,
                UsageOverlayStatus::Loading,
                vec![
                    "## Live refresh".to_string(),
                    if from_cache {
                        "• Cached results are visible immediately.".to_string()
                    } else {
                        "• Waiting for provider responses.".to_string()
                    },
                    if total > 0 {
                        format!(
                            "• Completed {}/{} provider checks.",
                            completed.min(total),
                            total
                        )
                    } else {
                        "• Discovering connected providers.".to_string()
                    },
                    "• This panel updates as each provider returns.".to_string(),
                ],
            ));
        } else if items.is_empty() {
            items.push(UsageOverlayItem::new(
                "no-providers",
                "No connected providers",
                "Connect Claude or OpenAI OAuth to show usage limits",
                UsageOverlayStatus::Info,
                vec![
                    "## No usage sources found".to_string(),
                    "• No providers with OAuth credentials were found.".to_string(),
                    "• Use `/login claude` or `/login openai` to connect a provider.".to_string(),
                    "• Then run `/usage` again.".to_string(),
                ],
            ));
        }

        let mut summary = UsageOverlaySummary {
            provider_count: reports.len(),
            session_visible: false,
            ..UsageOverlaySummary::default()
        };
        for report in reports {
            match provider_status(report) {
                UsageOverlayStatus::Warning => summary.warning_count += 1,
                UsageOverlayStatus::Critical => summary.critical_count += 1,
                UsageOverlayStatus::Error => summary.error_count += 1,
                _ => {}
            }
        }

        let title = if refreshing {
            " Usage · refreshing "
        } else {
            " Usage "
        };
        Self::new(title, items, summary)
    }

    pub fn debug_memory_profile(&self) -> serde_json::Value {
        let items_estimate_bytes: usize = self.items.iter().map(estimate_item_bytes).sum();
        let filtered_estimate_bytes = self.filtered.capacity() * std::mem::size_of::<usize>();
        let filter_bytes = self.filter.capacity();
        let title_bytes = self.title.capacity();
        let total_estimate_bytes =
            items_estimate_bytes + filtered_estimate_bytes + filter_bytes + title_bytes;

        serde_json::json!({
            "items_count": self.items.len(),
            "filtered_count": self.filtered.len(),
            "selected": self.selected,
            "title_bytes": title_bytes,
            "filter_bytes": filter_bytes,
            "items_estimate_bytes": items_estimate_bytes,
            "filtered_estimate_bytes": filtered_estimate_bytes,
            "total_estimate_bytes": total_estimate_bytes,
        })
    }

    pub fn new(
        title: impl Into<String>,
        items: Vec<UsageOverlayItem>,
        summary: UsageOverlaySummary,
    ) -> Self {
        let mut overlay = Self {
            title: title.into(),
            items,
            filtered: Vec::new(),
            selected: 0,
            filter: String::new(),
            summary,
        };
        overlay.apply_filter();
        overlay
    }

    pub fn selected_item_title(&self) -> Option<&str> {
        self.selected_item().map(|item| item.title.as_str())
    }

    pub fn replace_preserving_view(&mut self, mut next: Self) {
        let selected_id = self.selected_item().map(|item| item.id.clone());
        next.filter = self.filter.clone();
        next.apply_filter();
        if let Some(selected_id) = selected_id
            && let Some(selected) = next
                .filtered
                .iter()
                .position(|item_idx| next.items[*item_idx].id == selected_id)
        {
            next.selected = selected;
        }
        *self = next;
    }

    pub fn selected_item_detail_text(&self) -> String {
        self.selected_item()
            .map(|item| item.detail_lines.join("\n"))
            .unwrap_or_default()
    }

    fn selected_item(&self) -> Option<&UsageOverlayItem> {
        self.filtered
            .get(self.selected)
            .and_then(|idx| self.items.get(*idx))
    }

    fn apply_filter(&mut self) {
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                jcode_tui_usage_overlay::item_matches_filter(item, &self.filter).then_some(idx)
            })
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn handle_overlay_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<OverlayAction> {
        match code {
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.apply_filter();
                    return Ok(OverlayAction::Continue);
                }
                return Ok(OverlayAction::Close);
            }
            KeyCode::Char('q') if !modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(OverlayAction::Close);
            }
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(OverlayAction::Close);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.filtered.len().saturating_sub(1);
                self.selected = (self.selected + 1).min(max);
            }
            KeyCode::PageUp | KeyCode::Char('K') => {
                self.selected = self.selected.saturating_sub(6);
            }
            KeyCode::PageDown | KeyCode::Char('J') => {
                let max = self.filtered.len().saturating_sub(1);
                self.selected = (self.selected + 6).min(max);
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.filtered.len().saturating_sub(1);
            }
            KeyCode::Backspace => {
                if self.filter.pop().is_some() {
                    self.apply_filter();
                }
            }
            KeyCode::Char(c)
                if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.filter.push(c);
                self.apply_filter();
            }
            _ => {}
        }
        Ok(OverlayAction::Continue)
    }

    pub fn render(&self, frame: &mut Frame) {
        let frame_area = Rect {
            x: 0,
            y: 0,
            width: frame.width(),
            height: frame.height(),
        };
        let area = centered_rect(OVERLAY_PERCENT_X, OVERLAY_PERCENT_Y, frame_area);

        let title = format!(" {} ", self.title);
        let block = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .border_style(Style::default().fg_compat(PANEL_BORDER));
        block.render(area, frame);

        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        let rows = Flex::vertical()
            .constraints([
                Constraint::Fixed(5),
                Constraint::Min(10),
                Constraint::Fixed(2),
            ])
            .split(inner);

        self.render_header(frame, rows[0]);

        let body = Flex::horizontal()
            .constraints([Constraint::Percentage(39.0), Constraint::Percentage(61.0)])
            .split(rows[1]);

        self.render_item_list(frame, body[0]);
        self.render_detail_pane(frame, body[1]);

        let footer = Paragraph::new(Text::from_line(Line::from_spans(vec![
            Span::styled("Focus ", Style::default().fg_compat(MUTED_DARK)),
            Span::styled(
                "Use this panel to compare provider headroom and reset times without cluttering the chat transcript.",
                Style::default().fg_compat(MUTED),
            ),
        ])));
        footer.render(rows[2], frame);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Usage overview ")
            .borders(Borders::ALL)
            .style(Style::default().bg_compat(PANEL_BG))
            .border_style(Style::default().fg_compat(SECTION_BORDER));
        let inner = block.inner(area);
        block.render(area, frame);

        let lines = vec![
            Line::from_spans(vec![
                Span::styled("Filter ", Style::default().fg_compat(MUTED_DARK)),
                Span::styled(
                    if self.filter.is_empty() {
                        "type provider or plan name".to_string()
                    } else {
                        self.filter.clone()
                    },
                    if self.filter.is_empty() {
                        Style::default().fg_compat(Color::Mono(MonoColor::Black)).italic()
                    } else {
                        Style::default().fg_compat(Color::Mono(MonoColor::White))
                    },
                ),
                Span::styled(
                    format!("  ·  {} results", self.filtered.len()),
                    Style::default().fg_compat(MUTED_DARK),
                ),
            ]),
            Line::from_spans(vec![
                metric_span(
                    "providers",
                    self.summary.provider_count,
                    Color::Rgb(Rgb { r: 111, g: 214, b: 181 }),
                ),
                Span::raw("  "),
                metric_span(
                    "watch",
                    self.summary.warning_count,
                    Color::Rgb(Rgb { r: 255, g: 196, b: 112 }),
                ),
                Span::raw("  "),
                metric_span(
                    "high",
                    self.summary.critical_count,
                    Color::Rgb(Rgb { r: 255, g: 146, b: 110 }),
                ),
                Span::raw("  "),
                metric_span(
                    "errors",
                    self.summary.error_count,
                    Color::Rgb(Rgb { r: 232, g: 134, b: 134 }),
                ),
                if self.summary.session_visible {
                    Span::styled("  · session included", Style::default().fg_compat(MUTED_DARK))
                } else {
                    Span::raw("")
                },
            ]),
        ];

        Paragraph::new(Text::from_lines(lines))
            .wrap(WrapMode::Word)
            .render(inner, frame);
    }

    fn render_item_list(&self, frame: &mut Frame, area: Rect) {
        let title = if self.filtered.is_empty() {
            " Sources ".to_string()
        } else {
            format!(" Sources ({}/{}) ", self.selected + 1, self.filtered.len())
        };
        let block = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .style(Style::default().bg_compat(PANEL_BG))
            .border_style(Style::default().fg_compat(PANEL_BORDER_ACTIVE));
        let inner = block.inner(area);
        block.render(area, frame);

        if self.filtered.is_empty() {
            Paragraph::new(Text::from_line(Line::styled("No usage items match the current filter.", Style::default().fg_compat(MUTED))))
                .wrap(WrapMode::Word)
                .render(inner, frame);
            return;
        }

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut selected_line = 0usize;
        for (visible_idx, item_idx) in self.filtered.iter().enumerate() {
            let item = &self.items[*item_idx];
            let selected = visible_idx == self.selected;
            if selected {
                selected_line = lines.len();
            }
            let title_style = if selected {
                Style::default().fg_compat(Color::Mono(MonoColor::White)).bg_compat(SELECTED_BG).bold()
            } else {
                Style::default().fg_compat(Color::Mono(MonoColor::White))
            };
            let subtitle_style = if selected {
                Style::default().fg_compat(MUTED).bg_compat(SELECTED_BG)
            } else {
                Style::default().fg_compat(MUTED)
            };
            let badge_style = Style::default()
                .fg_compat(item.status.color())
                .bg_compat(if selected { SELECTED_BG } else { PANEL_BG })
                .bold();
            let marker = if selected { "›" } else { " " };
            lines.push(Line::from_spans(vec![
                Span::styled(
                    format!("{} {} ", marker, item.status.icon()),
                    Style::default().fg_compat(item.status.color()).bg_compat(if selected {
                        SELECTED_BG
                    } else {
                        PANEL_BG
                    }),
                ),
                Span::styled(
                    truncate_with_ellipsis(&item.title, inner.width.saturating_sub(16) as usize),
                    title_style,
                ),
                Span::raw(" "),
                Span::styled(format!("[{}]", item.status.label()), badge_style),
            ]));
            lines.push(Line::from_spans(vec![Span::styled(
                format!("   {}", item.subtitle),
                subtitle_style,
            )]));
            lines.push(Line::from_spans(vec![]));
        }

        let visible_height = inner.height.max(1) as usize;
        let scroll = selected_line.saturating_sub(visible_height.saturating_sub(3));
        Paragraph::new(Text::from_lines(lines))
            .scroll((scroll.min(u16::MAX as usize) as u16, 0))
            .wrap(WrapMode::Word)
            .render(inner, frame);
    }

    fn render_detail_pane(&self, frame: &mut Frame, area: Rect) {
        let selected = self.selected_item();
        let title = selected
            .map(|item| format!(" {} · {} ", item.title, item.status.label()))
            .unwrap_or_else(|| " Usage details ".to_string());
        let border_color = selected
            .map(|item| item.status.color())
            .unwrap_or(PANEL_BORDER_ACTIVE);
        let block = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .style(Style::default().bg_compat(PANEL_BG))
            .border_style(Style::default().fg_compat(border_color));
        let inner = block.inner(area);
        block.render(area, frame);

        let lines: Vec<Line<'static>> = match selected {
            Some(item) => item
                .detail_lines
                .iter()
                .map(|line| {
                    if line.is_empty() {
                        Line::from_spans(vec![])
                    } else if let Some(rest) = line.strip_prefix("## ") {
                        Line::from_spans(vec![Span::styled(
                            format!("  {}", rest),
                            Style::default().fg_compat(Color::Mono(MonoColor::White)).bold(),
                        )])
                    } else if let Some(rest) = line.strip_prefix("• ") {
                        Line::from_spans(vec![
                            Span::styled("  • ", Style::default().fg_compat(MUTED_DARK)),
                            Span::styled(rest.to_string(), Style::default().fg_compat(MUTED)),
                        ])
                    } else {
                        Line::from_spans(vec![Span::styled(
                            line.clone(),
                            Style::default().fg_compat(MUTED),
                        )])
                    }
                })
                .collect(),
            None => vec![Line::from_spans(vec![Span::styled(
                "No usage item selected.",
                Style::default().fg_compat(MUTED),
            )])],
        };

        Paragraph::new(Text::from_lines(lines))
            .wrap(WrapMode::Word)
            .render(inner, frame);
    }
}

fn estimate_item_bytes(item: &UsageOverlayItem) -> usize {
    item.id.capacity()
        + item.title.capacity()
        + item.subtitle.capacity()
        + item
            .detail_lines
            .iter()
            .map(|value| value.capacity())
            .sum::<usize>()
}

fn hotkey(text: &'static str) -> Span<'static> {
    Span::styled(text, Style::default().fg_compat(Color::Mono(MonoColor::White)).bg_compat(Color::Ansi16(Ansi16::BrightBlack)))
}

fn metric_span(label: &'static str, value: usize, color: Color) -> Span<'static> {
    Span::styled(
        format!("{} {}", label, value),
        Style::default().fg_compat(color).bold(),
    )
}

fn provider_item(report: &crate::usage::ProviderUsage) -> UsageOverlayItem {
    let status = provider_status(report);
    let subtitle = provider_subtitle(report);
    UsageOverlayItem::new(
        report.provider_name.clone(),
        report.provider_name.clone(),
        subtitle,
        status,
        provider_detail_lines(report),
    )
}

fn provider_status(report: &crate::usage::ProviderUsage) -> UsageOverlayStatus {
    if report.error.is_some() {
        return UsageOverlayStatus::Error;
    }
    if report.hard_limit_reached {
        return UsageOverlayStatus::Critical;
    }
    let max_percent = report
        .limits
        .iter()
        .map(|limit| limit.usage_percent)
        .fold(0.0_f32, f32::max);
    if max_percent >= 90.0 {
        UsageOverlayStatus::Critical
    } else if max_percent >= 70.0 {
        UsageOverlayStatus::Warning
    } else if report.limits.is_empty() && report.extra_info.is_empty() {
        UsageOverlayStatus::Info
    } else {
        UsageOverlayStatus::Good
    }
}

fn provider_subtitle(report: &crate::usage::ProviderUsage) -> String {
    if let Some(error) = &report.error {
        return truncate_with_ellipsis(error, 72);
    }
    if report.hard_limit_reached {
        return "Hard limit reached".to_string();
    }
    let mut parts = Vec::new();
    if let Some(limit) = report
        .limits
        .iter()
        .max_by(|a, b| a.usage_percent.total_cmp(&b.usage_percent))
    {
        let mut part = format!(
            "{} {:.0}% used",
            limit.name,
            limit.usage_percent.clamp(0.0, 999.0)
        );
        if let Some(reset) = limit.resets_at.as_deref() {
            part.push_str(&format!(
                " · resets in {}",
                crate::usage::format_reset_time(reset)
            ));
        }
        parts.push(part);
    }
    if let Some((key, value)) = report.extra_info.first() {
        parts.push(format!("{}: {}", key, value));
    }
    if parts.is_empty() {
        "No usage data available".to_string()
    } else {
        truncate_with_ellipsis(&parts.join(" · "), 96)
    }
}

fn provider_detail_lines(report: &crate::usage::ProviderUsage) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push("## Status".to_string());
    if let Some(error) = &report.error {
        lines.push(format!("• Error: {}", error));
        lines.push("".to_string());
        lines.push("## Next steps".to_string());
        lines.push(
            "• Re-run `/usage` to retry after credentials or network issues are fixed.".to_string(),
        );
        if report.provider_name.to_lowercase().contains("openai") {
            lines.push("• Use `/login openai` if the token needs refreshing.".to_string());
        } else if report.provider_name.to_lowercase().contains("anthropic")
            || report.provider_name.to_lowercase().contains("claude")
        {
            lines.push("• Use `/login claude` if the token needs refreshing.".to_string());
        }
        return lines;
    }

    lines.push(format!("• {}", provider_status(report).label()));
    if report.hard_limit_reached {
        lines.push("• Hard limit reached.".to_string());
    }

    if !report.limits.is_empty() {
        lines.push("".to_string());
        lines.push("## Limits".to_string());
        for limit in &report.limits {
            let reset = limit
                .resets_at
                .as_deref()
                .map(crate::usage::format_reset_time)
                .map(|value| format!(" · resets in {}", value))
                .unwrap_or_default();
            lines.push(format!(
                "• {}  {}{}",
                limit.name,
                crate::usage::format_usage_bar(limit.usage_percent, 18),
                reset
            ));
        }
    }

    if !report.extra_info.is_empty() {
        lines.push("".to_string());
        lines.push("## Details".to_string());
        for (key, value) in &report.extra_info {
            lines.push(format!("• {}: {}", key, value));
        }
    }

    if report.limits.is_empty() && report.extra_info.is_empty() {
        lines.push("• No usage data available from this provider.".to_string());
    }

    lines
}

fn truncate_with_ellipsis(input: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = input.chars().collect();
    if chars.len() <= width {
        return input.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let mut out: String = chars.into_iter().take(width - 3).collect();
    out.push_str("...");
    out
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let percent_y_f = percent_y as f32;
    let percent_x_f = percent_x as f32;
    let popup = Flex::vertical()
        .constraints([
            Constraint::Percentage((100.0 - percent_y_f) / 2.0),
            Constraint::Percentage(percent_y_f),
            Constraint::Percentage((100.0 - percent_y_f) / 2.0),
        ])
        .split(area);
    Flex::horizontal()
        .constraints([
            Constraint::Percentage((100.0 - percent_x_f) / 2.0),
            Constraint::Percentage(percent_x_f),
            Constraint::Percentage((100.0 - percent_x_f) / 2.0),
        ])
        .split(popup[1])[1]
}
