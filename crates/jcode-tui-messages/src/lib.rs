// Phase 5 widget work - stubbed for Phase 1.3 compilation
use ftui_text::text::Line;
use jcode_tui_markdown::CopyTargetKind;

#[derive(Debug, Clone)]
pub struct MessageCacheContext;

pub fn centered_wrap_width(_area_width: u16) -> usize { 80 }
pub fn get_cached_message_lines(_msg_id: u64) -> Vec<Line<'static>> { Vec::new() }
pub fn left_pad_lines_for_centered_mode(_lines: &mut [Line<'static>], _area_width: u16) {}

#[derive(Debug, Clone)]
pub struct DisplayMessage;
impl DisplayMessage {
    pub fn error() -> Self { Self }
    pub fn system() -> Self { Self }
    pub fn user() -> Self { Self }
}
#[derive(Debug, Clone)]
pub struct TranscriptPreviewLabels;

pub fn display_messages_from_rendered_messages(_messages: &[DisplayMessage]) -> Vec<Line<'static>> { Vec::new() }
pub fn latest_user_transcript_preview(_messages: &[DisplayMessage]) -> Option<String> { None }
pub fn normalize_transcript_preview_text(_text: &str) -> String { String::new() }
pub fn transcript_preview_line(_preview: &str, _labels: &TranscriptPreviewLabels) -> Line<'static> { Line::default() }
pub fn transcript_preview_lines(_preview: &str, _labels: &TranscriptPreviewLabels, _width: usize) -> Vec<Line<'static>> { Vec::new() }
pub fn truncate_transcript_preview(_preview: &str, _max_lines: usize) -> String { String::new() }

#[derive(Debug, Clone)]
pub struct CopyTarget;
#[derive(Debug, Clone)]
pub struct EditToolRange;
#[derive(Debug, Clone)]
pub struct ImageRegion;
#[derive(Debug, Clone)]
pub struct PreparedChatFrame;
#[derive(Debug, Clone)]
pub struct PreparedMessages;
#[derive(Debug, Clone)]
pub struct PreparedSection;
#[derive(Debug, Clone)]
pub enum PreparedSectionKind { Unknown }

#[derive(Debug, Clone)]
pub struct WrappedLineMap;
