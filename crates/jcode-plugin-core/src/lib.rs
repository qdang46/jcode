pub mod config;
pub mod errors;
pub mod events;
pub mod manifest;
pub mod preflight;
pub mod security;
pub mod serde;
pub mod types;

pub use config::{
    DiscoveryPaths, PluginConfig, PluginSource, is_valid_package_name, sanitize_name,
};
pub use errors::PluginError;
pub use events::{
    EventInput, EventOutput, HandlerAction, HandlerResult, PermissionDecision, PluginEvent,
};
pub use manifest::{
    PluginCapabilities, PluginEngines, PluginEntry, PluginFeature, PluginKind, PluginManifest,
    SettingSchema,
};
pub use preflight::{PreflightAnalyzer, PreflightResult, StaticAnalysis};
pub use security::{
    AccessDecision, AccessDefault, AccessMode, CapabilityAction, CapabilityChain, CapabilitySet,
};
pub use types::{PluginId, PluginOrigin, PluginState, PluginVersion};

#[cfg(test)]
mod tests;
