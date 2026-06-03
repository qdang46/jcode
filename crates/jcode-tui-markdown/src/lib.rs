// Phase 5 widget work - stubbed for Phase 1.3 compilation
#[path = "markdown_types.rs"]
mod types;

use serde::Serialize;

pub use types::{CopyTargetKind, DiagramDisplayMode, MarkdownSpacingMode};

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct MarkdownConfigSnapshot {
    pub diagram_mode: DiagramDisplayMode,
    pub markdown_spacing: MarkdownSpacingMode,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ProcessMemorySnapshot {
    pub rss_bytes: Option<u64>,
    pub peak_rss_bytes: Option<u64>,
    pub virtual_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MarkdownDebugStats {
    pub total_renders: u64,
    pub last_render_ms: Option<f32>,
    pub last_text_len: Option<usize>,
    pub last_lines: Option<usize>,
    pub last_headings: usize,
    pub last_code_blocks: usize,
    pub last_mermaid_blocks: usize,
    pub last_tables: usize,
    pub last_list_items: usize,
    pub last_blockquotes: usize,
    pub highlight_cache_hits: u64,
    pub highlight_cache_misses: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MarkdownMemoryProfile {
    pub process_rss_bytes: Option<u64>,
    pub process_peak_rss_bytes: Option<u64>,
    pub process_virtual_bytes: Option<u64>,
    pub highlight_cache_entries: usize,
    pub highlight_cache_limit: usize,
    pub highlight_cache_lines: usize,
    pub highlight_cache_spans: usize,
    pub highlight_cache_text_bytes: usize,
    pub highlight_cache_estimate_bytes: usize,
}

pub type RawCopyTarget = types::RawCopyTarget;

pub fn set_config_snapshot_hook(_hook: fn() -> MarkdownConfigSnapshot) {}
pub fn set_memory_snapshot_hook(_hook: fn() -> ProcessMemorySnapshot) {}
pub fn render_markdown(_text: &str) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn render_markdown_with_width(
    _text: &str,
    _width: Option<usize>,
) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn render_markdown_lazy(_text: &str) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn extract_copy_targets_from_rendered_lines(
    _lines: &[ftui_text::text::Line],
) -> Vec<RawCopyTarget> {
    Vec::new()
}
pub fn highlight_code_cached(
    _code: &str,
    _lang: Option<&str>,
) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn highlight_file_lines(
    _text: &str,
    _lang: Option<&str>,
) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn highlight_line(_line: &str, _lang: Option<&str>) -> ftui_text::text::Line<'static> {
    ftui_text::text::Line::default()
}
pub fn render_table(
    _rows: &[Vec<ftui_text::text::Line<'static>>],
    _widths: &[usize],
) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn render_table_with_width(
    _rows: &[Vec<ftui_text::text::Line<'static>>],
    _width: usize,
) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn center_code_blocks() -> bool {
    false
}
pub fn set_center_code_blocks(_value: bool) {}
pub fn get_diagram_mode_override() -> Option<DiagramDisplayMode> {
    None
}
pub fn set_diagram_mode_override(_mode: Option<DiagramDisplayMode>) {}
pub fn effective_diagram_mode() -> DiagramDisplayMode {
    DiagramDisplayMode::default()
}
pub fn effective_markdown_spacing_mode() -> MarkdownSpacingMode {
    MarkdownSpacingMode::default()
}
pub fn with_deferred_mermaid_render_context<R>(_f: impl FnOnce() -> R) -> R {
    _f()
}
pub fn deferred_mermaid_render_context_enabled() -> bool {
    false
}
pub fn streaming_render_context_enabled() -> bool {
    false
}
pub fn with_streaming_render_context<R>(_f: impl FnOnce() -> R) -> R {
    _f()
}
pub fn debug_stats() -> MarkdownDebugStats {
    MarkdownDebugStats::default()
}
pub fn debug_memory_profile() -> MarkdownMemoryProfile {
    MarkdownMemoryProfile::default()
}
pub fn reset_debug_stats() {}
pub fn debug_stats_json() -> Option<serde_json::Value> {
    None
}
pub fn wrap_line(
    _line: ftui_text::text::Line<'static>,
    _width: usize,
) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn wrap_lines(
    _lines: Vec<ftui_text::text::Line<'static>>,
    _width: usize,
) -> Vec<ftui_text::text::Line<'static>> {
    Vec::new()
}
pub fn progress_bar(_progress: f32, _width: usize) -> String {
    String::new()
}
pub fn progress_line(
    _label: &str,
    _progress: f32,
    _width: usize,
) -> ftui_text::text::Line<'static> {
    ftui_text::text::Line::default()
}
pub fn recenter_structured_blocks_for_display(
    _lines: &mut [ftui_text::text::Line<'static>],
    _width: usize,
) {
}

pub struct IncrementalMarkdownRenderer;

impl Default for IncrementalMarkdownRenderer {
    fn default() -> Self {
        Self::new(None)
    }
}

impl IncrementalMarkdownRenderer {
    pub fn new(_max_width: Option<usize>) -> Self {
        Self
    }
    pub fn update(&mut self, _text: &str) {}
    pub fn lines(&self) -> Vec<ftui_text::text::Line<'static>> {
        Vec::new()
    }
    pub fn take_error(&mut self) -> Option<String> {
        None
    }
    pub fn reset(&mut self) {}
    pub fn set_width(&mut self, _max_width: Option<usize>) {}
    pub fn debug_memory_profile(&self) -> serde_json::Value {
        serde_json::json!({"present": false, "total_estimate_bytes": 0})
    }
}
