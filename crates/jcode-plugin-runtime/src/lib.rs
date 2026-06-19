pub mod api;
pub mod audit;
pub mod bridge;
pub mod dispatcher;
pub mod errors;
pub mod gate;
pub mod loader;
pub mod native;
pub mod registry;
pub mod runtime;
pub mod sandbox;
pub mod server;
pub mod timer;
pub mod transpiler;
pub mod tui_api;
pub mod tui_system;
pub mod types;

pub use api::PluginApiBindings;
pub use audit::{AuditEntry, AuditTrail};
pub use bridge::PromiseBridge;
pub use dispatcher::RcuDispatcher;
pub use errors::RuntimeError;
pub use loader::PluginLoader;
pub use native::NativeBindings;
pub use registry::{JsToolRegistry, PluginRegistry};
pub use runtime::{RuntimeConfig, RuntimeManager};
pub use sandbox::{DualTimeout, SandboxContext};
pub use server::{
    DISABLE_ALL_PLUGINS, FORCE_DENY, PluginSystem, SKIP_HOOKS, check_kill_switches, is_force_deny,
};
pub use timer::PluginTimer;
pub use transpiler::Transpiler;
pub use tui_api::{SlotContent, SlotRegistry, SlotType, TuiPluginApi};
pub use tui_system::TuiPluginSystem;
pub use types::{HandlerSlot, PreflightResult, ResolvedEntry, StaticAnalysis};

#[cfg(test)]
mod integration_tests;
