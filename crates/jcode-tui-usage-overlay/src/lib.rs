// Phase 5 widget work - stubbed for Phase 1.3 compilation
use ftui_style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageOverlayStatus {
    Loading,
    Good,
    Warning,
    Critical,
    Error,
    Info,
}

impl UsageOverlayStatus {
    pub fn label_for_display(self) -> &'static str {
        self.label()
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Good => "healthy",
            Self::Warning => "watch",
            Self::Critical => "high",
            Self::Error => "error",
            Self::Info => "info",
        }
    }
    pub fn color(self) -> Color {
        match self {
            Self::Loading => Color::rgb(129, 184, 255),
            Self::Good => Color::rgb(111, 214, 181),
            Self::Warning => Color::rgb(255, 196, 112),
            Self::Critical => Color::rgb(255, 146, 110),
            Self::Error => Color::rgb(232, 134, 134),
            Self::Info => Color::rgb(196, 170, 255),
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Loading => "◌",
            Self::Good => "●",
            Self::Warning => "▲",
            Self::Critical => "◆",
            Self::Error => "✕",
            Self::Info => "○",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageOverlayItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub status: UsageOverlayStatus,
    pub detail_lines: Vec<String>,
}

impl UsageOverlayItem {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        subtitle: impl Into<String>,
        status: UsageOverlayStatus,
        detail_lines: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            status,
            detail_lines,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageOverlaySummary {
    pub provider_count: usize,
    pub warning_count: usize,
    pub critical_count: usize,
    pub error_count: usize,
    pub session_visible: bool,
}

impl Default for UsageOverlaySummary {
    fn default() -> Self {
        Self {
            provider_count: 0,
            warning_count: 0,
            critical_count: 0,
            error_count: 0,
            session_visible: false,
        }
    }
}

pub fn item_matches_filter(_item: &UsageOverlayItem, _filter: &str) -> bool {
    true
}
