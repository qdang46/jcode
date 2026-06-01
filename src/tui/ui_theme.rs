use ftui_style::Color;
pub(super) use jcode_tui_style::theme::{
    accent_color, ai_color, ai_text, asap_color, blend_color, dim_color, file_link_color,
    header_icon_color, header_name_color, header_session_color, pending_color,
    prompt_entry_bg_color, prompt_entry_color, prompt_entry_shimmer_color, queued_color,
    rainbow_prompt_color, system_message_color, tool_color, user_bg, user_color, user_text,
};

pub(super) fn activity_indicator_frame_index(elapsed: f64, fps: f64) -> usize {
    jcode_tui_style::theme::activity_indicator_frame_index(elapsed, fps)
}

pub(super) fn activity_indicator(elapsed: f64, fps: f64, _use_secondary: bool) -> &'static str {
    // Frame index based on elapsed time and fps
    let _frame = (elapsed * fps) as usize % 8;
    "⠋"
}

pub(super) fn animated_tool_color(frame: usize) -> Color {
    jcode_tui_style::theme::animated_tool_color(frame)
}
