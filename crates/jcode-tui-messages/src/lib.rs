// Phase 5 widget work - stubbed for Phase 1.3 compilation
mod wrapped_line_map;
pub use wrapped_line_map::WrappedLineMap;

pub mod message;
pub use message::{DisplayMessage, display_messages_from_rendered_messages};

mod prepared;
pub use prepared::{
    CopyTarget, EditToolRange, ImageRegion, PreparedChatFrame, PreparedMessages,
    PreparedSection, PreparedSectionKind,
};

pub mod cache;
pub use cache::MessageCacheContext;

use ftui_text::text::Line;
use jcode_config_types::DiffDisplayMode;

pub fn centered_wrap_width(width: u16, centered: bool, centered_max_width: usize) -> usize {
    let width = width as usize;
    if centered {
        width.min(centered_max_width).max(1)
    } else {
        width.max(1)
    }
}

pub fn get_cached_message_lines<F>(
    _msg: &DisplayMessage,
    _width: u16,
    _diff_mode: DiffDisplayMode,
    _context: MessageCacheContext,
    _render: F,
) -> Vec<Line<'static>>
where
    F: FnOnce(&DisplayMessage, u16, DiffDisplayMode) -> Vec<Line<'static>>,
{
    Vec::new()
}
pub fn left_pad_lines_for_centered_mode(_lines: &mut [Line<'static>], _area_width: u16) {}

#[derive(Debug, Clone)]
pub struct TranscriptPreviewLabels;
impl TranscriptPreviewLabels {
    pub const DESKTOP: Self = Self;
}

pub fn latest_user_transcript_preview<'a, I>(_messages: I, _char_limit: usize) -> Option<String>
where
    I: DoubleEndedIterator<Item = (&'a str, &'a str)>,
{
    None
}
pub fn normalize_transcript_preview_text(_text: &str) -> String {
    String::new()
}
pub fn transcript_preview_line(
    _role: &str,
    _content: &str,
    _char_limit: usize,
    _labels: TranscriptPreviewLabels,
) -> Option<String> {
    None
}
pub fn transcript_preview_lines<'a, I>(
    _messages: I,
    _limit: usize,
    _char_limit: usize,
    _labels: TranscriptPreviewLabels,
) -> Vec<String>
where
    I: DoubleEndedIterator<Item = (&'a str, &'a str)>,
{
    Vec::new()
}
pub fn truncate_transcript_preview(_preview: &str, _max_lines: usize) -> String {
    String::new()
}


