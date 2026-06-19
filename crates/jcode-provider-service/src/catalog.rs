//! Catalog: provider/model registry and dynamic resolution.
//!
//! Phase 3 of the master plan.
//!
//! The Catalog layer is the *single source of truth* for "what providers
//! and models are available". It is dynamic — providers self-register at
//! boot time via a `Plugin` hook — and it consults the Integration layer
//! to decide which entries are *available* (have credentials).
//!
//! Three derived views the runtime uses:
//!
//! - [`CatalogService::available`] — providers with credentials.
//! - [`CatalogService::default`] — the user's preferred model (from config)
//!   or the best available model.
//! - [`CatalogService::small`] — the cheapest available model matching
//!   `/nano|flash|lite/i` (matches opencode's heuristic).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::types::{ModelId, ProviderId};

/// Static, in-catalog metadata for a model. Does *not* include connection
/// state — that's a query against the Integration layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: ModelId,
    pub provider: ProviderId,
    /// User-visible display name.
    pub name: String,
    /// USD per million input tokens. `None` for free / unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_million_input: Option<f64>,
    /// USD per million output tokens. `None` for free / unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_million_output: Option<f64>,
    pub context_window: u32,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_streaming: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
}

impl ModelInfo {
    /// `true` if the model has nonzero costs on either side.
    pub fn has_cost(&self) -> bool {
        self.cost_per_million_input.unwrap_or(0.0) > 0.0
            || self.cost_per_million_output.unwrap_or(0.0) > 0.0
    }
}

/// Coarse "size" tier used by `CatalogService::small` heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Flagship,
    Standard,
    Mini,
    Nano,
}

impl ModelTier {
    /// Heuristic: does the model id look like a "small" model?
    pub fn id_suggests_small(id: &str) -> bool {
        let lower = id.to_ascii_lowercase();
        lower.contains("nano")
            || lower.contains("flash")
            || lower.contains("lite")
            || lower.contains("mini")
            || lower.contains("haiku")
    }
}

/// Provider metadata in the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: ProviderId,
    pub name: String,
    /// Whether the provider is enabled in config (`config.toml
    /// [provider] disabled = [...]`).
    pub enabled: bool,
    /// Whether the provider has a usable credential. Recomputed every time
    /// the catalog is queried.
    pub is_connected: bool,
    /// List of models registered for this provider.
    pub models: Vec<ModelInfo>,
}

impl ProviderInfo {
    pub fn model(&self, id: &ModelId) -> Option<&ModelInfo> {
        self.models.iter().find(|m| &m.id == id)
    }
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("unknown provider: {0}")]
    UnknownProvider(ProviderId),
    #[error("unknown model: {0}")]
    UnknownModel(ModelId),
    #[error("provider {0} has no models")]
    NoModels(ProviderId),
    #[error("no available providers (none have credentials)")]
    NoAvailableProviders,
}

/// The catalog service. The runtime asks the catalog for *lists* of
/// providers and models; it does *not* ask it for credentials (that's the
/// Integration layer's job).
#[async_trait]
pub trait CatalogService: Send + Sync {
    /// Register a provider entry. Provider implementations call this at
    /// boot (Phase 0) and the `catalog.transform` plugin hook fills in
    /// models afterwards.
    async fn register_provider(&self, info: ProviderInfo) -> Result<(), CatalogError>;

    /// Register a single model for an existing provider.
    async fn register_model(
        &self,
        provider: &ProviderId,
        model: ModelInfo,
    ) -> Result<(), CatalogError>;

    /// Look up a provider entry.
    async fn provider(&self, id: &ProviderId) -> Result<ProviderInfo, CatalogError>;

    /// All registered providers, including disabled ones.
    async fn list_providers(&self) -> Result<Vec<ProviderInfo>, CatalogError>;

    /// All providers that are enabled *and* have credentials. The list the
    /// `jcode provider list` CLI shows by default.
    async fn available(&self) -> Result<Vec<ProviderInfo>, CatalogError>;

    /// All models for a single provider.
    async fn models(&self, provider: &ProviderId) -> Result<Vec<ModelInfo>, CatalogError>;

    /// Find a model by `provider/model` string.
    async fn find_model(
        &self,
        provider: &ProviderId,
        model: &ModelId,
    ) -> Result<ModelInfo, CatalogError>;

    /// The user's default `(provider, model)`. Tries (in order):
    /// 1. `config.toml [provider] default = "<p>/<m>"`.
    /// 2. The first flagship model of the first available provider.
    /// 3. The first model of the first available provider.
    async fn default(&self) -> Result<(ProviderId, ModelId), CatalogError>;

    /// The cheapest "small" model available. Heuristic: model id contains
    /// `nano` / `flash` / `lite` / `mini` / `haiku`. If none match, returns
    /// the cheapest model with a non-zero cost.
    async fn small(&self) -> Result<(ProviderId, ModelId), CatalogError>;
}

// ---------------------------------------------------------------------------
// In-memory reference implementation
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Simple in-memory catalog. Used for tests and the Phase 0 boot path.
pub struct InMemoryCatalog {
    inner: Arc<RwLock<HashMap<ProviderId, ProviderInfo>>>,
}

impl InMemoryCatalog {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CatalogService for InMemoryCatalog {
    async fn register_provider(&self, info: ProviderInfo) -> Result<(), CatalogError> {
        let mut map = self.inner.write().await;
        map.insert(info.id.clone(), info);
        Ok(())
    }

    async fn register_model(
        &self,
        provider: &ProviderId,
        model: ModelInfo,
    ) -> Result<(), CatalogError> {
        let mut map = self.inner.write().await;
        let entry = map
            .get_mut(provider)
            .ok_or_else(|| CatalogError::UnknownProvider(provider.clone()))?;
        entry.models.push(model);
        Ok(())
    }

    async fn provider(&self, id: &ProviderId) -> Result<ProviderInfo, CatalogError> {
        let map = self.inner.read().await;
        map.get(id)
            .cloned()
            .ok_or_else(|| CatalogError::UnknownProvider(id.clone()))
    }

    async fn list_providers(&self) -> Result<Vec<ProviderInfo>, CatalogError> {
        let map = self.inner.read().await;
        Ok(map.values().cloned().collect())
    }

    async fn available(&self) -> Result<Vec<ProviderInfo>, CatalogError> {
        let map = self.inner.read().await;
        Ok(map
            .values()
            .filter(|p| p.enabled && p.is_connected)
            .cloned()
            .collect())
    }

    async fn models(&self, provider: &ProviderId) -> Result<Vec<ModelInfo>, CatalogError> {
        let map = self.inner.read().await;
        map.get(provider)
            .map(|p| p.models.clone())
            .ok_or_else(|| CatalogError::UnknownProvider(provider.clone()))
    }

    async fn find_model(
        &self,
        provider: &ProviderId,
        model: &ModelId,
    ) -> Result<ModelInfo, CatalogError> {
        let map = self.inner.read().await;
        let p = map
            .get(provider)
            .ok_or_else(|| CatalogError::UnknownProvider(provider.clone()))?;
        p.model(model)
            .cloned()
            .ok_or_else(|| CatalogError::UnknownModel(model.clone()))
    }

    async fn default(&self) -> Result<(ProviderId, ModelId), CatalogError> {
        let providers = self.available().await?;
        if providers.is_empty() {
            return Err(CatalogError::NoAvailableProviders);
        }
        for p in &providers {
            if let Some(m) = p.models.iter().find(|m| m.tier == Some(ModelTier::Flagship)) {
                return Ok((p.id.clone(), m.id.clone()));
            }
        }
        let p = &providers[0];
        let m = p
            .models
            .first()
            .ok_or_else(|| CatalogError::NoModels(p.id.clone()))?;
        Ok((p.id.clone(), m.id.clone()))
    }

    async fn small(&self) -> Result<(ProviderId, ModelId), CatalogError> {
        let providers = self.available().await?;
        if providers.is_empty() {
            return Err(CatalogError::NoAvailableProviders);
        }
        // Pass 1: id-based heuristic.
        for p in &providers {
            for m in &p.models {
                if ModelTier::id_suggests_small(m.id.as_str()) {
                    return Ok((p.id.clone(), m.id.clone()));
                }
            }
        }
        // Pass 2: cheapest model with a cost.
        let mut best: Option<(f64, ProviderId, ModelId)> = None;
        for p in &providers {
            for m in &p.models {
                if let Some(cost) = m.cost_per_million_output {
                    let dominated = best.as_ref().map(|(c, _, _)| cost >= *c).unwrap_or(false);
                    if !dominated {
                        best = Some((cost, p.id.clone(), m.id.clone()));
                    }
                }
            }
        }
        match best {
            Some((_, p, m)) => Ok((p, m)),
            None => {
                // Fallback: just the first model of the first provider.
                let p = &providers[0];
                let m = p
                    .models
                    .first()
                    .ok_or_else(|| CatalogError::NoModels(p.id.clone()))?;
                Ok((p.id.clone(), m.id.clone()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anthropic() -> ProviderInfo {
        ProviderInfo {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            enabled: true,
            is_connected: true,
            models: vec![
                ModelInfo {
                    id: "claude-opus-4-8".into(),
                    provider: "anthropic".into(),
                    name: "Claude Opus 4.8".into(),
                    cost_per_million_input: Some(15.0),
                    cost_per_million_output: Some(75.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Flagship),
                },
                ModelInfo {
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
                },
            ],
        }
    }

    #[tokio::test]
    async fn register_and_lookup() {
        let cat = InMemoryCatalog::new();
        cat.register_provider(anthropic()).await.unwrap();
        let got = cat.provider(&"anthropic".into()).await.unwrap();
        assert_eq!(got.models.len(), 2);
    }

    #[tokio::test]
    async fn available_filters_disconnected() {
        let cat = InMemoryCatalog::new();
        let mut p = anthropic();
        p.is_connected = false;
        cat.register_provider(p).await.unwrap();
        let avail = cat.available().await.unwrap();
        assert!(avail.is_empty());
    }

    #[tokio::test]
    async fn default_picks_flagship() {
        let cat = InMemoryCatalog::new();
        cat.register_provider(anthropic()).await.unwrap();
        let (p, m) = cat.default().await.unwrap();
        assert_eq!(p.as_str(), "anthropic");
        assert_eq!(m.as_str(), "claude-opus-4-8");
    }

    #[tokio::test]
    async fn small_uses_id_heuristic() {
        let cat = InMemoryCatalog::new();
        cat.register_provider(anthropic()).await.unwrap();
        let (p, m) = cat.small().await.unwrap();
        assert_eq!(p.as_str(), "anthropic");
        assert!(ModelTier::id_suggests_small(m.as_str()));
    }

    #[tokio::test]
    async fn small_falls_back_to_cheapest() {
        let cat = InMemoryCatalog::new();
        let mut p = anthropic();
        // Strip the haiku model so id_heuristic finds nothing.
        p.models.retain(|m| m.id.as_str() != "claude-haiku-4-5");
        cat.register_provider(p).await.unwrap();
        let (_, m) = cat.small().await.unwrap();
        // Only Opus left, must be selected.
        assert_eq!(m.as_str(), "claude-opus-4-8");
    }

    #[tokio::test]
    async fn default_errors_when_no_providers() {
        let cat = InMemoryCatalog::new();
        let err = cat.default().await.unwrap_err();
        assert!(matches!(err, CatalogError::NoAvailableProviders));
    }

    #[test]
    fn model_tier_id_heuristic() {
        assert!(ModelTier::id_suggests_small("claude-haiku-4-5"));
        assert!(ModelTier::id_suggests_small("gpt-5-mini"));
        assert!(ModelTier::id_suggests_small("gemini-2.5-flash"));
        assert!(!ModelTier::id_suggests_small("claude-opus-4-8"));
    }
}
