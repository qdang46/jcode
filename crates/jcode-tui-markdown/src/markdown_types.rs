// Phase 5 widget work - stubbed for Phase 1.3 compilation
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub enum DiagramDisplayMode {
    #[default]
    None,
    Margin,
    Pinned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub enum MarkdownSpacingMode {
    #[default]
    Compact,
    Document,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyTargetKind {
    CodeBlock { language: Option<String> },
    Error,
    ToolOutput,
}

#[derive(Clone, Debug)]
pub struct RawCopyTarget {
    pub kind: CopyTargetKind,
    pub content: String,
    pub start_raw_line: usize,
    pub end_raw_line: usize,
    pub badge_raw_line: usize,
}
