use ftui_style::MonoColor;
use crate::tui::compat::StyleCompatExt;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ftui_core::geometry::Rect;
use ftui_render::frame::Frame;
use ftui_style::{Color, Rgb, Style};
use ftui_text::text::{Line, Span, Text};
use ftui_widgets::Widget;
use ftui_layout::{Constraint, Flex};
use ftui_widgets::block::{Block};
use ftui_widgets::borders::{Borders};
use ftui_widgets::paragraph::Paragraph;
use ftui_text::wrap::WrapMode;
use std::collections::HashMap;

pub use jcode_tui_account_picker::{
    AccountPickerCommand, AccountPickerItem, AccountPickerSummary, AccountProviderKind,
};

#[path = "account_picker_render.rs"]
mod render_support;
use render_support::{
    ActionSection, account_count_summary, account_is_active, action_icon, action_kind_badge,
    action_kind_help, action_section, centered_rect, command_preview, compact_item_title, hotkey,
    metric_span, provider_header_line, provider_style, truncate_with_ellipsis,
};

const PANEL_BG: Color = Color::Rgb(Rgb::new(24, 28, 40));
const PANEL_BORDER: Color = Color::Rgb(Rgb::new(90, 95, 110));
const PANEL_BORDER_ACTIVE: Color = Color::Rgb(Rgb::new(120, 140, 190));
const SECTION_BORDER: Color = Color::Rgb(Rgb::new(70, 78, 94));
const SELECTED_BG: Color = Color::Rgb(Rgb::new(38, 42, 56));
const MUTED: Color = Color::Rgb(Rgb::new(140, 146, 163));
const MUTED_DARK: Color = Color::Rgb(Rgb::new(100, 106, 122));
const OVERLAY_PERCENT_X: u16 = 88;
const OVERLAY_PERCENT_Y: u16 = 74;

#[derive(Debug, Clone)]
pub struct AccountPicker {
    title: String,
    items: Vec<AccountPickerItem>,
    filtered: Vec<usize>,
    selected: usize,
    filter: String,
    summary: Option<AccountPickerSummary>,
    last_action_list_area: Option<Rect>,
}

pub enum OverlayAction {
    Continue,
    Close,
    Execute(AccountPickerCommand),
}

impl AccountPicker {
    pub fn new(title: impl Into<String>, items: Vec<AccountPickerItem>) -> Self {
        Self::with_summary(title, items, AccountPickerSummary::default())
    }

    pub fn debug_memory_profile(&self) -> serde_json::Value {
        let items_estimate_bytes: usize = self.items.iter().map(estimate_item_bytes).sum();
        let filtered_estimate_bytes = self.filtered.capacity() * std::mem::size_of::<usize>();
        let filter_bytes = self.filter.capacity();
        let title_bytes = self.title.capacity();
        let summary_estimate_bytes = self
            .summary
            .as_ref()
            .map(estimate_summary_bytes)
            .unwrap_or(0);
        let total_estimate_bytes = items_estimate_bytes
            + filtered_estimate_bytes
            + filter_bytes
            + title_bytes
            + summary_estimate_bytes;

        serde_json::json!({
            "items_count": self.items.len(),
            "filtered_count": self.filtered.len(),
            "selected": self.selected,
            "title_bytes": title_bytes,
            "filter_bytes": filter_bytes,
            "summary_estimate_bytes": summary_estimate_bytes,
            "items_estimate_bytes": items_estimate_bytes,
            "filtered_estimate_bytes": filtered_estimate_bytes,
            "total_estimate_bytes": total_estimate_bytes,
        })
    }

    pub fn with_summary(
        title: impl Into<String>,
        items: Vec<AccountPickerItem>,
        summary: AccountPickerSummary,
    ) -> Self {
        let mut picker = Self {
            title: title.into(),
            items,
            filtered: Vec::new(),
            selected: 0,
            filter: String::new(),
            summary: Some(summary),
            last_action_list_area: None,
        };
        picker.apply_filter();
        picker
    }

    fn selected_item(&self) -> Option<&AccountPickerItem> {
        self.filtered
            .get(self.selected)
            .and_then(|idx| self.items.get(*idx))
    }

    fn visible_window_start(&self, available_items: usize) -> usize {
        self.selected
            .saturating_sub(available_items.saturating_sub(1).min(available_items / 2))
    }

    fn visible_index_for_action_row(&self, row: u16, list_height: u16) -> Option<usize> {
        if self.filtered.is_empty() {
            return None;
        }

        let available_items = (list_height as usize).max(1);
        let start = self.visible_window_start(available_items);
        let end = (start + available_items).min(self.filtered.len());
        let mut current_provider: Option<&str> = None;
        let mut rendered_row = 0u16;

        for visible_idx in start..end {
            let item = &self.items[self.filtered[visible_idx]];
            if current_provider != Some(item.provider_id.as_str()) {
                current_provider = Some(item.provider_id.as_str());
                if rendered_row == row {
                    return None;
                }
                rendered_row = rendered_row.saturating_add(1);
                if rendered_row >= list_height {
                    return None;
                }
            }

            if rendered_row == row {
                return Some(visible_idx);
            }
            rendered_row = rendered_row.saturating_add(1);
            if rendered_row > row && rendered_row >= list_height {
                return None;
            }
        }

        None
    }

    fn apply_filter(&mut self) {
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                jcode_tui_account_picker::item_matches_filter(item, &self.filter).then_some(idx)
            })
            .collect();
        let provider_order = self.provider_order();
        self.filtered.sort_by(|left, right| {
            let left_item = &self.items[*left];
            let right_item = &self.items[*right];

            provider_order
                .get(&left_item.provider_id)
                .cmp(&provider_order.get(&right_item.provider_id))
                .then_with(|| action_section(left_item).cmp(&action_section(right_item)))
                .then_with(|| left_item.title.cmp(&right_item.title))
                .then_with(|| left.cmp(right))
        });
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    fn provider_order(&self) -> HashMap<String, usize> {
        let mut order = HashMap::new();
        let mut next = 0usize;
        for item in &self.items {
            if order.contains_key(&item.provider_id) {
                continue;
            }
            let rank = if item.provider_id == "defaults" {
                usize::MAX / 2
            } else {
                let current = next;
                next += 1;
                current
            };
            order.insert(item.provider_id.clone(), rank);
        }
        order
    }

    fn filtered_provider_switch_count(&self, provider_id: &str) -> usize {
        self.filtered
            .iter()
            .filter(|idx| {
                let item = &self.items[**idx];
                item.provider_id == provider_id
                    && matches!(action_section(item), ActionSection::Switch)
            })
            .count()
    }

    fn filtered_provider_secondary_count(&self, provider_id: &str) -> usize {
        self.filtered
            .iter()
            .filter(|idx| {
                let item = &self.items[**idx];
                item.provider_id == provider_id
                    && !matches!(action_section(item), ActionSection::Switch)
            })
            .count()
    }

    fn select_prev_provider_group(&mut self) {
        let Some(current_idx) = self.filtered.get(self.selected).copied() else {
            return;
        };
        let current_provider = self.items[current_idx].provider_id.as_str();
        let mut target = None;

        for pos in (0..self.selected).rev() {
            let provider_id = self.items[self.filtered[pos]].provider_id.as_str();
            if provider_id != current_provider {
                target = Some(pos);
                break;
            }
        }

        let Some(mut pos) = target else {
            return;
        };
        let provider_id = self.items[self.filtered[pos]].provider_id.clone();
        while pos > 0 && self.items[self.filtered[pos - 1]].provider_id == provider_id {
            pos -= 1;
        }
        self.selected = pos;
    }

    fn select_next_provider_group(&mut self) {
        let Some(current_idx) = self.filtered.get(self.selected).copied() else {
            return;
        };
        let current_provider = self.items[current_idx].provider_id.as_str();

        for pos in (self.selected + 1)..self.filtered.len() {
            let provider_id = self.items[self.filtered[pos]].provider_id.as_str();
            if provider_id != current_provider {
                self.selected = pos;
                break;
            }
        }
    }

    fn provider_overview_line(&self) -> Line<'static> {
        let mut seen = Vec::new();
        let mut stats: HashMap<String, (String, usize, usize)> = HashMap::new();

        for item in &self.items {
            if matches!(item.provider_id.as_str(), "defaults" | "account-flow") {
                continue;
            }
            if !stats.contains_key(&item.provider_id) {
                seen.push(item.provider_id.clone());
                stats.insert(
                    item.provider_id.clone(),
                    (item.provider_label.clone(), 0, 0),
                );
            }
            if let Some((_, accounts, actions)) = stats.get_mut(&item.provider_id) {
                if matches!(action_section(item), ActionSection::Switch) {
                    *accounts += 1;
                } else {
                    *actions += 1;
                }
            }
        }

        let mut spans = vec![Span::styled("Providers ", Style::new().fg_compat(MUTED_DARK))];
        let mut first = true;
        for provider_id in seen {
            let Some((label, accounts, actions)) = stats.get(&provider_id) else {
                continue;
            };
            if !first {
                spans.push(Span::styled(" | ", Style::new().fg_compat(MUTED_DARK)));
            }
            first = false;
            let summary = if *accounts > 0 {
                format!("{} {}", label, account_count_summary(*accounts))
            } else {
                format!(
                    "{} {} control{}",
                    label,
                    actions,
                    if *actions == 1 { "" } else { "s" }
                )
            };
            spans.push(Span::styled(summary, provider_style(&provider_id)));
        }
        if first {
            spans.push(Span::styled(
                "No providers available",
                Style::new().fg_compat(MUTED),
            ));
        }
        Line::from_spans(spans)
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
            KeyCode::Left => {
                self.select_prev_provider_group();
            }
            KeyCode::Right => {
                self.select_next_provider_group();
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
            KeyCode::Enter => {
                if let Some(item) = self.selected_item() {
                    return Ok(OverlayAction::Execute(item.command.clone()));
                }
                return Ok(OverlayAction::Close);
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

    pub fn handle_overlay_mouse(&mut self, mouse: MouseEvent) {
        let Some(list_inner) = self.last_action_list_area else {
            return;
        };
        let inside_list = mouse.column >= list_inner.x
            && mouse.column < list_inner.x.saturating_add(list_inner.width)
            && mouse.row >= list_inner.y
            && mouse.row < list_inner.y.saturating_add(list_inner.height);

        match mouse.kind {
            MouseEventKind::ScrollUp if inside_list => {
                self.selected = self.selected.saturating_sub(1);
            }
            MouseEventKind::ScrollDown if inside_list => {
                let max = self.filtered.len().saturating_sub(1);
                self.selected = (self.selected + 1).min(max);
            }
            MouseEventKind::Down(MouseButton::Left) if inside_list => {
                let row = mouse.row.saturating_sub(list_inner.y);
                if let Some(visible_idx) = self.visible_index_for_action_row(row, list_inner.height)
                {
                    self.selected = visible_idx;
                }
            }
            _ => {}
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = centered_rect(OVERLAY_PERCENT_X, OVERLAY_PERCENT_Y, Rect::new(0, 0, frame.buffer.width(), frame.buffer.height()));

        let title = format!(" {} ", self.title);
        let block = Block::new()
            .title(title.as_str())
            .borders(Borders::ALL)
            .border_style(Style::new().fg_compat(PANEL_BORDER));
        block.render(area, frame);

        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        let rows = Flex::vertical()
            .constraints([
                Constraint::Fixed(7),
                Constraint::Min(10),
                Constraint::Fixed(1),
            ])
            .split(inner);

        self.render_header(frame, rows[0]);

        let body = Flex::horizontal()
            .constraints([Constraint::Percentage(58.0), Constraint::Percentage(42.0)])
            .split(rows[1]);

        self.render_action_list(frame, body[0]);
        self.render_detail_pane(frame, body[1]);

        let footer = Paragraph::new(Text::from_line(Line::from_spans(vec![
            Span::styled("Focus ", Style::new().fg_compat(MUTED_DARK)),
            Span::styled(
                "saved accounts stay surfaced here; click actions to focus them, use Left/Right to jump provider groups, or use `/account <provider> settings` for the full text view.",
                Style::new().fg_compat(MUTED),
            ),
        ])));
        footer.render(rows[2], frame);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let block = Block::new()
            .title(" Overview ")
            .borders(Borders::ALL)
            .style(Style::new().bg_compat(PANEL_BG))
            .border_style(Style::new().fg_compat(SECTION_BORDER));
        let inner = block.inner(area);
        block.render(area, frame);

        let lines = vec![
            Line::from_spans(vec![
                Span::styled("Filter ", Style::new().fg_compat(MUTED_DARK)),
                Span::styled(
                    if self.filter.is_empty() {
                        "type provider or account name".to_string()
                    } else {
                        self.filter.clone()
                    },
                    if self.filter.is_empty() {
                        Style::new().fg_compat(Color::Rgb(Rgb::new(128, 128, 128))).italic()
                    } else {
                        Style::new().fg_compat(Color::Mono(MonoColor::White))
                    },
                ),
                Span::styled(
                    format!("  -  {} results", self.filtered.len()),
                    Style::new().fg_compat(MUTED_DARK),
                ),
            ]),
            self.provider_overview_line(),
            self.summary_line(),
            self.defaults_line(),
        ];

        let paragraph = Paragraph::new(Text::from_lines(lines)).wrap(WrapMode::Word);
        paragraph.render(inner, frame);
    }

    fn render_action_list(&mut self, frame: &mut Frame, area: Rect) {
        let title = if self.filtered.is_empty() {
            " Providers & Quick Actions ".to_string()
        } else {
            format!(
                " Providers & Quick Actions ({}/{}) ",
                self.selected + 1,
                self.filtered.len()
            )
        };
        let block = Block::new()
            .title(title.as_str())
            .borders(Borders::ALL)
            .style(Style::new().bg_compat(PANEL_BG))
            .border_style(Style::new().fg_compat(PANEL_BORDER_ACTIVE));
        let list_inner = block.inner(area);
        block.render(area, frame);
        self.last_action_list_area = Some(list_inner);

        let available_items = (list_inner.height as usize).max(1);
        let start = self.visible_window_start(available_items);
        let end = (start + available_items.saturating_sub(1)).min(self.filtered.len());

        let mut lines = Vec::new();
        if self.filtered.is_empty() {
            lines.push(Line::from_spans(vec![Span::styled(
                "No matching account or provider actions.",
                Style::new().fg_compat(Color::Rgb(Rgb::new(128, 128, 128))).italic(),
            )]));
            lines.push(Line::from_spans(vec![Span::styled(
                "Try `openai`, `claude`, an account label, `login`, or `default`.",
                Style::new().fg_compat(MUTED),
            )]));
        } else {
            let mut current_provider: Option<&str> = None;
            for visible_idx in start..end {
                let idx = self.filtered[visible_idx];
                let item = &self.items[idx];
                let selected = visible_idx == self.selected;

                if current_provider != Some(item.provider_id.as_str()) {
                    current_provider = Some(item.provider_id.as_str());
                    lines.push(provider_header_line(
                        &item.provider_label,
                        self.filtered_provider_switch_count(&item.provider_id),
                        self.filtered_provider_secondary_count(&item.provider_id),
                        &item.provider_id,
                    ));
                }

                let row_style = if selected {
                    Style::new().bg_compat(SELECTED_BG)
                } else {
                    Style::new()
                };
                let (icon, icon_color) = action_icon(item);
                let title = compact_item_title(item);
                let meta_width = list_inner.width.saturating_sub(16) as usize;
                let meta = truncate_with_ellipsis(&item.subtitle, meta_width);
                lines.push(Line::from_spans(vec![
                    Span::styled(
                        if selected { "> " } else { "  " },
                        row_style.fg_compat(Color::Mono(MonoColor::White)),
                    ),
                    Span::styled(format!("{} ", icon), row_style.fg_compat(icon_color).bold()),
                    Span::styled(
                        truncate_with_ellipsis(&title, 22),
                        row_style.fg_compat(Color::Mono(MonoColor::White)),
                    ),
                    Span::styled(" - ", row_style.fg_compat(MUTED_DARK)),
                    Span::styled(meta, row_style.fg_compat(MUTED)),
                ]));
            }
        }

        let paragraph = Paragraph::new(Text::from_lines(lines)).wrap(WrapMode::Word);
        paragraph.render(list_inner, frame);
    }

    fn render_detail_pane(&self, frame: &mut Frame, area: Rect) {
        let title = self
            .selected_item()
            .map(|item| format!(" {} ", item.provider_label))
            .unwrap_or_else(|| " Details ".to_string());
        let block = Block::new()
            .title(title.as_str())
            .borders(Borders::ALL)
            .style(Style::new().bg_compat(PANEL_BG))
            .border_style(Style::new().fg_compat(SECTION_BORDER));
        let inner = block.inner(area);
        block.render(area, frame);

        let Some(item) = self.selected_item() else {
            let paragraph = Paragraph::new(Text::from_line(Line::from("No action selected")))
                .style(Style::new().fg_compat(Color::Rgb(Rgb::new(80, 80, 80))));
            paragraph.render(inner, frame);
            return;
        };

        let provider_items: Vec<&AccountPickerItem> = self
            .items
            .iter()
            .filter(|candidate| candidate.provider_id == item.provider_id)
            .collect();
        let mut account_items: Vec<&AccountPickerItem> = provider_items
            .iter()
            .copied()
            .filter(|candidate| matches!(action_section(candidate), ActionSection::Switch))
            .collect();
        account_items.sort_by(|left, right| {
            account_is_active(right)
                .cmp(&account_is_active(left))
                .then_with(|| compact_item_title(left).cmp(&compact_item_title(right)))
        });
        let mut secondary_items: Vec<&AccountPickerItem> = provider_items
            .iter()
            .copied()
            .filter(|candidate| !matches!(action_section(candidate), ActionSection::Switch))
            .filter(|candidate| candidate.title != item.title)
            .collect();
        secondary_items.sort_by(|left, right| {
            action_section(left)
                .cmp(&action_section(right))
                .then_with(|| compact_item_title(left).cmp(&compact_item_title(right)))
        });
        secondary_items.truncate(6);
        let (kind_label, kind_color) = action_kind_badge(&item.command);

        let mut lines = vec![
            Line::from_spans(vec![
                Span::styled("Provider ", Style::new().fg_compat(MUTED_DARK)),
                Span::styled(
                    item.provider_label.clone(),
                    provider_style(&item.provider_id),
                ),
            ]),
            Line::from_spans(vec![
                Span::styled("Saved accounts ", Style::new().fg_compat(MUTED_DARK)),
                Span::styled(
                    account_count_summary(account_items.len()),
                    Style::new().fg_compat(Color::Mono(MonoColor::White)).bold(),
                ),
            ]),
            Line::from_spans(vec![]),
            Line::from_spans(vec![Span::styled(
                "Quick switch",
                Style::new().fg_compat(MUTED_DARK).bold(),
            )]),
        ];

        if account_items.is_empty() {
            lines.push(Line::from_spans(vec![Span::styled(
                "No saved accounts for this provider yet.",
                Style::new().fg_compat(MUTED),
            )]));
        } else {
            for account in &account_items {
                let is_selected = account.title == item.title;
                let bullet = if account_is_active(account) { "*" } else { "o" };
                let note = if is_selected { "  [selected]" } else { "" };
                lines.push(Line::from_spans(vec![
                    Span::styled(
                        format!("{} ", bullet),
                        Style::new().fg_compat(if account_is_active(account) {
                            Color::Rgb(Rgb::new(110, 214, 158))
                        } else {
                            MUTED_DARK
                        }),
                    ),
                    Span::styled(
                        compact_item_title(account),
                        Style::new().fg_compat(Color::Mono(MonoColor::White)).bold(),
                    ),
                    Span::styled(
                        note.to_string(),
                        Style::new().fg_compat(Color::Rgb(Rgb::new(170, 210, 255))),
                    ),
                ]));
                lines.push(Line::from_spans(vec![Span::styled(
                    format!(
                        "  {}",
                        truncate_with_ellipsis(
                            &account.subtitle,
                            inner.width.saturating_sub(3) as usize,
                        )
                    ),
                    Style::new().fg_compat(MUTED),
                )]));
            }
        }

        lines.push(Line::from_spans(vec![]));
        lines.push(Line::from_spans(vec![Span::styled(
            "Selected action",
            Style::new().fg_compat(MUTED_DARK).bold(),
        )]));
        lines.push(Line::from_spans(vec![
            Span::styled(kind_label, Style::new().fg_compat(kind_color).bold()),
            Span::styled(" - ", Style::new().fg_compat(MUTED_DARK)),
            Span::styled(item.title.clone(), Style::new().fg_compat(Color::Mono(MonoColor::White)).bold()),
        ]));
        lines.push(Line::from_spans(vec![Span::styled(
            item.subtitle.clone(),
            Style::new().fg_compat(MUTED),
        )]));
        lines.push(Line::from_spans(vec![]));
        lines.push(Line::from_spans(vec![Span::styled(
            "Runs",
            Style::new().fg_compat(MUTED_DARK).bold(),
        )]));
        lines.push(Line::from_spans(vec![Span::styled(
            command_preview(&item.command),
            Style::new().fg_compat(Color::Mono(MonoColor::White)),
        )]));
        lines.push(Line::from_spans(vec![Span::styled(
            action_kind_help(&item.command),
            Style::new().fg_compat(MUTED),
        )]));

        if !secondary_items.is_empty() {
            lines.push(Line::from_spans(vec![]));
            lines.push(Line::from_spans(vec![Span::styled(
                "Other controls",
                Style::new().fg_compat(MUTED_DARK).bold(),
            )]));
            for related in secondary_items {
                lines.push(Line::from_spans(vec![
                    Span::styled("- ", Style::new().fg_compat(MUTED_DARK)),
                    Span::styled(compact_item_title(related), Style::new().fg_compat(Color::Mono(MonoColor::White))),
                ]));
            }
        }

        lines.push(Line::from_spans(vec![]));
        lines.push(Line::from_spans(vec![Span::styled(
            "Press Enter to run this action.",
            Style::new().fg_compat(Color::Rgb(Rgb::new(170, 210, 255))),
        )]));

        let paragraph = Paragraph::new(Text::from_lines(lines)).wrap(WrapMode::Word);
        paragraph.render(inner, frame);
    }

    fn summary_line(&self) -> Line<'static> {
        if let Some(summary) = &self.summary {
            let mut spans = vec![
                metric_span("ready", summary.ready_count, Color::Rgb(Rgb::new(110, 214, 158))),
                Span::raw("  "),
                metric_span(
                    "attention",
                    summary.attention_count,
                    Color::Rgb(Rgb::new(255, 192, 120)),
                ),
                Span::raw("  "),
                metric_span("setup", summary.setup_count, Color::Rgb(Rgb::new(160, 168, 188))),
                Span::raw("  "),
                metric_span(
                    "providers",
                    summary.provider_count,
                    Color::Rgb(Rgb::new(140, 176, 255)),
                ),
            ];
            if summary.named_account_count > 0 {
                spans.push(Span::raw("  "));
                spans.push(metric_span(
                    "accounts",
                    summary.named_account_count,
                    Color::Rgb(Rgb::new(196, 170, 255)),
                ));
            }
            return Line::from_spans(spans);
        }

        Line::from_spans(vec![Span::styled(
            format!("{} actions available", self.filtered.len()),
            Style::new().fg_compat(MUTED),
        )])
    }

    fn defaults_line(&self) -> Line<'static> {
        let Some(summary) = &self.summary else {
            return Line::from_spans(vec![Span::styled(
                "Type to narrow actions by provider, account label, or setting.",
                Style::new().fg_compat(MUTED),
            )]);
        };

        let provider = summary.default_provider.as_deref().unwrap_or("auto");
        let model = summary
            .default_model
            .as_deref()
            .unwrap_or("provider default");

        Line::from_spans(vec![
            Span::styled("Defaults ", Style::new().fg_compat(MUTED_DARK)),
            Span::styled("provider ", Style::new().fg_compat(MUTED_DARK)),
            Span::styled(provider.to_string(), Style::new().fg_compat(Color::Mono(MonoColor::White))),
            Span::styled("  -  model ", Style::new().fg_compat(MUTED_DARK)),
            Span::styled(model.to_string(), Style::new().fg_compat(Color::Mono(MonoColor::White))),
        ])
    }
}

fn estimate_optional_string_bytes(value: &Option<String>) -> usize {
    value.as_ref().map(|value| value.capacity()).unwrap_or(0)
}

fn estimate_command_bytes(command: &AccountPickerCommand) -> usize {
    match command {
        AccountPickerCommand::SubmitInput(value) => value.capacity(),
        AccountPickerCommand::OpenAccountCenter { provider_filter }
        | AccountPickerCommand::OpenAddReplaceFlow { provider_filter } => {
            estimate_optional_string_bytes(provider_filter)
        }
        AccountPickerCommand::PromptValue {
            prompt,
            command_prefix,
            empty_value,
            status_notice,
        } => {
            prompt.capacity()
                + command_prefix.capacity()
                + estimate_optional_string_bytes(empty_value)
                + status_notice.capacity()
        }
        AccountPickerCommand::Switch { label, .. }
        | AccountPickerCommand::Login { label, .. }
        | AccountPickerCommand::Remove { label, .. } => label.capacity(),
        AccountPickerCommand::PromptNew { .. } => 0,
    }
}

fn estimate_item_bytes(item: &AccountPickerItem) -> usize {
    item.provider_id.capacity()
        + item.provider_label.capacity()
        + item.title.capacity()
        + item.subtitle.capacity()
        + estimate_command_bytes(&item.command)
}

fn estimate_summary_bytes(summary: &AccountPickerSummary) -> usize {
    estimate_optional_string_bytes(&summary.default_provider)
        + estimate_optional_string_bytes(&summary.default_model)
}
