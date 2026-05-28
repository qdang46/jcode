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
pub struct UsageOverlayItem;

#[derive(Debug, Clone)]
pub struct UsageOverlaySummary;

pub fn item_matches_filter(_item: &UsageOverlayItem, _filter: &str) -> bool {
    true
}
