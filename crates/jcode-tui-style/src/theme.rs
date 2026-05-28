// Phase 5 widget work - stubbed for Phase 1.3 compilation
use crate::color::rgb;
use ftui_style::Color;

pub fn user_color() -> Color { rgb(138, 180, 248) }
pub fn ai_color() -> Color { rgb(129, 199, 132) }
pub fn tool_color() -> Color { rgb(120, 120, 120) }
pub fn file_link_color() -> Color { rgb(180, 200, 255) }
pub fn dim_color() -> Color { rgb(80, 80, 80) }
pub fn accent_color() -> Color { rgb(186, 139, 255) }
pub fn system_message_color() -> Color { rgb(255, 170, 220) }
pub fn queued_color() -> Color { rgb(255, 193, 7) }
pub fn asap_color() -> Color { rgb(110, 210, 255) }
pub fn error_color() -> Color { rgb(255, 95, 87) }
pub fn warning_color() -> Color { rgb(255, 184, 76) }
pub fn success_color() -> Color { rgb(129, 199, 132) }
pub fn info_color() -> Color { rgb(129, 184, 255) }

pub fn ai_text() -> ftui_style::Style { ftui_style::Style::default() }
pub fn blend_color(_c1: Color, _c2: Color, _t: f32) -> Color { rgb(128, 128, 128) }
pub fn header_icon_color() -> Color { rgb(200, 200, 200) }
pub fn header_name_color() -> Color { rgb(180, 180, 180) }
pub fn header_session_color() -> Color { rgb(160, 160, 160) }
pub fn pending_color() -> Color { rgb(255, 200, 0) }
pub fn prompt_entry_bg_color() -> Color { rgb(30, 30, 30) }
pub fn prompt_entry_color() -> Color { rgb(200, 200, 200) }
pub fn prompt_entry_shimmer_color() -> Color { rgb(100, 100, 100) }
pub fn rainbow_prompt_color(_i: usize) -> Color { rgb(128, 128, 128) }
pub fn user_bg() -> ftui_style::Style { ftui_style::Style::default() }
pub fn user_text() -> ftui_style::Style { ftui_style::Style::default() }
pub fn activity_indicator(_frame: usize) -> Color { rgb(128, 128, 128) }
pub fn activity_indicator_frame_index(_t: f64, _speed: f64) -> usize { 0 }
pub fn animated_tool_color(_i: usize) -> Color { rgb(128, 128, 128) }
