//! High-level provider service facade.
//!
//! Phase 0 brings together the three layers:
//!
//! - [`CatalogService`] (catalog.rs) — providers and models.
//! - [`IntegrationService`] (integration.rs) — credentials and OAuth.
//! - [`CredentialService`] (credential.rs) — credential storage.
//!
//! [`ProviderService`] bundles them into a single `Send + Sync` handle that
//! the rest of jcode (CLI, TUI, session runner) can hold behind an
//! `Arc<dyn ProviderService>`. It also exposes the [`RouteResolver`] that
//! turns a `(provider, model)` request into a fully-prepared
//! [`jcode_llm_core::route::Route`].

use async_trait::async_trait;
use jcode_llm_core::route::Route;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::CatalogService;
use crate::credential::CredentialService;
use crate::integration::IntegrationService;
use crate::types::{ModelId, ProviderId, ProviderProfile};

/// Result of resolving a `(provider, model)` pair to a concrete
/// [`Route`]. Carries the resolved ids alongside the route so callers can
/// log/cache them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRoute {
    pub provider: ProviderId,
    pub model: ModelId,
    pub route: Route,
}

/// Resolves a high-level `provider + model` selection into a concrete
/// [`Route`]. Implementations typically consult the catalog for the
/// model metadata and the integration layer for the auth route.
#[async_trait]
pub trait RouteResolver: Send + Sync {
    /// Resolve a `(provider, model)` pair to a [`Route`].
    async fn resolve_route(
        &self,
        provider: &ProviderId,
        model: &ModelId,
    ) -> Result<ResolvedRoute, ResolveError>;

    /// Resolve a [`ProviderProfile`] (CLI flag, config block) to a
    /// concrete `(provider, model)` pair. Default model is taken from
    /// the catalog.
    async fn resolve_profile(
        &self,
        profile: &ProviderProfile,
        default_model: Option<&ModelId>,
    ) -> Result<(ProviderId, ModelId), ResolveError>;
}

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("unknown provider: {0}")]
    UnknownProvider(ProviderId),
    #[error("unknown model: {provider}/{model}")]
    UnknownModel {
        provider: ProviderId,
        model: ModelId,
    },
    #[error("provider {0} is not connected (no credentials)")]
    NotConnected(ProviderId),
    #[error("no route registered for {provider}/{model}")]
    NoRoute {
        provider: ProviderId,
        model: ModelId,
    },
    #[error("catalog error: {0}")]
    Catalog(#[from] crate::catalog::CatalogError),
    #[error("integration error: {0}")]
    Integration(#[from] crate::integration::IntegrationError),
    #[error("credential error: {0}")]
    Credential(#[from] crate::credential::CredentialError),
}

/// The high-level provider service facade.
#[async_trait]
pub trait ProviderService: Send + Sync {
    /// The catalog layer.
    fn catalog(&self) -> &dyn CatalogService;
    /// The integration layer.
    fn integration(&self) -> &dyn IntegrationService;
    /// The credential storage layer.
    fn credentials(&self) -> &dyn CredentialService;
    /// The route resolver (typically backed by the catalog + integration).
    fn resolver(&self) -> &dyn RouteResolver;
    /// The policy service (deny-list checking).
    fn policy(&self) -> &dyn crate::policy::PolicyService;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_route_serializes_without_route_body() {
        // Sanity check that the struct shape is stable. The full route
        // serialization is covered by `jcode-llm-core`'s own tests.
        let r = ResolvedRoute {
            provider: "anthropic".into(),
            model: "claude-haiku-4-5".into(),
            route: Route::new(
                "anthropic",
                jcode_llm_core::schema::ModelRef {
                    provider_id: "anthropic".into(),
                    id: "claude-haiku-4-5".into(),
                    variant: None,
                },
            ),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("anthropic"));
        assert!(s.contains("claude-haiku-4-5"));
    }

    #[test]
    fn resolve_error_displays_unknown_provider() {
        let e = ResolveError::UnknownProvider(ProviderId::from("mystery"));
        let s = format!("{e}");
        assert!(s.contains("mystery"));
        assert!(s.contains("unknown provider"));
    }

    #[test]
    fn resolve_error_displays_unknown_model() {
        let e = ResolveError::UnknownModel {
            provider: ProviderId::from("anthropic"),
            model: ModelId::from("claude-fake"),
        };
        let s = format!("{e}");
        assert!(s.contains("anthropic"));
        assert!(s.contains("claude-fake"));
        assert!(s.contains("unknown model"));
    }

    #[test]
    fn resolve_error_displays_not_connected() {
        let e = ResolveError::NotConnected(ProviderId::from("anthropic"));
        let s = format!("{e}");
        assert!(s.contains("anthropic"));
        assert!(s.contains("not connected"));
    }

    #[test]
    fn resolve_error_displays_no_route() {
        let e = ResolveError::NoRoute {
            provider: ProviderId::from("anthropic"),
            model: ModelId::from("claude-fake"),
        };
        let s = format!("{e}");
        assert!(s.contains("anthropic"));
        assert!(s.contains("claude-fake"));
        assert!(s.contains("no route"));
    }

    #[test]
    fn resolve_error_from_catalog_error() {
        // Verify the #[from] conversion works as advertised.
        use crate::catalog::CatalogError;
        let inner = CatalogError::UnknownProvider(ProviderId::from("mystery"));
        let outer: ResolveError = inner.into();
        let s = format!("{outer}");
        assert!(s.contains("mystery"));
    }
}

#[test]
fn resolve_error_displays_unknown_provider() {
    let e = ResolveError::UnknownProvider(ProviderId::from("mystery"));
    let s = format!("{e}");
    assert!(s.contains("mystery"));
    assert!(s.contains("unknown provider"));
}

#[test]
fn resolve_error_displays_unknown_model() {
    let e = ResolveError::UnknownModel {
        provider: ProviderId::from("anthropic"),
        model: ModelId::from("claude-fake"),
    };
    let s = format!("{e}");
    assert!(s.contains("anthropic"));
    assert!(s.contains("claude-fake"));
    assert!(s.contains("unknown model"));
}

#[test]
fn resolve_error_displays_not_connected() {
    let e = ResolveError::NotConnected(ProviderId::from("anthropic"));
    let s = format!("{e}");
    assert!(s.contains("anthropic"));
    assert!(s.contains("not connected"));
}

#[test]
fn resolve_error_displays_no_route() {
    let e = ResolveError::NoRoute {
        provider: ProviderId::from("anthropic"),
        model: ModelId::from("claude-fake"),
    };
    let s = format!("{e}");
    assert!(s.contains("anthropic"));
    assert!(s.contains("claude-fake"));
    assert!(s.contains("no route"));
}
