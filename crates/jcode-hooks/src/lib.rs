//! Hooks module — lifecycle hooks for jcode events.

pub mod cli;
pub mod config;
pub mod dispatch;
pub mod execute;
pub mod matcher;
pub mod registry;
pub mod types;

pub use config::{
    load_hooks_config, AgentHandlerConfig, CommandHandlerConfig,
    HookEvent, HookHandlerConfig, HookSettings, HooksConfig,
    HttpHandlerConfig, PluginHandlerConfig,
};
pub use dispatch::{
    dispatch_hooks, get_hook_metrics, get_hook_metrics_for_event, ClassifiedOutcome,
    ClassifiedResult, DispatchConfig, DispatchStats,
};
pub use execute::{execute_hook, execute_command_hook, execute_http_hook};
pub use matcher::{matches, HookMatcher, MatcherContext, parse_multi_pattern};
pub use registry::{HookContext, HookRegistry};
pub use types::*;

#[cfg(test)]
mod tests;
