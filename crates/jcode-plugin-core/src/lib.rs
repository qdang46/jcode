pub mod config;
pub mod errors;
pub mod events;
pub mod manager;
pub mod manifest;
pub mod preflight;
pub mod security;
pub mod serde;
pub mod types;

pub use config::{
    DiscoveryPaths, PluginConfig, PluginSourceConfig, is_valid_package_name, sanitize_name,
};
pub use errors::PluginError;
pub use events::{
    EventInput, EventOutput, HandlerAction, HandlerResult, PermissionDecision, PluginEvent,
};
pub use manager::{InstalledPlugin, PluginManager, PluginSource};
pub use manifest::{
    PluginCapabilities, PluginEngines, PluginEntry, PluginFeature, PluginKind, PluginManifest,
    SettingSchema, ToolTier,
};
pub use preflight::{PreflightAnalyzer, PreflightResult, StaticAnalysis};
pub use security::{
    AccessDecision, AccessDecisionV2, AccessDefault, AccessMode, CapabilityAction, CapabilityChain,
    CapabilityChainV2, CapabilitySet, PolicyMode,
};
pub use types::{PluginId, PluginOrigin, PluginState, PluginVersion};

#[cfg(test)]
mod tests;
