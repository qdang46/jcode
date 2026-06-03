use super::*;
use ftui_style::Style;
use std::collections::VecDeque;

pub(super) fn lru_touch<K: PartialEq>(order: &mut VecDeque<K>, key: &K) {
    if let Some(pos) = order.iter().position(|existing| existing == key) {
        order.remove(pos);
    }
}

pub(super) fn side_panel_content_signature(_page: &crate::side_panel::SidePanelPage) -> u64 {
    // TODO: implement real signature once side_panel content hashing is needed
    0
}

pub(super) fn side_panel_content_area(area: Rect) -> Option<Rect> {
    if area.width < 2 || area.height < 2 {
        return None;
    }
    Some(Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2))
}

pub(super) fn side_panel_border_style(focused: bool) -> Style {
    crate::tui::ui_layout::right_rail_border_style(
        focused,
        jcode_tui_style::theme::accent_color(),
    )
}

pub(super) fn pinned_diagram_preferred_aspect_ratio(
    page: Rect,
    pane_position: crate::config::DiagramPanePosition,
) -> Option<f32> {
    crate::tui::ui::diagram_pane::pinned_diagram_preferred_aspect_ratio(page, pane_position)
}

pub(super) fn side_panel_mermaid_preferred_aspect_ratio(
    _page: &crate::side_panel::SidePanelPage,
    content_inner: Rect,
    has_protocol: bool,
) -> Option<f32> {
    if !has_protocol {
        return None;
    }
    pinned_diagram_preferred_aspect_ratio(
        content_inner,
        crate::config::DiagramPanePosition::default(),
    )
}

pub(super) fn estimate_side_panel_pane_area(
    terminal_width: u16,
    terminal_height: u16,
    ratio_percent: u8,
) -> Option<Rect> {
    const MIN_DIFF_WIDTH: u16 = 30;
    const MIN_CHAT_WIDTH: u16 = 20;

    let max_diff = terminal_width.saturating_sub(MIN_CHAT_WIDTH);
    if max_diff < MIN_DIFF_WIDTH || terminal_height < 3 {
        return None;
    }

    let diff_width = (((terminal_width as u32 * ratio_percent.clamp(25, 100) as u32) / 100) as u16)
        .max(MIN_DIFF_WIDTH)
        .min(max_diff);
    Some(Rect::new(0, 0, diff_width, terminal_height))
}

pub(super) fn compact_image_label(label: &str) -> String {
    if label.contains('/') {
        return label
            .rsplit('/')
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/");
    }
    label.to_string()
}
