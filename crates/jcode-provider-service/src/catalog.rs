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
    /// Optional release date for the model. When set, the opencode-style
    /// `small()` algorithm uses it to prefer newer models (with an 18-month
    /// freshness cap).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<chrono::NaiveDate>,

    /// Optional per-model base URL override. When set, this model
    /// uses a different endpoint than the provider default (e.g. a
    /// fine-tuned model on a custom endpoint).
    /// Matches opencode's `model.api.url` merge in `projectModel()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Optional per-model API path override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Optional per-model protocol override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Optional per-model body overrides (opencode request merge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_overrides: Option<serde_json::Value>,
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
    /// opencode-style small-model detection. Checks the model id and
    /// an optional display name for tokens like `nano`, `flash`, `lite`,
    /// `mini`, `haiku`, `small`, `fast`. Matches opencode's
    /// `SMALL_MODEL_RE = /\b(nano|flash|lite|mini|haiku|small|fast)\b/`.
    pub fn suggests_small(id: &str, name: Option<&str>) -> bool {
        let id_lower = id.to_ascii_lowercase();
        let combined = match name {
            Some(n) => format!("{} {}", id_lower, n.to_ascii_lowercase()),
            None => id_lower,
        };
        combined.contains("nano")
            || combined.contains("flash")
            || combined.contains("lite")
            || combined.contains("mini")
            || combined.contains("haiku")
            || combined.contains("small")
            || combined.contains("fast")
    }

    /// Old name; delegates to [`suggests_small`] with `name: None`.
    pub fn id_suggests_small(id: &str) -> bool {
        Self::suggests_small(id, None)
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
    /// Whether the provider has an integration entry (i.e. is registered in
    /// the Integration layer). Used by opencode's `available()` logic:
    /// if integration exists but NOT connected → not available.
    /// If no integration at all → available (can be set up later).
    #[serde(default)]
    pub has_integration: bool,
    /// List of models registered for this provider.
    pub models: Vec<ModelInfo>,
    /// Optional inline API key (opencode catalog.ts:96-101).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Base URL for API requests (e.g. "https://api.anthropic.com").
    /// Used by RouteResolver to build the Route without hardcoding.
    #[serde(default = "default_anthropic_url")]
    pub base_url: String,
    /// API path (e.g. "/v1/messages"). Used by RouteResolver.
    #[serde(default = "default_chat_path")]
    pub path: String,
    /// Protocol identifier (e.g. "anthropic-messages-2023-01-01").
    /// Used by RouteResolver to select the correct protocol adapter.
    #[serde(default = "default_chat_protocol")]
    pub protocol: String,
    /// Body-level defaults merged into Route by RouteResolver (opencode
    /// request merge pattern).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_defaults: Option<serde_json::Value>,
}

fn default_anthropic_url() -> String {
    "https://api.anthropic.com".into()
}
fn default_chat_path() -> String {
    "/v1/chat/completions".into()
}
fn default_chat_protocol() -> String {
    "openai-chat-2024".into()
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
    #[error("policy error: {0}")]
    Policy(String),
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

    /// Set the cached connection state for one provider. The new
    /// state is used by [available], [default], and [small] to
    /// decide which providers to surface.
    async fn set_connected(
        &self,
        provider: &ProviderId,
        connected: bool,
    ) -> Result<(), CatalogError>;

    /// Update the cached connection state for one provider based on
    /// the integration layer's live detection. Returns the new
    /// is_connected value.
    async fn refresh_connection(
        &self,
        provider: &ProviderId,
        integration: &dyn crate::integration::IntegrationService,
    ) -> Result<bool, CatalogError> {
        let connected = integration
            .detect(provider)
            .await
            .map(|s| s.is_connected())
            .unwrap_or(false);
        self.set_connected(provider, connected).await?;
        Ok(connected)
    }

    /// Smart variant of [`available`]: returns providers that are
    /// either statically connected OR have a live credential
    /// detected through the supplied integration layer. This matches
    /// opencode's `Catalog.available()` which consults the
    /// integration layer for the actual connection state.
    async fn live_available(
        &self,
        integration: &dyn crate::integration::IntegrationService,
    ) -> Result<Vec<ProviderInfo>, CatalogError> {
        let all = self.list_providers().await?;
        let mut out = Vec::new();
        for p in all {
            if !p.enabled {
                continue;
            }
            if p.is_connected {
                out.push(p);
                continue;
            }
            // Check the integration layer for live credentials.
            if let Ok(status) = integration.detect(&p.id).await
                && status.is_connected()
            {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// All models for a single provider.
    async fn models(&self, provider: &ProviderId) -> Result<Vec<ModelInfo>, CatalogError>;

    /// Find a model by `provider/model` string.
    async fn find_model(
        &self,
        provider: &ProviderId,
        model: &ModelId,
    ) -> Result<ModelInfo, CatalogError>;

    /// Persist a user-set default model choice. The next call to
    /// [`default`] returns this pair when its provider is available
    /// and the model is enabled.
    async fn set_default_model(
        &self,
        provider: &ProviderId,
        model: &ModelId,
    ) -> Result<(), CatalogError>;

    /// The user's default `(provider, model)`. Tries (in order):
    /// 1. User-set default (via [`set_default_model`]).
    /// 2. The first flagship model of the first available provider.
    /// 3. The first model of the first available provider.
    async fn default(&self) -> Result<(ProviderId, ModelId), CatalogError>;

    /// The cheapest "small" model available. When `provider_id` is `Some`,
    /// scoped to that provider (opencode-style). When `None`, searches all
    /// available providers (jcode default).
    /// Heuristic: model id contains
    /// `nano` / `flash` / `lite` / `mini` / `haiku`. If none match, returns
    /// the cheapest model with a non-zero cost.
    async fn small(
        &self,
        provider_id: Option<&ProviderId>,
    ) -> Result<(ProviderId, ModelId), CatalogError>;

    // -- policy integration (opencode-style finalize) --

    /// Remove every provider that the policy denies from the catalog.
    ///
    /// Mirrors opencode's `Catalog.finalize` which iterates the
    /// provider list and calls `policy.evaluate("provider.use", id,
    /// "allow")` to drop denied entries.  Call this after all
    /// boot-time registrations are done and whenever the policy is
    /// reloaded.
    ///
    /// The base implementation is a no-op; concrete implementations
    /// that hold a [`PolicyService`](crate::policy::PolicyService)
    /// reference should override it.
    async fn remove_denied_providers(&self) -> Result<(), CatalogError> {
        // No-op by default — implementations with a policy reference
        // override this.
        Ok(())
    }

    /// Attach a policy service.  The catalog will filter future
    /// [`available`] calls and [`remove_denied_providers`] through it.
    ///
    /// The base implementation is a no-op.
    fn set_policy(&self, _policy: Arc<dyn crate::policy::PolicyService>) {}

    /// Optional callback invoked after every catalog mutation
    /// (`register_provider`, `register_model`, `set_connected`).
    ///
    /// The callback is called *after* the mutation is committed so the
    /// subscriber sees the latest state.  The default implementation
    /// returns `None` (no callback).
    fn on_updated(&self) -> Option<Box<dyn Fn() + Send + Sync>> {
        None
    }
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
    policy: std::sync::RwLock<Option<Arc<dyn crate::policy::PolicyService>>>,
    on_updated: std::sync::RwLock<Option<Box<dyn Fn() + Send + Sync>>>,
    default_model: std::sync::RwLock<Option<(ProviderId, ModelId)>>,
}

impl InMemoryCatalog {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            policy: std::sync::RwLock::new(None),
            on_updated: std::sync::RwLock::new(None),
            default_model: std::sync::RwLock::new(None),
        }
    }

    /// Attach a policy service for filtering.  The trait method
    /// [`CatalogService::set_policy`] is sync, so the policy lock uses
    /// `std::sync::RwLock` rather than `tokio::sync::RwLock`.
    pub fn with_policy(self, policy: Arc<dyn crate::policy::PolicyService>) -> Self {
        *self.policy.write().unwrap() = Some(policy);
        self
    }

    /// Attach an "updated" callback, invoked after every catalog mutation.
    pub fn with_on_updated(self, cb: Box<dyn Fn() + Send + Sync>) -> Self {
        *self.on_updated.write().unwrap() = Some(cb);
        self
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
        drop(map);
        self.fire_on_updated();
        Ok(())
    }

    async fn set_connected(
        &self,
        provider: &ProviderId,
        connected: bool,
    ) -> Result<(), CatalogError> {
        let mut map = self.inner.write().await;
        if let Some(p) = map.get_mut(provider) {
            p.is_connected = connected;
        }
        drop(map);
        self.fire_on_updated();
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
        drop(map);
        self.fire_on_updated();
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
        // opencode catalog.ts:96-101: available = NOT disabled AND (
        //   has inline apiKey OR has connection OR integration exists
        // )
        // PLUS: policy deny list — providers that policy denies are
        // filtered out so they can never appear in the available list.
        let map = self.inner.read().await;
        let policy = self.policy.read().unwrap();
        Ok(map
            .values()
            .filter(|p| {
                if !p.enabled {
                    return false;
                }
                if let Some(ref pol) = *policy
                    && !pol.is_allowed(&p.id)
                {
                    return false; // policy denies this provider
                }
                if p.api_key.is_some() {
                    return true; // inline key → available (opencode)
                }
                if p.is_connected {
                    return true; // stored credential → available
                }
                // opencode: if integration exists but NOT connected → not available.
                // If no integration at all → available (can be set up later).
                !p.has_integration
            })
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

    async fn set_default_model(
        &self,
        provider: &ProviderId,
        model: &ModelId,
    ) -> Result<(), CatalogError> {
        *self.default_model.write().unwrap() = Some((provider.clone(), model.clone()));
        Ok(())
    }

    async fn default(&self) -> Result<(ProviderId, ModelId), CatalogError> {
        // opencode-style: try user-set default first (via set_default_model),
        // then a Flagship model, then fall back to the newest available model.
        let saved_default = { self.default_model.read().unwrap().clone() };
        if let Some((ref p, ref m)) = saved_default {
            // Verify the provider is still available and model exists+enabled.
            if let Ok(provider) = self.provider(p).await
                && provider.enabled
                && provider.model(m).is_some()
            {
                // Check that provider is in available set.
                if let Ok(available) = self.available().await
                    && available.iter().any(|a| &a.id == p)
                {
                    return Ok((p.clone(), m.clone()));
                }
            }
        }
        let providers = self.available().await?;
        if providers.is_empty() {
            return Err(CatalogError::NoAvailableProviders);
        }
        // Collect every (provider, model) with a release date so we can
        // pick the newest one. Models without a release date sort to
        // the bottom of the heap (treated as epoch 0).
        let mut all: Vec<(&ProviderId, &ModelInfo, i64)> = Vec::new();
        for p in &providers {
            for m in &p.models {
                let released = m
                    .release_date
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
                    .unwrap_or(0);
                all.push((&p.id, m, released));
            }
        }
        // Pass 1: Flagship, newest first.
        let mut flagship: Vec<_> = all
            .iter()
            .filter(|(_, m, _)| m.tier == Some(ModelTier::Flagship))
            .collect();
        flagship.sort_by_key(|(_, _, r)| -r);
        if let Some((p, m, _)) = flagship.first() {
            return Ok(((*p).clone(), m.id.clone()));
        }
        // Pass 2: newest available model overall.
        all.sort_by_key(|(_, _, r)| -r);
        if let Some((p, m, _)) = all.first() {
            return Ok(((*p).clone(), m.id.clone()));
        }
        // Final fallback: first model of first provider.
        let p = &providers[0];
        let m = p
            .models
            .first()
            .ok_or_else(|| CatalogError::NoModels(p.id.clone()))?;
        Ok((p.id.clone(), m.id.clone()))
    }

    async fn small(
        &self,
        provider_id: Option<&ProviderId>,
    ) -> Result<(ProviderId, ModelId), CatalogError> {
        // opencode-style: build a candidate list with cost + age, then
        // pick the lowest (cost*0.8 + age*0.2) score among models whose
        // id contains a "small" token, falling back to all candidates.
        let providers = self.available().await?;
        if providers.is_empty() {
            return Err(CatalogError::NoAvailableProviders);
        }
        // If scoped to a specific provider, only use that one.
        let providers: Vec<_> = match provider_id {
            Some(pid) => providers.into_iter().filter(|p| &p.id == pid).collect(),
            None => providers,
        };
        if providers.is_empty() {
            return Err(CatalogError::NoAvailableProviders);
        }
        let today = chrono::Utc::now().date_naive();
        let mut candidates: Vec<(ProviderId, ModelId, f64, f64, bool)> = Vec::new();
        for p in &providers {
            for m in &p.models {
                let Some(cost_in) = m.cost_per_million_input else {
                    continue;
                };
                let Some(cost_out) = m.cost_per_million_output else {
                    continue;
                };
                let cost = cost_in + cost_out;
                if cost <= 0.0 {
                    continue;
                }
                let age_months = m
                    .release_date
                    .map(|d| {
                        let days = (today - d).num_days();
                        (days.max(0) as f64) / 30.0
                    })
                    .unwrap_or(0.0);
                if age_months > 18.0 {
                    continue;
                }
                let is_small = ModelTier::suggests_small(m.id.as_str(), Some(&m.name));
                candidates.push((p.id.clone(), m.id.clone(), cost, age_months, is_small));
            }
        }
        if candidates.is_empty() {
            // No candidates with cost+age — fall back to id-based pick.
            for p in &providers {
                for m in &p.models {
                    if ModelTier::suggests_small(m.id.as_str(), Some(&m.name)) {
                        return Ok((p.id.clone(), m.id.clone()));
                    }
                }
            }
            // Final fallback: first model of first provider.
            let p = &providers[0];
            let m = p
                .models
                .first()
                .ok_or_else(|| CatalogError::NoModels(p.id.clone()))?;
            return Ok((p.id.clone(), m.id.clone()));
        }
        let max_cost = candidates
            .iter()
            .map(|c| c.2)
            .fold(0.0_f64, f64::max)
            .max(0.01);
        let max_age = candidates
            .iter()
            .map(|c| c.3)
            .fold(0.0_f64, f64::max)
            .max(0.01);
        let scored: Vec<_> = candidates
            .iter()
            .map(|c| {
                let score = (c.2 / max_cost) * 0.8 + (c.3 / max_age) * 0.2;
                (score, c.4, &c.0, &c.1)
            })
            .collect();
        // Prefer "small" candidates first, then lowest score.
        let mut best: Option<(f64, bool, ProviderId, ModelId)> = None;
        for (score, is_small, p, m) in &scored {
            let dominated = match &best {
                None => false,
                Some((s, sm, _, _)) => {
                    // If we have a small candidate already, non-small ones
                    // are dominated. Among same-small, prefer lower score.
                    if *sm && !*is_small {
                        true
                    } else if !*sm && *is_small {
                        false
                    } else {
                        *score >= *s
                    }
                }
            };
            if !dominated {
                best = Some((*score, *is_small, (*p).clone(), (*m).clone()));
            }
        }
        match best {
            Some((_, _, p, m)) => Ok((p, m)),
            None => {
                let p = &providers[0];
                let m = p
                    .models
                    .first()
                    .ok_or_else(|| CatalogError::NoModels(p.id.clone()))?;
                Ok((p.id.clone(), m.id.clone()))
            }
        }
    }

    async fn remove_denied_providers(&self) -> Result<(), CatalogError> {
        // opencode catalog.ts:189-197 finalize(): iterate the
        // provider list and drop any that the policy denies.
        // Clone the policy Arc out from under the std::sync::RwLock
        // so we never hold a non-Send guard across an await point.
        let policy: Option<Arc<dyn crate::policy::PolicyService>> = {
            match self.policy.read() {
                Ok(g) => g.clone(), // clones the Option<Arc<...>>
                Err(e) => return Err(CatalogError::Policy(e.to_string())),
            }
        };
        let policy = match policy {
            None => return Ok(()),
            Some(p) if !p.has_rules() => return Ok(()),
            Some(p) => p,
        };
        let denied: Vec<ProviderId> = self
            .inner
            .read()
            .await
            .keys()
            .filter(|id| !policy.is_allowed(id))
            .cloned()
            .collect();
        if denied.is_empty() {
            return Ok(());
        }
        let mut map = self.inner.write().await;
        for id in &denied {
            map.remove(id);
        }
        drop(map);
        self.fire_on_updated();
        Ok(())
    }

    fn set_policy(&self, policy: Arc<dyn crate::policy::PolicyService>) {
        *self.policy.write().unwrap() = Some(policy);
    }
}

impl InMemoryCatalog {
    fn fire_on_updated(&self) {
        if let Ok(g) = self.on_updated.read()
            && let Some(ref cb) = *g
        {
            cb();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anthropic() -> ProviderInfo {
        ProviderInfo {
            base_url: "https://api.anthropic.com".into(),
            path: "/v1/messages".into(),
            protocol: "anthropic-messages-2023-01-01".into(),
            api_key: None,
            id: "anthropic".into(),
            name: "Anthropic".into(),
            enabled: true,
            is_connected: true,
            has_integration: false,
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

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
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

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
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
        // enabled + not connected + no api_key → not available (no integration)
        p.enabled = false;
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
        let (p, m) = cat.small(None).await.unwrap();
        assert_eq!(p.as_str(), "anthropic");
        assert!(
            ModelTier::id_suggests_small(m.as_str()) || ModelTier::suggests_small(m.as_str(), None)
        );
    }

    #[tokio::test]
    async fn small_falls_back_to_cheapest() {
        let cat = InMemoryCatalog::new();
        let mut p = anthropic();
        // Strip the haiku model so id_heuristic finds nothing.
        p.models.retain(|m| m.id.as_str() != "claude-haiku-4-5");
        cat.register_provider(p).await.unwrap();
        let (_, m) = cat.small(None).await.unwrap();
        // Only Opus left, must be selected.
        assert_eq!(m.as_str(), "claude-opus-4-8");
    }

    #[tokio::test]
    async fn default_errors_when_no_providers() {
        let cat = InMemoryCatalog::new();
        let err = cat.default().await.unwrap_err();
        assert!(matches!(err, CatalogError::NoAvailableProviders));
    }

    #[tokio::test]
    async fn small_prefers_newer_within_age_cap() {
        // opencode-style: among two "small" candidates, prefer the newer
        // one (with an 18-month cap).
        let cat = InMemoryCatalog::new();
        let p = ProviderInfo {
            base_url: "https://api.anthropic.com".into(),
            path: "/v1/messages".into(),
            protocol: "anthropic-messages-2023-01-01".into(),
            api_key: None,
            id: "anthropic".into(),
            name: "Anthropic".into(),
            enabled: true,
            is_connected: true,
            has_integration: false,
            models: vec![
                ModelInfo {
                    id: "claude-haiku-old".into(),
                    provider: "anthropic".into(),
                    name: "Claude Haiku Old".into(),
                    cost_per_million_input: Some(0.8),
                    cost_per_million_output: Some(4.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Nano),
                    release_date: chrono::NaiveDate::from_ymd_opt(2020, 1, 1),
                    base_url: None,
                    path: None,
                    protocol: None,
                },
                ModelInfo {
                    id: "claude-haiku-new".into(),
                    provider: "anthropic".into(),
                    name: "Claude Haiku New".into(),
                    cost_per_million_input: Some(0.8),
                    cost_per_million_output: Some(4.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Nano),
                    release_date: chrono::NaiveDate::from_ymd_opt(2025, 1, 1),
                    base_url: None,
                    path: None,
                    protocol: None,
                },
            ],
        };
        cat.register_provider(p).await.unwrap();
        let (provider, model) = cat.small(None).await.unwrap();
        assert_eq!(provider.as_str(), "anthropic");
        assert_eq!(model.as_str(), "claude-haiku-new");
    }

    #[tokio::test]
    async fn small_skips_models_older_than_age_cap() {
        // Anything older than 18 months should be dropped from candidates.
        let cat = InMemoryCatalog::new();
        let p = ProviderInfo {
            base_url: "https://api.anthropic.com".into(),
            path: "/v1/messages".into(),
            protocol: "anthropic-messages-2023-01-01".into(),
            api_key: None,
            id: "anthropic".into(),
            name: "Anthropic".into(),
            enabled: true,
            is_connected: true,
            has_integration: false,
            models: vec![
                ModelInfo {
                    id: "claude-haiku-ancient".into(),
                    provider: "anthropic".into(),
                    name: "Claude Haiku Ancient".into(),
                    cost_per_million_input: Some(0.8),
                    cost_per_million_output: Some(4.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Nano),
                    release_date: chrono::NaiveDate::from_ymd_opt(2020, 1, 1),
                    base_url: None,
                    path: None,
                    protocol: None,
                },
                ModelInfo {
                    id: "claude-haiku-fresh".into(),
                    provider: "anthropic".into(),
                    name: "Claude Haiku Fresh".into(),
                    cost_per_million_input: Some(0.8),
                    cost_per_million_output: Some(4.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Nano),
                    release_date: chrono::NaiveDate::from_ymd_opt(2025, 6, 1),
                    base_url: None,
                    path: None,
                    protocol: None,
                },
            ],
        };
        cat.register_provider(p).await.unwrap();
        let (_, model) = cat.small(None).await.unwrap();
        assert_eq!(model.as_str(), "claude-haiku-fresh");
    }

    #[tokio::test]
    async fn small_chooses_cheapest_when_no_small_token() {
        // No "small" id tokens, but cost+age should still pick the cheapest.
        let cat = InMemoryCatalog::new();
        let p = ProviderInfo {
            base_url: "https://api.anthropic.com".into(),
            path: "/v1/messages".into(),
            protocol: "anthropic-messages-2023-01-01".into(),
            api_key: None,
            id: "openai".into(),
            name: "OpenAI".into(),
            enabled: true,
            is_connected: true,
            has_integration: false,
            models: vec![
                ModelInfo {
                    id: "gpt-flagship".into(),
                    provider: "openai".into(),
                    name: "Flagship".into(),
                    cost_per_million_input: Some(15.0),
                    cost_per_million_output: Some(75.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Flagship),
                    release_date: chrono::NaiveDate::from_ymd_opt(2025, 1, 1),
                    base_url: None,
                    path: None,
                    protocol: None,
                },
                ModelInfo {
                    id: "gpt-standard".into(),
                    provider: "openai".into(),
                    name: "Standard".into(),
                    cost_per_million_input: Some(0.5),
                    cost_per_million_output: Some(2.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Standard),
                    release_date: chrono::NaiveDate::from_ymd_opt(2025, 1, 1),
                    base_url: None,
                    path: None,
                    protocol: None,
                },
            ],
        };
        cat.register_provider(p).await.unwrap();
        let (_, model) = cat.small(None).await.unwrap();
        assert_eq!(model.as_str(), "gpt-standard");
    }

    #[test]
    fn model_tier_id_heuristic() {
        // opencode SMALL_MODEL_RE = /\b(nano|flash|lite|mini|haiku|small|fast)\b/
        assert!(ModelTier::suggests_small("claude-haiku-4-5", None));
        assert!(ModelTier::suggests_small("gpt-5-mini", None));
        assert!(ModelTier::suggests_small("gemini-2.5-flash", None));
        assert!(ModelTier::suggests_small("claude-haiku-fast", None));
        assert!(ModelTier::suggests_small("xyz", Some("nano model")));
        assert!(ModelTier::suggests_small(
            "some-model",
            Some("Fast inference")
        ));
        assert!(!ModelTier::suggests_small("claude-opus-4-8", None));
    }
}
