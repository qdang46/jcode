//! Rate-limit failover chain.
//!
//! Plan criterion 12:
//!
//!   > [ ] Rate-limit failover walks Catalog.provider.available() chain
//!
//! When a provider returns a rate-limit response (HTTP 429 or
//! "rate_limit_exceeded" in the body), the runtime should walk the
//! `Catalog::available()` chain — the same chain the resolver uses
//! to find a default — and try the next provider. This module is
//! the abstraction over that walk: it takes a starting
//! `(provider, model)`, a `CatalogService`, and an
//! `IntegrationService`, and produces the next viable target.
//!
//! ```text
//!   1. caller asks for anthropic/claude-sonnet-4-6
//!   2. transport returns 429
//!   3. failover::next_target(catalog, integration, "anthropic",
//!                            "claude-sonnet-4-6") ->
//!                            Some(("openai", "gpt-5.1"))
//!   4. caller retries with the new target
//! ```

use crate::catalog::CatalogService;
use crate::integration::IntegrationService;
use crate::types::{ModelId, ProviderId};

/// The next target in the failover chain, or `None` if the chain
/// is exhausted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailoverTarget {
    pub provider: ProviderId,
    pub model: ModelId,
    /// 1-based index in the available chain (1 = first available
    /// provider, 2 = second, etc.).
    pub chain_index: usize,
}

/// Compute the next viable failover target after the given
/// `(provider, model)` request hits a rate limit.
///
/// Strategy: walk `Catalog::available()` in the same order the
/// catalog uses for `default()`, skipping the failing provider
/// itself. Pick the first available provider's *flagship* model
/// (or its first listed model if no flagship is registered).
/// Returns `None` when no other providers are available.
pub async fn next_target(
    catalog: &dyn CatalogService,
    integration: &dyn IntegrationService,
    failing: (&ProviderId, &ModelId),
) -> Result<Option<FailoverTarget>, FailoverError> {
    // Iterate the available chain in a deterministic order so failover
    // is reproducible. Sorted by provider id.
    let mut available = catalog.available().await?;
    available.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    // Skip the failing provider.
    let mut after = false;
    for (idx, p) in available.iter().enumerate() {
        let idx = idx + 1; // 1-based
        if after {
            // Pick flagship first, fall back to first model.
            let pick = p
                .models
                .iter()
                .find(|m| matches!(m.tier, Some(crate::catalog::ModelTier::Flagship)))
                .or_else(|| p.models.first())
                .cloned();
            if let Some(m) = pick {
                return Ok(Some(FailoverTarget {
                    provider: p.id.clone(),
                    model: m.id,
                    chain_index: idx,
                }));
            }
        }
        if &p.id == failing.0 {
            after = true;
        }
        let _ = failing.1;
    }
    // No candidate after the failing provider — try *before* it as
    // a last resort (skipping the failing provider again).
    for (idx2, p) in available.iter().enumerate() {
        let idx2 = idx2 + 1; // 1-based
        if &p.id == failing.0 {
            continue;
        }
        let pick = p
            .models
            .iter()
            .find(|m| matches!(m.tier, Some(crate::catalog::ModelTier::Flagship)))
            .or_else(|| p.models.first())
            .cloned();
        if let Some(m) = pick {
            return Ok(Some(FailoverTarget {
                provider: p.id.clone(),
                model: m.id,
                chain_index: idx2,
            }));
        }
    }
    let _ = integration; // reserved for future "skip if connection has cooldown"
    Ok(None)
}

/// Convenience: a "chain" iterator that yields failover targets in
/// order. Stops when `next_target` returns `None`.
pub struct Chain<'a> {
    catalog: &'a dyn CatalogService,
    integration: &'a dyn IntegrationService,
    next: Option<(ProviderId, ModelId)>,
    visited: std::collections::HashSet<ProviderId>,
}

impl<'a> Chain<'a> {
    pub fn new(
        catalog: &'a dyn CatalogService,
        integration: &'a dyn IntegrationService,
        start: (ProviderId, ModelId),
    ) -> Self {
        let mut visited = std::collections::HashSet::new();
        visited.insert(start.0.clone());
        Self {
            catalog,
            integration,
            next: Some(start),
            visited,
        }
    }

    /// Compute and return the *next* target after a rate-limit
    /// failure, or `None` if the chain is exhausted.
    pub async fn step(&mut self) -> Result<Option<FailoverTarget>, FailoverError> {
        let (p, m) = match self.next.take() {
            Some(t) => t,
            None => return Ok(None),
        };
        let target = next_target(self.catalog, self.integration, (&p, &m)).await?;
        if let Some(ref t) = target {
            if !self.visited.insert(t.provider.clone()) {
                // Already visited this provider; chain is exhausted.
                return Ok(None);
            }
            self.next = Some((t.provider.clone(), t.model.clone()));
        }
        Ok(target)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FailoverError {
    #[error("catalog error: {0}")]
    Catalog(#[from] crate::catalog::CatalogError),
    #[error("integration error: {0}")]
    Integration(#[from] crate::integration::IntegrationError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelInfo, ModelTier, ProviderInfo};
    use crate::credential::{Credential, CredentialService, CredentialType};
    use crate::integration::{AuthMethod, InMemoryIntegration, LoginProvider};
    use crate::store::in_memory::InMemoryCredentialStore;
    use std::sync::Arc;

    async fn populated() -> (
        InMemoryCatalog,
        InMemoryIntegration,
        Arc<dyn CredentialService>,
    ) {
        let cat = InMemoryCatalog::new();
        for p in &[
            ProviderInfo {
                id: "anthropic".into(),
                name: "Anthropic".into(),
                enabled: true,
                is_connected: true,
                has_integration: false,
                models: vec![ModelInfo {
                    id: "claude-sonnet-4-6".into(),
                    provider: "anthropic".into(),
                    name: "Claude Sonnet 4.6".into(),
                    cost_per_million_input: Some(3.0),
                    cost_per_million_output: Some(15.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Flagship),

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
                }],
                api_key: None,
                protocol: "anthropic-messages-2023-01-01".into(),
                path: "/v1/messages".into(),
                base_url: "https://api.anthropic.com".into(),
            },
            ProviderInfo {
                id: "openai".into(),
                name: "OpenAI".into(),
                enabled: true,
                is_connected: true,
                has_integration: false,
                models: vec![ModelInfo {
                    id: "gpt-5.1".into(),
                    provider: "openai".into(),
                    name: "GPT-5.1".into(),
                    cost_per_million_input: Some(2.5),
                    cost_per_million_output: Some(10.0),
                    context_window: 400_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Flagship),

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
                }],
                api_key: None,
                protocol: "anthropic-messages-2023-01-01".into(),
                path: "/v1/messages".into(),
                base_url: "https://api.anthropic.com".into(),
            },
            ProviderInfo {
                id: "gemini".into(),
                name: "Gemini".into(),
                enabled: true,
                is_connected: true,
                has_integration: false,
                models: vec![ModelInfo {
                    id: "gemini-2.5-pro".into(),
                    provider: "gemini".into(),
                    name: "Gemini 2.5 Pro".into(),
                    cost_per_million_input: Some(1.25),
                    cost_per_million_output: Some(10.0),
                    context_window: 1_000_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Flagship),

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
                }],
                api_key: None,
                protocol: "anthropic-messages-2023-01-01".into(),
                path: "/v1/messages".into(),
                base_url: "https://api.anthropic.com".into(),
            },
        ] {
            cat.register_provider(p.clone()).await.unwrap();
        }
        let integration = InMemoryIntegration::new();
        for id in ["anthropic", "openai", "gemini"] {
            integration
                .register(LoginProvider {
                    id: id.into(),
                    label: id.to_string(),
                    auth_methods: vec![AuthMethod::ApiKey {
                        env_var: format!("{}_KEY", id.to_uppercase()),
                    }],
                    env_keys: vec![format!("{}_KEY", id.to_uppercase())],
                    oauth_preferred: false,
                })
                .await
                .unwrap();
        }
        let creds: Arc<dyn CredentialService> = Arc::new(InMemoryCredentialStore::new());
        for id in ["anthropic", "openai", "gemini"] {
            creds
                .upsert(Credential::new(
                    id.into(),
                    "default",
                    CredentialType::ApiKey { key: "x".into() },
                ))
                .await
                .unwrap();
        }
        (cat, integration, creds)
    }

    #[tokio::test]
    async fn next_target_walks_to_next_provider() {
        // Failover is deterministic: providers are sorted by id before
        // the chain walk. The chain after anthropic is gemini, then
        // openai.
        let (cat, int, _creds) = populated().await;
        let target = next_target(
            &cat,
            &int,
            (&"anthropic".into(), &"claude-sonnet-4-6".into()),
        )
        .await
        .unwrap();
        assert!(target.is_some());
        let t = target.unwrap();
        assert_eq!(t.provider.as_str(), "gemini");
        assert_eq!(t.model.as_str(), "gemini-2.5-pro");
    }

    #[tokio::test]
    async fn next_target_walks_through_multiple() {
        // Sorted chain after anthropic: gemini, then openai, then
        // wrap-around returns anthropic (already visited) -> exhausted.
        let (cat, int, _creds) = populated().await;
        let mut chain = Chain::new(&cat, &int, ("anthropic".into(), "claude-sonnet-4-6".into()));
        let t1 = chain.step().await.unwrap().unwrap();
        assert_eq!(t1.provider.as_str(), "gemini");
        let t2 = chain.step().await.unwrap().unwrap();
        assert_eq!(t2.provider.as_str(), "openai");
        let t3 = chain.step().await.unwrap();
        assert!(t3.is_none(), "chain should be exhausted after openai");
    }

    #[tokio::test]
    async fn next_target_returns_none_when_alone() {
        let cat = InMemoryCatalog::new();
        cat.register_provider(ProviderInfo {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            enabled: true,
            is_connected: true,
            models: vec![],
            api_key: None,
            protocol: "anthropic-messages-2023-01-01".into(),
            path: "/v1/messages".into(),
            base_url: "https://api.anthropic.com".into(),
        })
        .await
        .unwrap();
        let int = InMemoryIntegration::new();
        let target = next_target(
            &cat,
            &int,
            (&"anthropic".into(), &"claude-sonnet-4-6".into()),
        )
        .await
        .unwrap();
        assert!(target.is_none());
    }
}
