//! Magic keyword system for jcode.
//!
//! Detects natural-language keyword triggers in user input, manages persistent
//! mode state, builds prompt injections for the system prompt, and dispatches
//! to 14 workflow handlers.
//!
//! # Architecture
//!
//! ```text
//! User types "$ultrawork fix the bug"
//!     ↓
//! detector::detect_keywords() → DetectedKeyword
//!     ↓
//! state::update_modes() → ModeState (persisted to .jcode/state/modes.toml)
//!     ↓
//! prompt_builder::build_keyword_prompt() → String (injected into system prompt)
//!     ↓
//! visual::compute_highlights() → Vec<KeywordHighlight> (rainbow TUI rendering)
//! ```

pub mod conflict;
pub mod detector;
pub mod intent;
pub mod prompt_builder;
pub mod registry;
pub mod sanitizer;
pub mod state;
pub mod task_size;
pub mod visual;
pub mod workflow;

// Re-exports for convenience
pub use detector::{DetectedKeyword, detect_keywords};
pub use registry::{KeywordEntry, WorkflowKind};
pub use state::ModeState;
pub use visual::KeywordHighlight;
