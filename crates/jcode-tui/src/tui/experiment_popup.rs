use crossterm::event::KeyCode;
use jcode_experiment_flags::{EXPERIMENT_FLAGS, ExperimentFlag, Experiments, Stage};

/// State for the experiment flags popup overlay.
#[derive(Debug, Clone)]
pub struct ExperimentPopupState {
    /// Current toggle states (key → enabled).
    flags: Vec<ExperimentPopupEntry>,
    /// Cursor position (index into `flags`).
    selected: usize,
    /// Scroll offset for the list.
    scroll: usize,
}

#[derive(Debug, Clone)]
pub struct ExperimentPopupEntry {
    key: &'static str,
    #[allow(dead_code)]
    flag: ExperimentFlag,
    stage: Stage,
    name: String,
    description: String,
    enabled: bool,
    default_enabled: bool,
}

/// Action returned after handling a key event in the popup.
pub enum ExperimentPopupAction {
    Continue,
    Cancel,
    Apply { changes: Vec<(String, bool)> },
}

impl ExperimentPopupState {
    /// Build popup state from the current config.
    pub fn from_config() -> Self {
        let config = crate::config::config();
        let experiments = Experiments::from_config(&config.experiments.entries);
        let mut flags = Vec::new();

        for spec in EXPERIMENT_FLAGS {
            // Only show Experimental and UnderDevelopment stage flags
            let (name, description) = match &spec.stage {
                Stage::Experimental {
                    name,
                    menu_description,
                    ..
                } => (name.to_string(), menu_description.to_string()),
                Stage::UnderDevelopment => (
                    format!("{:?}", spec.id),
                    "Under development — not ready for general use".to_string(),
                ),
                _ => continue,
            };

            flags.push(ExperimentPopupEntry {
                key: spec.key,
                flag: spec.id,
                stage: spec.stage,
                name,
                description,
                enabled: experiments.check(spec.id),
                default_enabled: spec.default_enabled,
            });
        }

        Self {
            flags,
            selected: 0,
            scroll: 0,
        }
    }

    /// Number of visible flags.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    /// Whether the popup is empty (no experimental flags visible).
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    /// Handle a key press. Returns the action to take.
    pub fn handle_key(&mut self, code: KeyCode) -> ExperimentPopupAction {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => ExperimentPopupAction::Cancel,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                ExperimentPopupAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.flags.len() {
                    self.selected += 1;
                }
                ExperimentPopupAction::Continue
            }
            KeyCode::Char(' ') => {
                if let Some(entry) = self.flags.get_mut(self.selected) {
                    entry.enabled = !entry.enabled;
                }
                ExperimentPopupAction::Continue
            }
            KeyCode::Enter => {
                let changes: Vec<(String, bool)> = self
                    .flags
                    .iter()
                    .filter(|e| e.enabled != e.default_enabled)
                    .map(|e| (e.key.to_string(), e.enabled))
                    .collect();
                ExperimentPopupAction::Apply { changes }
            }
            _ => ExperimentPopupAction::Continue,
        }
    }

    /// Get the current cursor index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Get the current scroll offset.
    #[allow(dead_code)]
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Update scroll to keep the selected item visible.
    #[allow(dead_code)]
    pub fn adjust_scroll(&mut self, visible_height: usize) {
        if self.selected >= self.scroll + visible_height {
            self.scroll = self.selected - visible_height + 1;
        } else if self.selected < self.scroll {
            self.scroll = self.selected;
        }
    }

    /// Get the entries for rendering.
    pub fn entries(&self) -> &[ExperimentPopupEntry] {
        &self.flags
    }
}

impl ExperimentPopupEntry {
    #[allow(dead_code)]
    pub fn key(&self) -> &'static str {
        self.key
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }
}
