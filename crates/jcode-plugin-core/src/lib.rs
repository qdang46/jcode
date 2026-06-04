pub mod errors;
pub mod preflight;
pub mod types;
pub mod manifest;
pub mod security;
pub mod config;
pub mod events;
pub mod serde;

pub use errors::PluginError;
pub use types::{PluginId, PluginVersion, PluginState, PluginOrigin};
pub use manifest::{PluginManifest, PluginKind, PluginEntry, PluginCapabilities, PluginFeature, SettingSchema, PluginEngines};
pub use security::{CapabilityChain, CapabilitySet, AccessDefault, AccessMode, CapabilityAction, AccessDecision};
pub use config::{PluginConfig, PluginSource, DiscoveryPaths, is_valid_package_name, sanitize_name};
pub use events::{PluginEvent, EventInput, EventOutput, HandlerResult, HandlerAction, PermissionDecision};
pub use preflight::{PreflightAnalyzer, PreflightResult, StaticAnalysis};

#[cfg(test)]
mod tests;
