//! Default [`ProviderService`] and [`RouteResolver`] implementations.
//!
//! Phase 3 + 6 of the master plan. This is the runtime facade the session
//! runner (and eventually the CLI) holds behind an
//! `Arc<dyn ProviderService>`. It composes the catalog, integration, and
//! credential services into a single handle and resolves
//! `(provider, model)` requests into concrete [`Route`]s.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use jcode_llm_core::endpoint::{Endpoint, PathSpec};
use jcode_llm_core::route::Route;
use jcode_llm_core::schema::ModelRef;

use crate::catalog::CatalogService;
use crate::credential::CredentialService;
use crate::integration::IntegrationService;
use crate::policy::PolicyService;
use crate::service::{ProviderService, ResolveError, ResolvedRoute, RouteResolver};
use crate::types::{ModelId, ProviderId, ProviderProfile};

/// Default composite service. Wraps catalog, integration, credential, and
/// policy services.
pub struct DefaultProviderService {
    catalog: Arc<dyn CatalogService>,
    integration: Arc<dyn IntegrationService>,
    credentials: Arc<dyn CredentialService>,
    policy: Arc<dyn PolicyService>,
}

impl DefaultProviderService {
    pub fn new(
        catalog: Arc<dyn CatalogService>,
        integration: Arc<dyn IntegrationService>,
        credentials: Arc<dyn CredentialService>,
    ) -> Self {
        Self::with_policy(catalog, integration, credentials, Arc::new(crate::policy::DenyListPolicy::from_env()))
    }

    /// Create a new provider service with an explicit policy.
    ///
    /// The policy is also injected into the catalog (via
    /// [`CatalogService::set_policy`]) so that [`available`] and
    /// [`remove_denied_providers`] are gated by it.
    pub fn with_policy(
        catalog: Arc<dyn CatalogService>,
        integration: Arc<dyn IntegrationService>,
        credentials: Arc<dyn CredentialService>,
        policy: Arc<dyn PolicyService>,
    ) -> Self {
        catalog.set_policy(policy.clone());
        Self {
            catalog,
            integration,
            credentials,
            policy,
        }
    }
}

impl ProviderService for DefaultProviderService {
    fn catalog(&self) -> &dyn CatalogService {
        self.catalog.as_ref()
    }

    fn integration(&self) -> &dyn IntegrationService {
        self.integration.as_ref()
    }

    fn credentials(&self) -> &dyn CredentialService {
        self.credentials.as_ref()
    }

    fn resolver(&self) -> &dyn RouteResolver {
        self
    }

    fn policy(&self) -> &dyn PolicyService {
        self.policy.as_ref()
    }
}

#[async_trait]
impl RouteResolver for DefaultProviderService {
    async fn resolve_route(
        &self,
        provider: &ProviderId,
        model: &ModelId,
    ) -> Result<ResolvedRoute, ResolveError> {
        // Look up provider + model info from the catalog (opencode-style).
        let info = self
            .catalog
            .provider(provider)
            .await
            .map_err(|_| ResolveError::UnknownProvider(provider.clone()))?;
        let _model_info = info
            .model(model)
            .ok_or_else(|| ResolveError::UnknownProvider(provider.clone()))?;

        let status = self
            .integration
            .detect(provider)
            .await
            .map_err(ResolveError::Integration)?;
        if !status.is_connected() {
            return Err(ResolveError::NotConnected(provider.clone()));
        }

        let route = Route {
            id: format!("{}/{}", provider, model),
            provider: ModelRef {
                provider_id: jcode_llm_core::schema::ProviderId::from(provider.as_str()),
                id: model.as_str().to_string(),
                variant: None,
            },
            protocol: info.protocol.clone(),
            endpoint: Endpoint {
                base_url: info.base_url.clone(),
                path: PathSpec::Static(info.path.clone()),
                query: None,
            },
            auth: HashMap::new(),
            framing: jcode_llm_core::framing::Framing::Sse,
            transport: jcode_llm_core::transport::Transport::Http,
            defaults: [("temperature".into(), serde_json::json!(0.0))].into(),
            body_overlay: Some(serde_json::json!({ "model": model.as_str() })),
        };
        Ok(ResolvedRoute {
            provider: provider.clone(),
            model: model.clone(),
            route,
        })
    }

    async fn resolve_profile(
        &self,
        profile: &ProviderProfile,
        default_model: Option<&ModelId>,
    ) -> Result<(ProviderId, ModelId), ResolveError> {
        let id = self.resolve_profile_id(profile).await?;
        let model = if let Some(m) = default_model {
            m.clone()
        } else {
            let models = self.catalog.models(&id).await?;
            models
                .first()
                .map(|m| m.id.clone())
                .ok_or(crate::catalog::CatalogError::NoModels(id.clone()))?
        };
        Ok((id, model))
    }
}

impl DefaultProviderService {
    /// Resolve just the provider id from a [`ProviderProfile`]. Walks
    /// the integration registry for label-based and named lookups.
    fn resolve_profile_id<'a>(
        &'a self,
        profile: &'a ProviderProfile,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ProviderId, ResolveError>> + Send + 'a>,
    > {
        Box::pin(async move {
            match profile {
                ProviderProfile::ById { id } => Ok(id.clone()),
                ProviderProfile::WithAuth { id, .. } => Ok(id.clone()),
                ProviderProfile::ByLabel { label } => {
                    let wanted = label.to_ascii_lowercase();
                    for p in self.integration.list().await? {
                        if p.label.to_ascii_lowercase() == wanted {
                            return Ok(p.id);
                        }
                    }
                    Err(ResolveError::UnknownProvider(ProviderId::from(
                        profile.describe(),
                    )))
                }
                ProviderProfile::Named { profile: name } => {
                    // A named profile is shorthand for a label-based lookup
                    // (e.g. "work" -> label "Work"). A more sophisticated
                    // implementation would consult a profile map from config.
                    self.resolve_profile_id(&ProviderProfile::ByLabel {
                        label: name.clone(),
                    })
                    .await
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelInfo, ModelTier, ProviderInfo};
    use crate::credential::{CredentialService, CredentialType};
    use crate::integration::{AuthMethod, LoginProvider};
    use crate::store::{KeyringCredentialStore, PersistentIntegration};
    use jcode_keyring_store::MockKeyringStore;

    async fn fixture() -> (
        Arc<dyn CatalogService>,
        Arc<dyn IntegrationService>,
        Arc<dyn CredentialService>,
    ) {
        let catalog = Arc::new(InMemoryCatalog::new());
        catalog
            .register_provider(ProviderInfo {
                id: "anthropic".into(),
                name: "Anthropic".into(),
                enabled: true,
                is_connected: true,
                models: vec![ModelInfo {
                    id: "claude-haiku-4-5".into(),
                    provider: "anthropic".into(),
                    name: "Claude Haiku 4.5".into(),
                    cost_per_million_input: Some(0.8),
                    cost_per_million_output: Some(4.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Nano),

                    release_date: None,
                }],
            api_key: None,
            base_url: "https://api.anthropic.com".into(),
            path: "/v1/messages".into(),
            protocol: "anthropic-messages-2023-01-01".into(),
            })
            .await
            .unwrap();

        let keyring = Arc::new(MockKeyringStore::new());
        let creds: Arc<dyn CredentialService> = Arc::new(KeyringCredentialStore::new(keyring));
        let integration: Arc<dyn IntegrationService> = Arc::new(PersistentIntegration::<
            MockKeyringStore,
        >::new(creds.clone()));
        integration
            .register(LoginProvider {
                id: "anthropic".into(),
                label: "Anthropic".into(),
                auth_methods: vec![AuthMethod::ApiKey {
                    env_var: "ANTHROPIC_API_KEY".into(),
                }],
                env_keys: vec!["ANTHROPIC_API_KEY".into()],
                oauth_preferred: false,
            })
            .await
            .unwrap();
        creds
            .upsert(crate::credential::Credential::new(
                "anthropic".into(),
                "default",
                CredentialType::ApiKey {
                    key: "sk-test".into(),
                },
            ))
            .await
            .unwrap();

        (catalog, integration, creds)
    }

    #[tokio::test]
    async fn resolve_route_returns_prepared_route() {
        let (cat, int, creds) = fixture().await;
        let svc = DefaultProviderService::new(cat, int, creds);
        let r = svc
            .resolver()
            .resolve_route(&"anthropic".into(), &"claude-haiku-4-5".into())
            .await
            .unwrap();
        assert_eq!(r.provider.as_str(), "anthropic");
        assert_eq!(r.model.as_str(), "claude-haiku-4-5");
        assert_eq!(r.route.protocol, "anthropic-messages-2023-01-01");
        assert_eq!(r.route.endpoint.base_url, "https://api.anthropic.com");
    }

    #[tokio::test]
    async fn resolve_route_errors_when_not_connected() {
        let (cat, int, creds) = fixture().await;
        let creds_clone = creds.clone();
        let all = creds.list(&"anthropic".into()).await.unwrap();
        for c in all {
            creds_clone.delete(&c.id).await.unwrap();
        }
        let svc = DefaultProviderService::new(cat, int, creds_clone);
        let err = svc
            .resolver()
            .resolve_route(&"anthropic".into(), &"claude-haiku-4-5".into())
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::NotConnected(_)));
    }

    #[tokio::test]
    async fn resolve_route_errors_for_unknown_provider() {
        let (cat, int, creds) = fixture().await;
        let svc = DefaultProviderService::new(cat, int, creds);
        let err = svc
            .resolver()
            .resolve_route(&"mystery".into(), &"m".into())
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::UnknownProvider(_)));
    }

    #[tokio::test]
    async fn resolve_profile_by_id_uses_provided_model() {
        let (cat, int, creds) = fixture().await;
        let svc = DefaultProviderService::new(cat, int, creds);
        let profile = ProviderProfile::ById {
            id: "anthropic".into(),
        };
        let (p, m) = svc
            .resolver()
            .resolve_profile(&profile, Some(&"claude-haiku-4-5".into()))
            .await
            .unwrap();
        assert_eq!(p.as_str(), "anthropic");
        assert_eq!(m.as_str(), "claude-haiku-4-5");
    }

    #[tokio::test]
    async fn resolve_profile_defaults_to_first_model() {
        let (cat, int, creds) = fixture().await;
        let svc = DefaultProviderService::new(cat, int, creds);
        let profile = ProviderProfile::ById {
            id: "anthropic".into(),
        };
        let (p, m) = svc
            .resolver()
            .resolve_profile(&profile, None)
            .await
            .unwrap();
        assert_eq!(p.as_str(), "anthropic");
        assert_eq!(m.as_str(), "claude-haiku-4-5");
    }

    #[tokio::test]
    async fn resolve_profile_by_label_resolves_to_id() {
        let (cat, int, creds) = fixture().await;
        let svc = DefaultProviderService::new(cat, int, creds);
        // Exact case.
        let profile = ProviderProfile::ByLabel {
            label: "Anthropic".into(),
        };
        let (p, _) = svc
            .resolver()
            .resolve_profile(&profile, Some(&"claude-haiku-4-5".into()))
            .await
            .unwrap();
        assert_eq!(p.as_str(), "anthropic");
        // Case-insensitive.
        let profile = ProviderProfile::ByLabel {
            label: "anthropic".into(),
        };
        let (p, _) = svc
            .resolver()
            .resolve_profile(&profile, Some(&"claude-haiku-4-5".into()))
            .await
            .unwrap();
        assert_eq!(p.as_str(), "anthropic");
    }

    #[tokio::test]
    async fn resolve_profile_by_label_unknown_errors() {
        let (cat, int, creds) = fixture().await;
        let svc = DefaultProviderService::new(cat, int, creds);
        let profile = ProviderProfile::ByLabel {
            label: "Mystery".into(),
        };
        let err = svc
            .resolver()
            .resolve_profile(&profile, None)
            .await
            .unwrap_err();
        assert!(matches!(err, ResolveError::UnknownProvider(_)));
    }

    #[tokio::test]
    async fn resolve_profile_named_falls_through_to_label() {
        let (cat, int, creds) = fixture().await;
        let svc = DefaultProviderService::new(cat, int, creds);
        let profile = ProviderProfile::Named {
            profile: "Anthropic".into(),
        };
        let (p, _) = svc
            .resolver()
            .resolve_profile(&profile, Some(&"claude-haiku-4-5".into()))
            .await
            .unwrap();
        assert_eq!(p.as_str(), "anthropic");
    }
}
