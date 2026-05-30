use ftui_style::{Ansi16, Color, MonoColor, Style};
use crate::tui::compat::StyleCompatExt;
use ftui_render::frame::Frame;
use ftui_text::text::{Line, Span};
use ftui_widgets::paragraph::Paragraph;
use std::sync::atomic::{AtomicUsize, Ordering};

use jcode_core::panic_util::panic_payload_to_string;

use super::layout_support::clear_area;
use super::theme_support::dim_color;

/// Number of recovered panics while rendering the frame.
static DRAW_PANIC_COUNT: AtomicUsize = AtomicUsize::new(0);

pub(super) fn render_recovered_panic_frame(
    frame: &mut Frame,
    payload: &(dyn std::any::Any + Send),
) {
    let panic_count = DRAW_PANIC_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let msg = panic_payload_to_string(payload);
    if panic_count <= 3 || panic_count.is_multiple_of(50) {
        crate::logging::error(&format!(
            "Recovered TUI draw panic #{}: {}",
            panic_count, msg
        ));
    }
    let area = frame.area().intersection(*frame.buffer_mut().area());
    if area.width == 0 || area.height == 0 {
        return;
    }
    clear_area(frame, area);
    let lines = vec![
        Line::from_spans(vec![Span::styled(
            "rendering error recovered",
            Style::default().fg_compat(Color::Ansi16(Ansi16::Red)),
        )]),
        Line::from_spans(vec![Span::styled(
            "continuing with a safe fallback frame",
            Style::default().fg(dim_color()),
        )]),
    ];
    Paragraph::new(lines).render(area, &mut frame.buffer);
}
