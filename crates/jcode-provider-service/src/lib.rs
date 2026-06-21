//! jcode-provider-service
//!
//! Catalog → Integration → Credential service traits and shared types for
//! jcode's provider resolution layer.
//!
//! This crate defines the *interfaces* that the rest of jcode (CLI, TUI,
//! session runner) talks to. Concrete implementations live in their own
//! crates:
//!
//! - [`credential`] — storage backends for credentials (in-memory, SQLite,
//!   OS keychain, mock for tests).
//! - [`integration`] — provider definitions, OAuth lifecycle, connection
//!   detection.
//! - [`catalog`] — provider/model registry, dynamic model lookups,
//!   `available()` / `default()` / `small()` resolvers.
//! - [`service`] — the high-level [`ProviderService`] facade that bundles the
//!   three layers and the [`RouteResolver`] that turns a `provider + model`
//!   request into a fully-prepared [`Route`].
//!
//! The old `Provider` trait in `jcode-provider-core` keeps working — this
//! crate sits *alongside* it, and Phase 6 of the master plan is when we
//! rewire consumers to flow through here.

pub mod aliases;
pub mod attempt;
pub mod boot;
pub mod callback_server;
pub mod catalog;
pub mod credential;
pub mod credential_rotation;
pub mod defaults;
pub mod error_classify;
pub mod expiry;
pub mod failover;
pub mod hooks;
pub mod idle_stream;
pub mod integration;
#[cfg(feature = "inventory")]
pub mod inventory;
#[cfg(feature = "metadata")]
pub mod metadata_profiles;
pub mod migrate;
pub mod model_prefs;
pub mod policy;
pub mod refresh;
pub mod registry;
pub mod retrofit;
pub mod retry_after;
pub mod route_provider;
pub mod runtime;
pub mod scrub;
pub mod service;
pub mod store;
pub mod tui_picker;
pub mod types;

pub use attempt::{AttemptStatus, OAuthAttempt};
pub use catalog::{CatalogService, ModelInfo, ProviderInfo};
pub use credential::{Credential, CredentialId, CredentialService, CredentialType};
pub use integration::{AuthMethod, ConnectionStatus, IntegrationService, LoginProvider};
pub use policy::{DenyListPolicy, PolicyService};
pub use service::{ProviderService, ResolvedRoute, RouteResolver};
pub use types::{ModelId, ProviderId, ProviderProfile};

/// Crate version, exposed for logging / diagnostics.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_nonempty() {
        assert!(!version().is_empty());
    }

    #[test]
    fn provider_id_and_model_id_are_strings() {
        let provider: ProviderId = "anthropic".into();
        let model: ModelId = "claude-sonnet-4-6".into();
        assert_eq!(provider.as_str(), "anthropic");
        assert_eq!(model.as_str(), "claude-sonnet-4-6");
    }
}
