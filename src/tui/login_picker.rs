use ftui_style::{Ansi16, Color, MonoColor, Style};
use crate::tui::compat::StyleCompatExt;
use ftui_text::text::{Line, Span, Text};
use crate::auth::AuthState;
use crate::provider_catalog::LoginProviderDescriptor;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ftui_render::cell::PackedRgba;
use ftui_text::wrap::WrapMode;
use ftui_widgets::Widget;
use ftui_widgets::block::Block;
use ftui_widgets::borders::Borders;
use ftui_widgets::paragraph::Paragraph;

const PANEL_BG: PackedRgba = PackedRgba::rgb(24, 28, 40);
const PANEL_BORDER: PackedRgba = PackedRgba::rgb(90, 95, 110);
const PANEL_BORDER_ACTIVE: PackedRgba = PackedRgba::rgb(120, 140, 190);
const SECTION_BORDER: PackedRgba = PackedRgba::rgb(70, 78, 94);
const SELECTED_BG: PackedRgba = PackedRgba::rgb(38, 42, 56);
const MUTED: PackedRgba = PackedRgba::rgb(140, 146, 163);
const MUTED_DARK: PackedRgba = PackedRgba::rgb(100, 106, 122);
const OVERLAY_PERCENT_X: u16 = 88;
const OVERLAY_PERCENT_Y: u16 = 74;

fn rgb(r: u8, g: u8, b: u8) -> PackedRgba {
    PackedRgba::rgb(r, g, b)
}

#[derive(Debug, Clone)]
pub struct LoginPickerItem {
    pub index: usize,
    pub provider: LoginProviderDescriptor,
    pub auth_state: AuthState,
    pub method_detail: String,
}

impl LoginPickerItem {
    pub fn new(
        index: usize,
        provider: LoginProviderDescriptor,
        auth_state: AuthState,
        method_detail: impl Into<String>,
    ) -> Self {
        Self {
            index,
            provider,
            auth_state,
            method_detail: method_detail.into(),
        }
    }

    fn matches_filter(&self, filter: &str) -> bool {
        let trimmed = filter.trim();
        if trimmed.is_empty() {
            return true;
        }

        if trimmed.chars().all(|c| c.is_ascii_digit()) {
            return self.index.to_string().starts_with(trimmed);
        }

        let haystack = format!(
            "{} {} {} {} {} {} {} {} {}",
            self.index,
            self.provider.id,
            self.provider.display_name,
            self.provider.aliases.join(" "),
            self.provider.auth_kind.label(),
            self.provider.menu_detail,
            self.status_label(),
            self.method_detail,
            if self.provider.recommended {
                "recommended"
            } else {
                ""
            }
        )
        .to_lowercase();

        trimmed
            .split_whitespace()
            .all(|needle| haystack.contains(&needle.to_lowercase()))
    }

    fn status_label(&self) -> &'static str {
        match self.auth_state {
            AuthState::Available => "configured",
            AuthState::Expired => "needs attention",
            AuthState::NotConfigured => "not set up",
        }
    }

    fn status_icon(&self) -> &'static str {
        match self.auth_state {
            AuthState::Available => "✓",
            AuthState::Expired | AuthState::NotConfigured => "✕",
        }
    }

    fn status_color(&self) -> PackedRgba {
        match self.auth_state {
            AuthState::Available => PackedRgba::rgb(111, 214, 181),
            AuthState::Expired => PackedRgba::rgb(255, 196, 112),
            AuthState::NotConfigured => PackedRgba::rgb(232, 134, 134),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct LoginPickerSummary {
    pub ready_count: usize,
    pub attention_count: usize,
    pub setup_count: usize,
    pub recommended_count: usize,
}

#[derive(Debug, Clone)]
pub struct LoginPicker {
    title: String,
    items: Vec<LoginPickerItem>,
    filtered: Vec<usize>,
    selected: usize,
    filter: String,
    summary: LoginPickerSummary,
    last_provider_list_area: Option<ftui_core::geometry::Rect>,
}

#[expect(
    clippy::large_enum_variant,
    reason = "execute action carries the selected provider descriptor directly for simple overlay handling"
)]
pub enum OverlayAction {
    Continue,
    Close,
    Execute(LoginProviderDescriptor),
}

impl LoginPicker {
    pub fn with_summary(
        title: impl Into<String>,
        items: Vec<LoginPickerItem>,
        summary: LoginPickerSummary,
    ) -> Self {
        let mut picker = Self {
            title: title.into(),
            items,
            filtered: Vec::new(),
            selected: 0,
            filter: String::new(),
            summary,
            last_provider_list_area: None,
        };
        picker.apply_filter();
        picker
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

    fn selected_item(&self) -> Option<&LoginPickerItem> {
        self.filtered
            .get(self.selected)
            .and_then(|idx| self.items.get(*idx))
    }

    fn visible_window_start(&self, available_items: usize) -> usize {
        self.selected
            .saturating_sub(available_items.saturating_sub(1).min(available_items / 2))
    }

    fn visible_index_for_list_row(&self, row: u16, list_height: u16) -> Option<usize> {
        if self.filtered.is_empty() {
            return None;
        }
        let available_items = (list_height as usize).max(1);
        let start = self.visible_window_start(available_items);
        let visible_idx = start + row as usize;
        (visible_idx < (start + available_items).min(self.filtered.len())).then_some(visible_idx)
    }

    fn apply_filter(&mut self) {
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| item.matches_filter(&self.filter).then_some(idx))
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn handle_overlay_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> anyhow::Result<OverlayAction> {
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
            KeyCode::Enter => {
                if let Some(item) = self.selected_item() {
                    return Ok(OverlayAction::Execute(item.provider));
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
        let Some(list_inner) = self.last_provider_list_area else {
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
                if let Some(visible_idx) = self.visible_index_for_list_row(row, list_inner.height) {
                    self.selected = visible_idx;
                }
            }
            _ => {}
        }
    }

    pub fn render(&mut self, frame: &mut ftui::Frame) {
        use ftui_core::geometry::Rect;
        use ftui_layout::{Constraint, Flex};

        let area = centered_rect(OVERLAY_PERCENT_X, OVERLAY_PERCENT_Y, Rect::new(0, 0, frame.buffer.width(), frame.buffer.height()));

        let title = format!(" {} ", self.title);
        let block = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .border_style(Style::new().fg(PANEL_BORDER));
        block.render(area, frame);

        let inner = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        let rows = Flex::vertical()
            .constraints([
                Constraint::Fixed(3),
                Constraint::Min(12),
                Constraint::Fixed(2),
            ])
            .split(inner);

        self.render_header(frame, rows[0]);

        let body = Flex::horizontal()
            .constraints([Constraint::Percentage(37.0), Constraint::Percentage(63.0)])
            .split(rows[1]);

        self.render_provider_list(frame, body[0]);
        self.render_detail_pane(frame, body[1]);

        let footer = Paragraph::new(Text::from_line(Line::from_spans(vec![
            Span::styled("Tip ", Style::new().fg(MUTED_DARK)),
            Span::styled(
                "Move or click through providers on the left; the focused provider expands on the right with setup and account details.",
                Style::new().fg(MUTED),
            ),
        ])));
        footer.render(rows[2], frame);
    }

    fn render_header(&self, frame: &mut ftui::Frame, area: ftui_core::geometry::Rect) {
        let block = Block::default()
            .title(" Login overview ")
            .borders(Borders::ALL)
            .style(Style::new().bg(PANEL_BG))
            .border_style(Style::new().fg(SECTION_BORDER));
        let inner = block.inner(area);
        block.render(area, frame);

        let lines = vec![
            Line::from_spans(vec![
                Span::styled("Filter ", Style::new().fg(MUTED_DARK)),
                Span::styled(
                    if self.filter.is_empty() {
                        "type provider, status, or auth method".to_string()
                    } else {
                        self.filter.clone()
                    },
                    if self.filter.is_empty() {
                        Style::new().fg_compat(Color::Ansi16(Ansi16::BrightBlack)).italic()
                    } else {
                        Style::new().fg_compat(Color::Mono(MonoColor::White))
                    },
                ),
                Span::styled(
                    format!("  ·  {} results", self.filtered.len()),
                    Style::new().fg(MUTED_DARK),
                ),
            ]),
            Line::from_spans(vec![
                metric_span(
                    "configured",
                    self.summary.ready_count,
                    PackedRgba::rgb(111, 214, 181),
                ),
                Span::raw("  "),
                metric_span(
                    "attention",
                    self.summary.attention_count,
                    PackedRgba::rgb(255, 196, 112),
                ),
                Span::raw("  "),
                metric_span(
                    "setup",
                    self.summary.setup_count,
                    PackedRgba::rgb(160, 168, 188),
                ),
                Span::raw("  "),
                metric_span(
                    "recommended",
                    self.summary.recommended_count,
                    PackedRgba::rgb(196, 170, 255),
                ),
            ]),
        ];

        Paragraph::new(Text::from_lines(lines))
            .wrap(WrapMode::Word)
            .render(inner, frame);
    }

    fn render_provider_list(&mut self, frame: &mut ftui::Frame, area: ftui_core::geometry::Rect) {
        let title = if self.filtered.is_empty() {
            " Providers ".to_string()
        } else {
            format!(
                " Providers ({}/{}) ",
                self.selected + 1,
                self.filtered.len()
            )
        };
        let block = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .style(Style::new().bg(PANEL_BG))
            .border_style(Style::new().fg(PANEL_BORDER_ACTIVE));
        let inner = block.inner(area);
        block.render(area, frame);
        self.last_provider_list_area = Some(inner);

        let available_items = inner.height.max(1) as usize;
        let start = self.visible_window_start(available_items);
        let end = (start + available_items).min(self.filtered.len());

        let mut lines = Vec::new();
        if self.filtered.is_empty() {
            lines.push(Line::from_spans(vec![Span::styled(
                "No matching providers.",
                Style::new().fg_compat(Color::Ansi16(Ansi16::BrightBlack)).italic(),
            )]));
            lines.push(Line::from_spans(vec![Span::styled(
                "Try `openai`, `oauth`, `configured`, or `setup`.",
                Style::new().fg(MUTED),
            )]));
        } else {
            for visible_idx in start..end {
                let idx = self.filtered[visible_idx];
                let item = &self.items[idx];
                let selected = visible_idx == self.selected;
                let row_style = if selected {
                    Style::new().bg(SELECTED_BG)
                } else {
                    Style::new()
                };

                let row_width = inner.width.saturating_sub(2) as usize;
                let name =
                    truncate_with_ellipsis(item.provider.display_name, row_width.saturating_sub(2));
                let visible_name_len = name.chars().count();
                let padding = row_width.saturating_sub(visible_name_len + 2);

                lines.push(Line::from_spans(vec![
                    Span::styled(
                        if selected { "› " } else { "  " },
                        row_style.fg_compat(Color::Mono(MonoColor::White)),
                    ),
                    Span::styled(name, row_style.patch(&provider_style(item.provider.id))),
                    Span::styled(" ".repeat(padding), row_style),
                    Span::styled(item.status_icon(), row_style.fg(item.status_color()).bold()),
                ]));
            }
        }

        Paragraph::new(Text::from_lines(lines))
            .wrap(WrapMode::Word)
            .render(inner, frame);
    }

    fn render_detail_pane(&self, frame: &mut ftui::Frame, area: ftui_core::geometry::Rect) {
        let title = self
            .selected_item()
            .map(|item| format!(" {} ", item.provider.display_name))
            .unwrap_or_else(|| " Details ".to_string());
        let block = Block::default()
            .title(title.as_str())
            .borders(Borders::ALL)
            .style(Style::new().bg(PANEL_BG))
            .border_style(Style::new().fg(SECTION_BORDER));
        let inner = block.inner(area);
        block.render(area, frame);

        let Some(item) = self.selected_item() else {
            Paragraph::new("No provider selected")
                .style(Style::new().fg_compat(Color::Ansi16(Ansi16::BrightBlack)))
                .render(inner, frame);
            return;
        };

        let aliases = if item.provider.aliases.is_empty() {
            "none".to_string()
        } else {
            item.provider.aliases.join(", ")
        };
        let mut lines = vec![
            Line::from_spans(vec![
                Span::styled(
                    item.status_icon(),
                    Style::new().fg(item.status_color()).bold(),
                ),
                Span::styled(
                    format!(" {}", item.status_label()),
                    Style::new().fg(item.status_color()).bold(),
                ),
            ]),
            Line::from_spans(vec![
                Span::styled("Provider ", Style::new().fg(MUTED_DARK)),
                Span::styled(
                    item.provider.display_name.to_string(),
                    provider_style(item.provider.id),
                ),
                if item.provider.recommended {
                    Span::styled(
                        "  recommended",
                        Style::new().fg(PackedRgba::rgb(196, 170, 255)),
                    )
                } else {
                    Span::raw("")
                },
            ]),
            Line::from_spans(vec![
                Span::styled("Login command ", Style::new().fg(MUTED_DARK)),
                Span::styled(
                    format!("/login {}", item.provider.id),
                    Style::new().fg_compat(Color::Mono(MonoColor::White)),
                ),
            ]),
            Line::from_spans(vec![Span::styled(
                "Authentication",
                Style::new().fg(MUTED_DARK).bold(),
            )]),
            Line::from_spans(vec![Span::styled(
                item.provider.auth_kind.label(),
                Style::new()
                    .fg(auth_kind_color(item.provider.auth_kind.label()))
                    .bold(),
            )]),
            Line::from_spans(vec![]),
            Line::from_spans(vec![Span::styled(
                "Detected setup",
                Style::new().fg(MUTED_DARK).bold(),
            )]),
            Line::from_spans(vec![Span::styled(
                item.method_detail.clone(),
                Style::new().fg(MUTED),
            )]),
            Line::from_spans(vec![]),
            Line::from_spans(vec![Span::styled(
                "What you need",
                Style::new().fg(MUTED_DARK).bold(),
            )]),
            Line::from_spans(vec![Span::styled(
                item.provider.menu_detail.to_string(),
                Style::new().fg_compat(Color::Mono(MonoColor::White)),
            )]),
            Line::from_spans(vec![]),
            Line::from_spans(vec![
                Span::styled("Aliases ", Style::new().fg(MUTED_DARK)),
                Span::styled(aliases, Style::new().fg(MUTED)),
            ]),
            Line::from_spans(vec![
                Span::styled("Numbered accounts ", Style::new().fg(MUTED_DARK)),
                Span::styled(
                    if provider_supports_named_accounts(item.provider) {
                        "supported"
                    } else {
                        "not used for this provider"
                    },
                    Style::new().fg(MUTED),
                ),
            ]),
        ];

        let account_lines = account_detail_lines(item.provider);
        if !account_lines.is_empty() {
            lines.push(Line::from_spans(vec![]));
            lines.extend(account_lines);
        }

        lines.push(Line::from_spans(vec![]));
        lines.push(Line::from_spans(vec![Span::styled(
            "Press Enter to begin login.",
            Style::new().fg(PackedRgba::rgb(170, 210, 255)),
        )]));

        Paragraph::new(Text::from_lines(lines))
            .wrap(WrapMode::Word)
            .render(inner, frame);
    }
}

fn estimate_item_bytes(item: &LoginPickerItem) -> usize {
    item.method_detail.capacity()
        + item.provider.id.len()
        + item.provider.display_name.len()
        + item
            .provider
            .aliases
            .iter()
            .map(|value| value.len())
            .sum::<usize>()
        + item.provider.menu_detail.len()
}

fn hotkey(text: &'static str) -> Span<'static> {
    Span::styled(text, Style::new().fg_compat(Color::Mono(MonoColor::White)).bg_compat(Color::Ansi16(Ansi16::BrightBlack)))
}

fn metric_span(label: &'static str, value: usize, color: PackedRgba) -> Span<'static> {
    Span::styled(
        format!("{} {}", label, value),
        Style::new().fg(color).bold(),
    )
}

fn provider_style(provider_id: &str) -> Style {
    let color = match provider_id {
        "claude" => PackedRgba::rgb(229, 187, 111),
        "openai" => PackedRgba::rgb(111, 214, 181),
        "gemini" | "google" => PackedRgba::rgb(129, 184, 255),
        "copilot" => PackedRgba::rgb(182, 154, 255),
        "cursor" => PackedRgba::rgb(131, 215, 255),
        "openrouter"
        | "openai-compatible"
        | "opencode"
        | "opencode-go"
        | "zai"
        | "chutes"
        | "cerebras"
        | "alibaba-coding-plan"
        | "antigravity"
        | "jcode" => PackedRgba::rgb(189, 200, 255),
        _ => PackedRgba::rgb(180, 190, 220),
    };
    Style::new().fg(color).bold()
}

fn auth_kind_color(kind: &str) -> PackedRgba {
    match kind {
        "OAuth" => PackedRgba::rgb(129, 184, 255),
        "API key" => PackedRgba::rgb(182, 154, 255),
        "device code" => PackedRgba::rgb(111, 214, 181),
        "CLI" => PackedRgba::rgb(131, 215, 255),
        "API key / CLI" => PackedRgba::rgb(229, 187, 111),
        "local endpoint" => PackedRgba::rgb(111, 214, 181),
        _ => PackedRgba::rgb(180, 190, 220),
    }
}

fn provider_supports_named_accounts(provider: LoginProviderDescriptor) -> bool {
    matches!(
        provider.target,
        crate::provider_catalog::LoginProviderTarget::Claude
            | crate::provider_catalog::LoginProviderTarget::OpenAi
    )
}

fn account_detail_lines(provider: LoginProviderDescriptor) -> Vec<Line<'static>> {
    match provider.target {
        crate::provider_catalog::LoginProviderTarget::Claude => claude_account_lines(),
        crate::provider_catalog::LoginProviderTarget::OpenAi => openai_account_lines(),
        _ => vec![
            Line::from_spans(vec![Span::styled(
                "Accounts",
                Style::new().fg(MUTED_DARK).bold(),
            )]),
            Line::from_spans(vec![Span::styled(
                "This provider is usually configured as a single credential or env-based login.",
                Style::new().fg(MUTED),
            )]),
        ],
    }
}

fn claude_account_lines() -> Vec<Line<'static>> {
    let accounts = crate::auth::claude::list_accounts().unwrap_or_default();
    let active_label = crate::auth::claude::active_account_label();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let mut lines = vec![Line::from_spans(vec![Span::styled(
        "Accounts",
        Style::new().fg(MUTED_DARK).bold(),
    )])];

    if accounts.is_empty() {
        lines.push(Line::from_spans(vec![Span::styled(
            "No saved Claude accounts yet.",
            Style::new().fg(MUTED),
        )]));
        lines.push(Line::from_spans(vec![
            Span::styled("Add more later with ", Style::new().fg(MUTED_DARK)),
            Span::styled("/account claude add", Style::new().fg_compat(Color::Mono(MonoColor::White))),
        ]));
        return lines;
    }

    let active = active_label.unwrap_or_else(crate::auth::claude::primary_account_label);
    lines.push(Line::from_spans(vec![Span::styled(
        format!("{} saved · active: {}", accounts.len(), active),
        Style::new().fg(MUTED),
    )]));

    for account in accounts.iter().take(6) {
        let is_active = active == account.label;
        let account_status = if account.expires > now_ms {
            "valid"
        } else {
            "expired"
        };
        let plan = account
            .subscription_type
            .as_deref()
            .unwrap_or("subscription unknown");
        let email = account
            .email
            .as_deref()
            .map(mask_email)
            .unwrap_or_else(|| "email unknown".to_string());
        lines.push(Line::from_spans(vec![
            Span::styled(
                if is_active { "● " } else { "○ " },
                Style::new().fg(if is_active {
                    PackedRgba::rgb(111, 214, 181)
                } else {
                    MUTED
                }),
            ),
            Span::styled(account.label.clone(), Style::new().fg_compat(Color::Mono(MonoColor::White))),
            Span::styled(
                format!(" · {} · {} · {}", email, account_status, plan),
                Style::new().fg(MUTED),
            ),
        ]));
    }

    if accounts.len() > 6 {
        lines.push(Line::from_spans(vec![Span::styled(
            format!("+{} more accounts", accounts.len() - 6),
            Style::new().fg(MUTED_DARK),
        )]));
    }

    lines.push(Line::from_spans(vec![
        Span::styled("Manage with ", Style::new().fg(MUTED_DARK)),
        Span::styled("/account claude", Style::new().fg_compat(Color::Mono(MonoColor::White))),
    ]));
    lines
}

fn openai_account_lines() -> Vec<Line<'static>> {
    let accounts = crate::auth::codex::list_accounts().unwrap_or_default();
    let active_label = crate::auth::codex::active_account_label();
    let now_ms = chrono::Utc::now().timestamp_millis();

    let mut lines = vec![Line::from_spans(vec![Span::styled(
        "Accounts",
        Style::new().fg(MUTED_DARK).bold(),
    )])];

    if accounts.is_empty() {
        lines.push(Line::from_spans(vec![Span::styled(
            "No saved OpenAI accounts yet.",
            Style::new().fg(MUTED),
        )]));
        lines.push(Line::from_spans(vec![
            Span::styled("Add more later with ", Style::new().fg(MUTED_DARK)),
            Span::styled("/account openai add", Style::new().fg_compat(Color::Mono(MonoColor::White))),
        ]));
        return lines;
    }

    let active = active_label.unwrap_or_else(crate::auth::codex::primary_account_label);
    lines.push(Line::from_spans(vec![Span::styled(
        format!("{} saved · active: {}", accounts.len(), active),
        Style::new().fg(MUTED),
    )]));

    for account in accounts.iter().take(6) {
        let is_active = active == account.label;
        let account_status = match account.expires_at {
            Some(expires_at) if expires_at > now_ms => "valid",
            Some(_) => "expired",
            None => "valid",
        };
        let email = account
            .email
            .as_deref()
            .map(mask_email)
            .unwrap_or_else(|| "email unknown".to_string());
        let account_id = account
            .account_id
            .as_deref()
            .unwrap_or("account id unknown");
        lines.push(Line::from_spans(vec![
            Span::styled(
                if is_active { "● " } else { "○ " },
                Style::new().fg(if is_active {
                    PackedRgba::rgb(111, 214, 181)
                } else {
                    MUTED
                }),
            ),
            Span::styled(account.label.clone(), Style::new().fg_compat(Color::Mono(MonoColor::White))),
            Span::styled(
                format!(" · {} · {} · {}", email, account_status, account_id),
                Style::new().fg(MUTED),
            ),
        ]));
    }

    if accounts.len() > 6 {
        lines.push(Line::from_spans(vec![Span::styled(
            format!("+{} more accounts", accounts.len() - 6),
            Style::new().fg(MUTED_DARK),
        )]));
    }

    lines.push(Line::from_spans(vec![
        Span::styled("Manage with ", Style::new().fg(MUTED_DARK)),
        Span::styled("/account openai", Style::new().fg_compat(Color::Mono(MonoColor::White))),
    ]));
    lines
}

fn mask_email(email: &str) -> String {
    let Some((local, domain)) = email.split_once('@') else {
        return email.to_string();
    };

    let masked_local = match local.chars().count() {
        0 => "?".to_string(),
        1..=2 => format!("{}*", local.chars().next().unwrap_or('?')),
        _ => {
            let first = local.chars().next().unwrap_or('?');
            let last = local.chars().last().unwrap_or('?');
            format!("{}***{}", first, last)
        }
    };

    format!("{}@{}", masked_local, domain)
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

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ftui_core::geometry::Rect,
) -> ftui_core::geometry::Rect {
    use ftui_layout::{Constraint, Flex};

    let popup = Flex::vertical()
        .constraints([
            Constraint::Percentage((100 - percent_y) as f32 / 2.0),
            Constraint::Percentage(percent_y as f32),
            Constraint::Percentage((100 - percent_y) as f32 / 2.0),
        ])
        .split(area);
    Flex::horizontal()
        .constraints([
            Constraint::Percentage((100 - percent_x) as f32 / 2.0),
            Constraint::Percentage(percent_x as f32),
            Constraint::Percentage((100 - percent_x) as f32 / 2.0),
        ])
        .split(popup[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ftui_widgets::Widget;
    use ftui_widgets::paragraph::Paragraph;

    fn buffer_to_text(buffer: &ftui_core::buffer::Buffer) -> String {
        let area = buffer.area;
        let mut out = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn test_login_picker_preserves_underlying_background_outside_panels() {
        let mut picker = LoginPicker::with_summary(
            " Login ",
            vec![LoginPickerItem::new(
                1,
                crate::provider_catalog::OPENAI_LOGIN_PROVIDER,
                AuthState::Available,
                "OAuth credentials configured",
            )],
            LoginPickerSummary {
                ready_count: 1,
                ..LoginPickerSummary::default()
            },
        );

        let backend = ftui_core::Terminal::<ftui_core::backend::TestBackend>::new();
        let mut terminal = backend.assert();
        let mut frame = terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, frame.buffer.width(), frame.buffer.height());
                let fill = vec![
                    Line::from_spans(vec!["X".repeat(area.width as usize)]);
                    area.height as usize
                ];
                Paragraph::new(Text::from_lines(fill)).render(area, frame);
                picker.render(frame);
            })
            .expect("draw failed");

        let overlay = centered_rect(
            OVERLAY_PERCENT_X,
            OVERLAY_PERCENT_Y,
            Rect::new(0, 0, 50, 14),
        );
        let probe = &frame.buffer[(overlay.x + overlay.width - 3, overlay.y + 2)];
        assert_eq!(probe.symbol(), "X");
        // Note: color comparison would need different approach in ftui
    }

    #[test]
    fn test_login_picker_mouse_click_selects_visible_provider() {
        let mut picker = LoginPicker::with_summary(
            " Login ",
            vec![
                LoginPickerItem::new(
                    1,
                    crate::provider_catalog::OPENAI_LOGIN_PROVIDER,
                    AuthState::NotConfigured,
                    "not configured",
                ),
                LoginPickerItem::new(
                    2,
                    crate::provider_catalog::CLAUDE_LOGIN_PROVIDER,
                    AuthState::Available,
                    "OAuth configured",
                ),
            ],
            LoginPickerSummary::default(),
        );

        let backend = ftui_core::Terminal::<ftui_core::backend::TestBackend>::new();
        let mut terminal = backend.assert();
        terminal
            .draw(|frame| picker.render(frame))
            .expect("draw failed");

        let list_area = picker
            .last_provider_list_area
            .expect("render should record provider list area");
        picker.handle_overlay_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: list_area.x + 1,
            row: list_area.y + 1,
            modifiers: KeyModifiers::empty(),
        });

        assert_eq!(
            picker.selected_item().map(|item| item.provider.id),
            Some("claude")
        );
    }

    #[test]
    fn login_picker_catalog_state_space_renders_and_executes_every_provider_state() {
        let providers = crate::provider_catalog::login_providers();
        assert!(
            !providers.is_empty(),
            "login provider catalog should not be empty"
        );

        for auth_state in [
            AuthState::Available,
            AuthState::Expired,
            AuthState::NotConfigured,
        ] {
            for (index, provider) in providers.iter().copied().enumerate() {
                let method_detail =
                    format!("state-space detail for {} {auth_state:?}", provider.id);
                let mut picker = LoginPicker::with_summary(
                    " Login ",
                    vec![LoginPickerItem::new(
                        index + 1,
                        provider,
                        auth_state,
                        method_detail.clone(),
                    )],
                    LoginPickerSummary {
                        ready_count: usize::from(matches!(auth_state, AuthState::Available)),
                        attention_count: usize::from(matches!(auth_state, AuthState::Expired)),
                        setup_count: usize::from(matches!(auth_state, AuthState::NotConfigured)),
                        recommended_count: usize::from(provider.recommended),
                    },
                );

                let backend = ftui_core::Terminal::<ftui_core::backend::TestBackend>::new();
                let mut terminal = backend.assert();
                terminal
                    .draw(|frame| picker.render(frame))
                    .expect("draw failed");

                match picker
                    .handle_overlay_key(KeyCode::Enter, KeyModifiers::empty())
                    .expect("enter should be handled")
                {
                    OverlayAction::Execute(selected) => assert_eq!(selected.id, provider.id),
                    _ => panic!(
                        "Enter should execute provider={} state={auth_state:?}",
                        provider.id
                    ),
                }
            }
        }
    }
}
