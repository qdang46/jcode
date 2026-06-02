// Phase 5 widget work - stubbed for Phase 1.3 compilation
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MermaidRenderOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct DiagramInfo {
    pub width: u32,
    pub height: u32,
    pub hash: u64,
}

#[derive(Debug, Clone)]
pub enum RenderResult {
    Image {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
    Svg {
        width: u32,
        height: u32,
        svg: String,
    },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugStats {
    pub deferred_pending: usize,
    pub deferred_enqueued: usize,
    pub deferred_deduped: usize,
    pub deferred_worker_renders: usize,
    pub image_state_hits: usize,
    pub image_state_misses: usize,
    pub fit_state_reuse_hits: usize,
    pub fit_protocol_rebuilds: usize,
    pub viewport_state_reuse_hits: usize,
    pub viewport_protocol_rebuilds: usize,
}

#[derive(Debug, Clone)]
pub struct ImageState;

pub fn render_mermaid_to_svg(
    _mermaid_code: &str,
    _options: MermaidRenderOptions,
) -> anyhow::Result<String> {
    Ok(String::new())
}

pub fn render_mermaid_to_png_data(
    _mermaid_code: &str,
    _options: MermaidRenderOptions,
) -> anyhow::Result<Vec<u8>> {
    Ok(Vec::new())
}

pub fn init_picker() {}
pub fn clear_image_state() {}
pub fn snapshot_active_diagrams() -> ImageState {
    ImageState
}
pub fn restore_active_diagrams(_state: ImageState) {}
pub fn reset_debug_stats() {}
pub fn clear_active_diagrams() {}
pub fn clear_streaming_preview_diagram() {}
pub fn clear_cache() {}
pub fn protocol_type() -> Option<ProtocolType> {
    Some(ProtocolType::Mermaid)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProtocolType {
    Mermaid,
}
pub fn debug_stats() -> DebugStats {
    DebugStats {
        deferred_pending: 0usize,
        deferred_enqueued: 0usize,
        deferred_deduped: 0usize,
        deferred_worker_renders: 0usize,
        image_state_hits: 0usize,
        image_state_misses: 0usize,
        fit_state_reuse_hits: 0usize,
        fit_protocol_rebuilds: 0usize,
        viewport_state_reuse_hits: 0usize,
        viewport_protocol_rebuilds: 0usize,
    }
}
pub fn debug_stats_json() -> String {
    String::new()
}
pub fn debug_image_state() -> String {
    String::new()
}
pub fn get_active_diagrams() -> Vec<DiagramInfo> {
    Vec::new()
}
pub fn debug_test_scroll(_content: Option<&str>) {}
pub fn debug_memory_profile() -> String {
    String::new()
}
pub fn debug_memory_benchmark(_iterations: usize) -> String {
    String::new()
}
pub fn debug_flicker_benchmark(_steps: usize) -> String {
    String::new()
}
pub fn debug_cache() -> String {
    String::new()
}
pub fn get_cached_path(_key: &str) -> Option<String> {
    None
}
pub fn set_log_hooks(_f: Option<fn(&str)>) {}
pub fn set_render_completed_hook(_f: Option<fn()>) {}
pub fn set_memory_snapshot_hook(_f: Option<fn()>) {}
pub fn parse_image_placeholder(_text: &str) -> Option<String> {
    None
}
pub fn parse_image_placeholder_from_line(_line: &ftui_text::text::Line<'_>) -> Option<u64> {
    None
}
pub fn get_font_size() -> Option<(u16, u16)> {
    Some((8, 16))
}
pub fn with_preferred_aspect_ratio<F, R>(_aspect_ratio: Option<f32>, f: F) -> R
where
    F: FnOnce() -> R,
{
    f()
}
pub fn diagram_placeholder_lines(_width: u32, _height: u32) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn render_image_widget_viewport(
    _hash: u64,
    _area: ftui_core::geometry::Rect,
    _buffer: &mut ftui_render::buffer::Buffer,
    _scroll_x: i32,
    _scroll_y: i32,
    _zoom_percent: u16,
    _bool_flag: bool,
) -> u16 {
    0
}
pub fn render_image_widget_scale(
    _hash: u64,
    _area: ftui_core::geometry::Rect,
    _buffer: &mut ftui_render::buffer::Buffer,
    _bool_flag: bool,
) -> u16 {
    0
}
pub fn render_image_widget_viewport_precise(
    _hash: u64,
    _area: ftui_core::geometry::Rect,
    _buffer: &mut ftui_render::buffer::Buffer,
    _scroll_x: i32,
    _scroll_y: i32,
    _zoom_percent: u16,
    _bool_flag: bool,
) -> u16 {
    0
}
pub fn is_video_export_mode() -> bool {
    false
}
pub fn write_video_export_marker(
    _hash: u64,
    _area: ftui_core::geometry::Rect,
    _buffer: &mut ftui_render::buffer::Buffer,
) {
}
pub fn deferred_render_epoch() -> u64 {
    0
}
pub fn current_preferred_aspect_ratio_bucket() -> usize {
    0
}
pub fn get_cached_png(_key: u64) -> Option<(std::path::PathBuf, u32, u32)> {
    None
}
pub fn get_cached_png_str(_key: &str) -> Option<Vec<u8>> {
    None
}

#[derive(Debug, Clone)]
pub struct ProcessMemorySnapshot;

pub fn is_mermaid_lang(_text: &str) -> bool {
    false
}
pub fn render_mermaid_untracked(_text: &str, _width_hint: Option<u16>) -> RenderResult {
    RenderResult::Image {
        width: 0,
        height: 0,
        bytes: Vec::new(),
    }
}
pub fn register_inline_image(_id: &str, _url: &str) -> Option<(u64, u32, u32)> {
    None
}
pub fn preferred_aspect_ratio_bucket() -> usize {
    0
}
pub fn preferred_aspect_ratio_bucket_for(_ratio: Option<f32>) -> Option<u16> {
    None
}
pub fn register_external_image(_id: &str, _url: &str, _w: u32, _h: u32) -> u64 {
    0
}
pub fn image_widget_placeholder_markdown(_hash: u64) -> String {
    String::new()
}
pub fn set_video_export_mode(_enabled: bool) {}
pub fn render_image_widget(
    _hash: u64,
    _area: ftui_core::geometry::Rect,
    _buffer: &mut ftui_render::buffer::Buffer,
    _centered: bool,
    _interactive: bool,
) -> u16 {
    0
}
