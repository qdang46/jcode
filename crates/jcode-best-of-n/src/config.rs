use serde::{Deserialize, Serialize};

/// Best-of-N editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum BestOfNMode {
    /// Auto: transparently run N candidates and pick the best.
    /// The user only sees the final result.
    #[serde(alias = "auto")]
    #[default]
    Auto,
    /// Show: present N candidates to the user and let them pick.
    /// Also shows the automatic selection as a recommendation.
    Show,
    /// Off: best-of-N is disabled; falls through to normal single-edit path.
    Off,
}

impl BestOfNMode {
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Off)
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Show => "show",
            Self::Off => "off",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "auto" | "automatic" | "on" | "1" | "true" => Some(Self::Auto),
            "show" | "interactive" | "manual" => Some(Self::Show),
            "off" | "false" | "0" | "disabled" | "none" => Some(Self::Off),
            _ => None,
        }
    }
}

/// Configuration for temperature-based strategy diversity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TemperatureStrategyConfig {
    /// Temperature values for each candidate.
    /// When empty, `generate()` produces N temperatures spread across the range.
    pub values: Vec<f64>,

    /// Min temperature when auto-generating.
    pub min: f64,

    /// Max temperature when auto-generating.
    pub max: f64,
}

impl Default for TemperatureStrategyConfig {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            min: 0.2,
            max: 1.0,
        }
    }
}

/// Deterministic selector configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectorConfig {
    /// If true, prefer candidates with fewer file changes (focus preference).
    pub prefer_focused: bool,
    /// If true, prefer candidates with lower token cost.
    pub prefer_low_cost: bool,
}

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            prefer_focused: true,
            prefer_low_cost: true,
        }
    }
}

/// Root configuration for the best-of-N editing system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BestOfNConfig {
    /// Operating mode: auto (default), show, or off.
    pub mode: BestOfNMode,

    /// Number of parallel candidates to spawn (default: 4).
    pub count: usize,

    /// Temperature strategy for diversity.
    pub temperatures: TemperatureStrategyConfig,

    /// Selector configuration.
    pub selector: SelectorConfig,

    /// Optional model override for spawned candidates.
    /// When None, inherits the current session model.
    pub model: Option<String>,

    /// If true, spread candidates across different models/providers
    /// when multiple are available. Future expansion — currently
    /// only temperature diversity is supported.
    #[serde(default)]
    pub model_diversity: bool,
}

impl Default for BestOfNConfig {
    fn default() -> Self {
        Self {
            mode: BestOfNMode::Auto,
            count: 4,
            temperatures: TemperatureStrategyConfig::default(),
            selector: SelectorConfig::default(),
            model: None,
            model_diversity: false,
        }
    }
}

impl BestOfNConfig {
    /// Returns true if best-of-N editing is enabled.
    pub fn enabled(&self) -> bool {
        self.mode.is_enabled() && self.count > 1
    }

    /// Returns the effective candidate count (clamped to at least 1).
    pub fn effective_count(&self) -> usize {
        self.count.max(1)
    }
}
