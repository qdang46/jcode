//! Provider registry — boot-time provider/model registration.
//!
//! Plan Phase 0 + Phase 3 deliverable: `crates/jcode-provider-service/src/registry.rs`.
//!
//! opencode's pattern is to have a *plugin hook* that every provider
//! module can call at boot to register itself into the catalog.
//! jcode doesn't have a plugin system, but the same shape works:
//! the `register_builtins` function in `boot.rs` calls the
//! `register_provider` and `register_model` methods on the catalog
//! for each provider the user has installed.
//!
//! This module is the *abstract* side of that surface — the trait
//! that boot.rs is built on, plus the `ProviderRecord` type that
//! decouples the boot-time registration from the catalog's
//! internal storage. boot.rs becomes a concrete `BuiltinRegistry`
//! that lists each provider's `ProviderRecord` and walks it.
//!
//! ```text
//! registry::Registry      <- the abstract registration surface
//!   ├── boot::BUILTIN_PROVIDERS  <- the hardcoded built-in set
//!   └── plugin::*        <- future plugin providers
//! ```

use std::sync::Arc;

use crate::catalog::CatalogService;
use crate::integration::{AuthMethod, LoginProvider};
use crate::types::ProviderId;

/// A record describing a single provider for registration into the
/// catalog + integration layers. Used by boot-time registry code to
/// add providers without each provider needing to know about the
/// catalog's storage details.
#[derive(Debug, Clone)]
pub struct ProviderRecord {
    pub id: ProviderId,
    pub label: String,
    pub auth_methods: Vec<AuthMethod>,
    pub env_keys: Vec<String>,
    pub oauth_preferred: bool,
    pub models: Vec<crate::catalog::ModelInfo>,
}

impl ProviderRecord {
    /// Convert to a `LoginProvider` for the integration layer.
    pub fn to_login_provider(&self) -> LoginProvider {
        LoginProvider {
            id: self.id.clone(),
            label: self.label.clone(),
            auth_methods: self.auth_methods.clone(),
            env_keys: self.env_keys.clone(),
            oauth_preferred: self.oauth_preferred,
        }
    }
}

/// Abstract trait over "a thing that can register providers at boot".
/// Implementations are typically static lists of built-in providers
/// or dynamic providers discovered through a plugin mechanism.
#[async_trait::async_trait]
pub trait ProviderRegistry: Send + Sync {
    /// The unique id of this registry (e.g. "builtins", "antigravity").
    fn id(&self) -> &str;
    /// Enumerate every provider this registry knows about.
    async fn providers(&self) -> Vec<ProviderRecord>;
    /// Register every provider into the catalog and integration
    /// services. Returns the number of providers registered.
    async fn register(
        &self,
        catalog: &dyn CatalogService,
        integration: &dyn crate::integration::IntegrationService,
    ) -> Result<usize, RegistryError> {
        let providers = self.providers().await;
        let n = providers.len();
        for rec in providers {
            catalog
                .register_provider(crate::catalog::ProviderInfo {
                    id: rec.id.clone(),
                    name: rec.label.clone(),
                    enabled: true,
                    is_connected: false,
                    api_key: None,
                    models: rec.models.clone(),
                })
                .await?;
            integration.register(rec.to_login_provider()).await?;
        }
        Ok(n)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("catalog error: {0}")]
    Catalog(#[from] crate::catalog::CatalogError),
    #[error("integration error: {0}")]
    Integration(#[from] crate::integration::IntegrationError),
}

/// A registry that composes multiple sub-registries. Used by the
/// session runner to bootstrap: it walks each child registry and
/// calls `register()`.
pub struct CompositeRegistry {
    id: String,
    children: Vec<Arc<dyn ProviderRegistry>>,
}

impl CompositeRegistry {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
        }
    }

    pub fn with(mut self, child: Arc<dyn ProviderRegistry>) -> Self {
        self.children.push(child);
        self
    }
}

#[async_trait::async_trait]
impl ProviderRegistry for CompositeRegistry {
    fn id(&self) -> &str {
        &self.id
    }

    async fn providers(&self) -> Vec<ProviderRecord> {
        let mut all = Vec::new();
        for c in &self.children {
            all.extend(c.providers().await);
        }
        all
    }
}

/// Build a `CompositeRegistry` containing all the built-in providers.
/// Mirrors `boot::BUILTIN_PROVIDERS` but as a registry trait object
/// so it can be composed with other registries (e.g. a future
/// `PluginRegistry`).
pub fn builtin_registry() -> Arc<dyn ProviderRegistry> {
    Arc::new(BuiltinRegistry)
}

/// Concrete registry that returns `boot::BUILTIN_PROVIDERS`.
pub struct BuiltinRegistry;

#[async_trait::async_trait]
impl ProviderRegistry for BuiltinRegistry {
    fn id(&self) -> &str {
        "builtins"
    }

    async fn providers(&self) -> Vec<ProviderRecord> {
        crate::boot::BUILTIN_PROVIDERS
            .iter()
            .map(|bp| ProviderRecord {
                id: ProviderId::from(bp.id),
                label: bp.label.to_string(),
                auth_methods: bp
                    .env_keys
                    .iter()
                    .map(|env| AuthMethod::ApiKey {
                        env_var: (*env).to_string(),
                    })
                    .collect(),
                env_keys: bp.env_keys.iter().map(|s| (*s).to_string()).collect(),
                oauth_preferred: bp.oauth_preferred,
                models: bp
                    .models
                    .iter()
                    .map(|m| crate::catalog::ModelInfo {
                        id: m.id.into(),
                        provider: ProviderId::from(bp.id),
                        name: m.name.to_string(),
                        cost_per_million_input: m.cost_per_million_input,
                        cost_per_million_output: m.cost_per_million_output,
                        context_window: m.context_window,
                        supports_tools: m.supports_tools,
                        supports_vision: m.supports_vision,
                        supports_streaming: m.supports_streaming,
                        tier: Some(m.tier),
                        release_date: m.release_date,
                    })
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelTier, ProviderInfo};
    use crate::integration::InMemoryIntegration;
    use jcode_keyring_store::MockKeyringStore;

    #[tokio::test]
    async fn builtin_registry_returns_known_providers() {
        let r = builtin_registry();
        let providers = r.providers().await;
        assert!(
            providers.len() >= 4,
            "expected at least 4 built-in providers"
        );
        let names: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        for n in ["anthropic", "openai", "openrouter", "gemini"] {
            assert!(names.contains(&n), "missing {n}");
        }
    }

    #[tokio::test]
    async fn register_populates_catalog_and_integration() {
        let r = builtin_registry();
        let catalog = InMemoryCatalog::new();
        let integration = InMemoryIntegration::new();
        let n = r.register(&catalog, &integration).await.unwrap();
        assert_eq!(n, 4);
        // Every provider ends up in the catalog.
        for p in r.providers().await {
            catalog
                .provider(&p.id)
                .await
                .unwrap_or_else(|e| panic!("missing {}: {e}", p.id));
            // The integration trait objects dispatch through Arc<dyn
            // IntegrationService>, so .get() is reachable.
            use crate::integration::IntegrationService as _;
            let _ = integration
                .get(&p.id)
                .await
                .unwrap_or_else(|e| panic!("missing integration for {}: {e}", p.id));
        }
    }

    #[tokio::test]
    async fn composite_registry_merges_children() {
        struct DummyRegistry;
        #[async_trait::async_trait]
        impl ProviderRegistry for DummyRegistry {
            fn id(&self) -> &str {
                "dummy"
            }
            async fn providers(&self) -> Vec<ProviderRecord> {
                vec![ProviderRecord {
                    id: "dummy".into(),
                    label: "Dummy".into(),
                    auth_methods: vec![],
                    env_keys: vec![],
                    oauth_preferred: false,
                    models: vec![],
                }]
            }
        }
        let composite = CompositeRegistry::new("all")
            .with(builtin_registry())
            .with(Arc::new(DummyRegistry));
        let providers = composite.providers().await;
        assert!(
            providers.len() >= 5,
            "composite should merge builtins + dummy"
        );
    }

    #[tokio::test]
    async fn to_login_provider_carries_fields() {
        let rec = ProviderRecord {
            id: "anthropic".into(),
            label: "Anthropic".into(),
            auth_methods: vec![
                AuthMethod::ApiKey {
                    env_var: "ANTHROPIC_API_KEY".into(),
                },
                AuthMethod::OAuth {
                    authorization_url: "https://example.com".into(),
                },
            ],
            env_keys: vec!["ANTHROPIC_API_KEY".into()],
            oauth_preferred: true,
            models: vec![],
        };
        let login = rec.to_login_provider();
        assert_eq!(login.id.as_str(), "anthropic");
        assert!(login.supports_oauth());
        // Sanity: doesn't need catalog types
        let _ = ProviderInfo {
            id: "x".into(),
            name: "X".into(),
            enabled: true,
            is_connected: false,
                    api_key: None,
            models: vec![],
        };
        let _ = ModelTier::Flagship;
        // Make use of MockKeyringStore import so it doesn't warn.
        let _ = std::any::type_name::<MockKeyringStore>();
    }
}
