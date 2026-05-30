// Phase 5 widget work - stubbed for Phase 1.3 compilation
mod wrapped_line_map;
pub use wrapped_line_map::WrappedLineMap;

mod prepared;
pub use prepared::{
    CopyTarget, EditToolRange, ImageRegion, PreparedChatFrame, PreparedMessages,
    PreparedSection, PreparedSectionKind,
};

use std::sync::Arc;
use ftui_text::text::Line;

#[derive(Debug, Clone)]
pub struct MessageCacheContext;

pub fn centered_wrap_width(_area_width: u16) -> usize {
    80
}
pub fn get_cached_message_lines(_msg_id: u64) -> Vec<Line<'static>> {
    Vec::new()
}
pub fn left_pad_lines_for_centered_mode(_lines: &mut [Line<'static>], _area_width: u16) {}

#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Vec<String>,
    pub duration_secs: Option<f32>,
    pub title: Option<String>,
    pub tool_data: Option<jcode_message_types::ToolCall>,
}
impl DisplayMessage {
    pub fn error(_msg: impl Into<String>) -> Self {
        Self {
            role: "error".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn system(_msg: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn user(_msg: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn assistant(_msg: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn tool_text(_msg: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn meta(_msg: impl Into<String>) -> Self {
        Self {
            role: "meta".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn tool(content: impl Into<String>, _tool_name: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn usage(_msg: impl Into<String>) -> Self {
        Self {
            role: "usage".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn memory(summary: impl Into<String>, _detail: impl Into<String>) -> Self {
        Self {
            role: "memory".to_string(),
            content: summary.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn background_task(_msg: impl Into<String>) -> Self {
        Self {
            role: "background_task".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn overnight(_msg: impl Into<String>) -> Self {
        Self {
            role: "overnight".to_string(),
            content: _msg.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: None,
            tool_data: None,
        }
    }
    pub fn swarm(_title: impl Into<String>, _body: impl Into<String>) -> Self {
        Self {
            role: "swarm".to_string(),
            content: _body.into(),
            tool_calls: Vec::new(),
            duration_secs: None,
            title: Some(_title.into()),
            tool_data: None,
        }
    }
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct TranscriptPreviewLabels;
impl TranscriptPreviewLabels {
    pub const DESKTOP: Self = Self;
}

pub fn display_messages_from_rendered_messages(_messages: &[DisplayMessage]) -> Vec<Line<'static>> {
    Vec::new()
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


