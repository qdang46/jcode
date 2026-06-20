//! Session runner entry point.
//!
//! Plan criterion 7:
//!
//!   > [ ] Agent::new() resolves via Catalog → Integration → Route
//!
//! The plan calls for replacing the current `Agent::new()` →
//! `ActiveProvider` resolution chain with a Catalog-based resolution.
//! This module is the *new* shape of that resolution: a single
//! function `start_session()` that takes the user's selection
//! (provider + model), asks the `ProviderService` facade to resolve
//! it, and returns a `Session` handle that the rest of the runtime
//! (transport, agent loop) can drive.
//!
//! The actual `Agent::new()` in `jcode-app-core` cannot be rewired
//! until the broken `jcode-tui` crate is repaired; this module gives
//! downstream consumers the *exact* shape of the new resolution so
//! the swap is a one-line change once the dependency is healthy.

use std::sync::Arc;

use jcode_llm_core::route::Route;

use crate::defaults::ProviderDefaults;
use crate::service::{ProviderService, ResolvedRoute};
use crate::types::{ModelId, ProviderId, ProviderProfile};

/// The new-shape session handle. The full `Agent` struct (in
/// `jcode-app-core`) has many more fields; this is the *minimum*
/// the new resolution path needs to return so downstream code can
/// be rewired incrementally.
#[derive(Debug, Clone)]
pub struct Session {
    pub provider: ProviderId,
    pub model: ModelId,
    pub route: Route,
}

impl Session {
    /// Convenience: short `provider/model` string for diagnostics.
    pub fn describe(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("could not resolve profile: {0}")]
    Resolve(#[from] crate::service::ResolveError),
    #[error("no default model available (no providers connected)")]
    NoDefault,
    #[error("provider-defaults file error: {0}")]
    Defaults(String),
    #[error("io error: {0}")]
    Io(String),
}

/// Resolve a user selection (CLI flag, config default, or TUI pick)
/// into a fully-prepared session.
///
/// The selection precedence is:
///  1. Explicit `--provider <flag> --model <flag>` (the most
///     specific override).
///  2. Per-provider default in `~/.jcode/provider-defaults.json`.
///  3. Global default in the same file.
///  4. `Catalog::default()` heuristic (flagship model of the first
///     available provider).
///
/// Returns the resolved session, ready to drive the transport.
pub async fn start_session(
    svc: &DefaultProviderService,
    cli_profile: Option<&ProviderProfile>,
    cli_model: Option<&ModelId>,
) -> Result<Session, SessionError> {
    let defaults = load_defaults();

    // 1. Explicit CLI override.
    if let (Some(profile), Some(model)) = (cli_profile, cli_model) {
        let (provider, resolved_model) = svc
            .resolver()
            .resolve_profile(profile, Some(model))
            .await?;
        return finish(svc, provider, resolved_model).await;
    }

    // 2-3. Persisted defaults.
    if let Some(profile) = cli_profile {
        let (provider, base_model) = svc
            .resolver()
            .resolve_profile(profile, None)
            .await?;
        let resolved = defaults
            .as_ref()
            .and_then(|d| d.resolve(&provider, Some(base_model.clone())))
            .unwrap_or(base_model);
        return finish(svc, provider, resolved).await;
    }

    // 4. Catalog default.
    let (provider, fallback_model) = svc
        .catalog()
        .default()
        .await
        .map_err(|_| SessionError::NoDefault)?;
    let model = defaults
        .as_ref()
        .and_then(|d| d.resolve(&provider, Some(fallback_model.clone())))
        .unwrap_or(fallback_model);
    finish(svc, provider, model).await
}

async fn finish(
    svc: &DefaultProviderService,
    provider: ProviderId,
    model: ModelId,
) -> Result<Session, SessionError> {
    let ResolvedRoute { route, .. } = svc
        .resolver()
        .resolve_route(&provider, &model)
        .await?;
    // Record the selection in the persistent recents so the next
    // session can surface it (per the plan: model picker surfaces
    // recents after favorites).
    record_recent(&provider, &model);
    Ok(Session {
        provider,
        model,
        route,
    })
}

/// Push the (provider, model) selection into ~/.jcode/model_prefs.json
/// under 'recents'. LIFO + dedup + 10-entry cap, matching the
/// tui_picker::PickerState::push_recent semantics.
fn record_recent(provider: &ProviderId, model: &ModelId) {
    use crate::model_prefs::ModelPrefs;
    let Some(path) = crate::model_prefs::default_path() else {
        return;
    };
    let mut prefs = ModelPrefs::load(&path).unwrap_or_default();
    prefs.push_recent(provider.clone(), model.clone());
    if let Err(e) = prefs.save(&path) {
        tracing::warn!(error = %e, "failed to save model_prefs recents");
    }
}

fn load_defaults() -> Option<ProviderDefaults> {
    let path = crate::defaults::default_path()?;
    ProviderDefaults::load(&path)
        .map_err(|e| {
            tracing::warn!(error = %e, "failed to load provider defaults; continuing with catalog heuristics");
        })
        .ok()
}

// Convenience re-exports so callers don't need to import
// jcode_provider_service::service::RouteResolver etc. themselves.
pub use crate::store::DefaultProviderService;

/// One-shot helper for tests and small binaries: build a default
/// service with the real keychain and the built-in providers
/// registered, then call `start_session()`.
pub async fn quick_session(
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
) -> Result<Session, SessionError> {
    use jcode_keyring_store::DefaultKeyringStore;

    let keyring = Arc::new(DefaultKeyringStore::new());
    let credentials: Arc<dyn crate::credential::CredentialService> = Arc::new(
        crate::store::KeyringCredentialStore::new(keyring),
    );
    let integration: Arc<dyn crate::integration::IntegrationService> = Arc::new(
        crate::store::PersistentIntegration::<DefaultKeyringStore>::new(
            credentials.clone(),
        ),
    );
    let catalog: Arc<dyn crate::catalog::CatalogService> =
        Arc::new(crate::catalog::InMemoryCatalog::new());
    crate::boot::register_builtins::<DefaultKeyringStore>(
        catalog.as_ref(),
        integration.as_ref(),
    )
    .await
    .map_err(|e| SessionError::Defaults(e.to_string()))?;
    let svc = DefaultProviderService::new(catalog, integration, credentials);

    let profile = cli_provider
        .map(|p| ProviderProfile::ById { id: p.into() });
    let model = cli_model.map(|m| ModelId::from(m));
    start_session(&svc, profile.as_ref(), model.as_ref()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CatalogService;
    use crate::store::PersistentIntegration;
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelInfo, ModelTier, ProviderInfo};
    use crate::defaults::ProviderDefaults;
    use crate::integration::{AuthMethod, InMemoryIntegration, LoginProvider};
    use crate::store::KeyringCredentialStore;
    use jcode_keyring_store::MockKeyringStore;

    async fn fixture() -> DefaultProviderService {
        let catalog = InMemoryCatalog::new();
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
                }],
            })
            .await
            .unwrap();
        let keyring = Arc::new(MockKeyringStore::new());
        let creds: Arc<dyn crate::credential::CredentialService> =
            Arc::new(KeyringCredentialStore::new(keyring));
        let integration: Arc<dyn crate::integration::IntegrationService> =
            Arc::new(PersistentIntegration::<MockKeyringStore>::new(creds.clone()));
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
                crate::credential::CredentialType::ApiKey { key: "x".into() },
            ))
            .await
            .unwrap();
        DefaultProviderService::new(
            Arc::new(catalog) as Arc<dyn CatalogService>,
            integration,
            creds,
        )
    }

    #[tokio::test]
    async fn explicit_profile_and_model_resolves() {
        let svc = fixture().await;
        let profile = ProviderProfile::ById {
            id: "anthropic".into(),
        };
        let s = start_session(&svc, Some(&profile), Some(&"claude-haiku-4-5".into()))
            .await
            .unwrap();
        assert_eq!(s.describe(), "anthropic/claude-haiku-4-5");
    }

    #[tokio::test]
    async fn default_falls_back_to_catalog() {
        let svc = fixture().await;
        let s = start_session(&svc, None, None).await.unwrap();
        assert_eq!(s.describe(), "anthropic/claude-haiku-4-5");
    }

    #[tokio::test]
    async fn by_label_profile_resolves_via_integration() {
        // The ByLabel profile goes through the integration layer
        // to find a provider whose label matches.
        let svc = fixture().await;
        let profile = ProviderProfile::ByLabel {
            label: "Anthropic".into(),
        };
        let s = start_session(&svc, Some(&profile), Some(&"claude-haiku-4-5".into()))
            .await
            .unwrap();
        assert_eq!(s.describe(), "anthropic/claude-haiku-4-5");
    }

    #[tokio::test]
    async fn with_auth_profile_resolves() {
        // ProviderProfile::WithAuth carries the provider id + an
        // auth suffix; the resolver treats it like ById.
        let svc = fixture().await;
        let profile = ProviderProfile::WithAuth {
            id: "anthropic".into(),
            auth: "api-key".into(),
        };
        let s = start_session(&svc, Some(&profile), Some(&"claude-haiku-4-5".into()))
            .await
            .unwrap();
        assert_eq!(s.describe(), "anthropic/claude-haiku-4-5");
    }

    #[tokio::test]
    async fn errors_when_no_providers_connected() {
        let catalog = InMemoryCatalog::new();
        let keyring = Arc::new(MockKeyringStore::new());
        let creds: Arc<dyn crate::credential::CredentialService> =
            Arc::new(KeyringCredentialStore::new(keyring));
        let integration: Arc<dyn crate::integration::IntegrationService> =
            Arc::new(InMemoryIntegration::new());
        let svc = DefaultProviderService::new(
            Arc::new(catalog) as Arc<dyn CatalogService>,
            integration,
            creds,
        );
        let err = start_session(&svc, None, None).await.unwrap_err();
        assert!(matches!(err, SessionError::NoDefault));
    }

    #[test]
    fn describe_returns_provider_slash_model() {
        let s = Session {
            provider: "anthropic".into(),
            model: "claude-haiku-4-5".into(),
            route: jcode_llm_core::route::Route::new(
                "anthropic",
                jcode_llm_core::schema::ModelRef {
                    provider_id: "anthropic".into(),
                    id: "claude-haiku-4-5".into(),
                    variant: None,
                },
            ),
        };
        assert_eq!(s.describe(), "anthropic/claude-haiku-4-5");
    }
}
