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
//! workflow::executor::execute_active_workflows() → Vec<(idx, kind, WorkflowAction)>
//!     ↓
//! workflow::executor::apply_actions() → updates ModeState metadata, removes completed
//!     ↓
//! workflow::executor::build_workflow_prompt() → String (injected into system prompt)
//! ```

pub mod conflict;
pub mod detector;
pub mod intent;
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
pub use workflow::executor::DeferredSpawn;
pub use workflow::executor::{
    apply_actions, build_workflow_prompt, execute_active_workflows, process_turn,
    process_turn_response,
};
pub use workflow::{WorkflowAction, WorkflowContext, WorkflowHandler};
