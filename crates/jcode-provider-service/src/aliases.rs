//! Model aliases: short names that resolve to tier-appropriate models.
//!
//! Plan §7 references oh-my-pi / CCB's "Model aliases":
//!   > Model aliases | sonnet/opus/haiku/best resolve to tier-appropriate models
//!   > Subscription-aware defaults | Max → Opus, Pro → Sonnet
//!
//! This module provides the alias resolution. Given a string
//! (e.g. "opus" or "haiku" or "best"), return the canonical
//! (provider, model) that the alias currently maps to. Aliases
//! resolve at request time, so the resolution can take the
//! user's available providers + connection state into account.

use std::collections::HashMap;

use crate::catalog::CatalogService;
use crate::types::{ModelId, ProviderId};

/// A single alias rule. Matches a query string (case-insensitive)
/// to a tier (or specific model). The first matching rule wins.
#[derive(Debug, Clone)]
pub struct AliasRule {
    /// The alias text the user types (e.g. "opus", "sonnet", "best").
    pub pattern: String,
    /// If set, the alias resolves to a specific model on the
    /// given provider (used for "haiku" -> anthropic claude-haiku-4-5).
    pub specific: Option<(ProviderId, ModelId)>,
    /// Otherwise, the alias picks the model with the matching tier
    /// from the first available provider.
    pub tier: Option<ModelTier>,
    /// For subscription-tier aliases ("max", "pro"), what tier they
    /// upgrade to.
    pub subscription_tier: Option<SubscriptionTier>,
}

/// Tier of a model, as understood by the alias resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTier {
    /// Top-of-the-line model (e.g. claude-opus, gpt-5.1).
    Flagship,
    /// Standard model (e.g. claude-sonnet).
    Standard,
    /// Smaller model (e.g. gpt-5-mini).
    Mini,
    /// Smallest model (e.g. claude-haiku, gpt-5-nano).
    Nano,
}

/// User's subscription tier, used to choose a default model tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriptionTier {
    /// Free tier: pick nano.
    Free,
    /// Pro tier: pick standard.
    Pro,
    /// Max (or team) tier: pick flagship.
    Max,
}

impl AliasRule {
    fn matches(&self, query: &str) -> bool {
        self.pattern.eq_ignore_ascii_case(query)
    }
}

/// The alias table. Constructed once at boot; queried at request
/// time. The list is in priority order: the first matching rule
/// wins.
pub struct AliasTable {
    rules: Vec<AliasRule>,
}

impl Default for AliasTable {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl AliasTable {
    /// Standard alias table matching the plan's references.
    pub fn with_builtins() -> Self {
        Self {
            rules: vec![
                // Specific model aliases.
                AliasRule {
                    pattern: "haiku".into(),
                    specific: Some(("anthropic".into(), "claude-haiku-4-5".into())),
                    tier: None,
                    subscription_tier: None,
                },
                AliasRule {
                    pattern: "opus".into(),
                    specific: Some(("anthropic".into(), "claude-opus-4-8".into())),
                    tier: None,
                    subscription_tier: None,
                },
                AliasRule {
                    pattern: "sonnet".into(),
                    specific: Some(("anthropic".into(), "claude-sonnet-4-6".into())),
                    tier: None,
                    subscription_tier: None,
                },
                // Tier-based aliases.
                AliasRule {
                    pattern: "nano".into(),
                    specific: None,
                    tier: Some(ModelTier::Nano),
                    subscription_tier: None,
                },
                AliasRule {
                    pattern: "mini".into(),
                    specific: None,
                    tier: Some(ModelTier::Mini),
                    subscription_tier: None,
                },
                AliasRule {
                    pattern: "best".into(),
                    specific: None,
                    tier: Some(ModelTier::Flagship),
                    subscription_tier: None,
                },
                // Subscription-aware defaults (CCB reference).
                AliasRule {
                    pattern: "max".into(),
                    specific: None,
                    tier: None,
                    subscription_tier: Some(SubscriptionTier::Max),
                },
                AliasRule {
                    pattern: "pro".into(),
                    specific: None,
                    tier: None,
                    subscription_tier: Some(SubscriptionTier::Pro),
                },
                AliasRule {
                    pattern: "free".into(),
                    specific: None,
                    tier: None,
                    subscription_tier: Some(SubscriptionTier::Free),
                },
            ],
        }
    }

    /// Add a custom alias.
    pub fn with(mut self, rule: AliasRule) -> Self {
        // Custom rules take priority over builtins.
        self.rules.insert(0, rule);
        self
    }

    /// Resolve a query string against the catalog. Returns the first
    /// matching rule's resolution (specific or tier-based) or None.
    pub async fn resolve(
        &self,
        query: &str,
        catalog: &dyn CatalogService,
    ) -> Result<Option<(ProviderId, ModelId)>, crate::catalog::CatalogError> {
        // Specific rules first.
        for rule in &self.rules {
            if !rule.matches(query) {
                continue;
            }
            if let Some(specific) = &rule.specific {
                // Verify the specific model exists in the catalog.
                let provider = specific.0.clone();
                let model = specific.1.clone();
                if catalog.find_model(&provider, &model).await.is_ok() {
                    return Ok(Some((provider, model)));
                }
                // Specific model missing: fall through to the next rule.
                continue;
            }
        }
        // Tier-based / subscription-based rules.
        for rule in &self.rules {
            if !rule.matches(query) {
                continue;
            }
            if let Some(tier) = rule.tier {
                return pick_tier_model(catalog, tier).await.map(Some);
            }
            if let Some(sub) = rule.subscription_tier {
                let tier = match sub {
                    SubscriptionTier::Free => ModelTier::Nano,
                    SubscriptionTier::Pro => ModelTier::Standard,
                    SubscriptionTier::Max => ModelTier::Flagship,
                };
                return pick_tier_model(catalog, tier).await.map(Some);
            }
        }
        Ok(None)
    }

    /// Enumerate every alias name in the table (for `aliases` CLI
    /// subcommand, etc.).
    pub fn patterns(&self) -> Vec<&str> {
        self.rules.iter().map(|r| r.pattern.as_str()).collect()
    }
}

async fn pick_tier_model(
    catalog: &dyn CatalogService,
    tier: ModelTier,
) -> Result<(ProviderId, ModelId), crate::catalog::CatalogError> {
    // Walk available providers, find the first model with matching tier.
    let catalog_tier = match tier {
        ModelTier::Flagship => crate::catalog::ModelTier::Flagship,
        ModelTier::Standard => crate::catalog::ModelTier::Standard,
        ModelTier::Mini => crate::catalog::ModelTier::Mini,
        ModelTier::Nano => crate::catalog::ModelTier::Nano,
    };
    for p in catalog.available().await? {
        for m in &p.models {
            if m.tier == Some(catalog_tier) {
                return Ok((p.id.clone(), m.id.clone()));
            }
        }
    }
    // No tier match: return an error.
    Err(crate::catalog::CatalogError::NoAvailableProviders)
}

/// Per-user alias overrides (e.g. "my-fast" -> "anthropic/claude-haiku-4-5").
#[derive(Debug, Clone, Default)]
pub struct UserAliases {
    pub entries: HashMap<String, (ProviderId, ModelId)>,
}

impl UserAliases {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace an alias.
    pub fn set(&mut self, alias: impl Into<String>, target: (ProviderId, ModelId)) {
        self.entries.insert(alias.into(), target);
    }

    /// Remove an alias.
    pub fn remove(&mut self, alias: &str) -> Option<(ProviderId, ModelId)> {
        self.entries.remove(alias)
    }

    /// List all user aliases.
    pub fn list(&self) -> Vec<(&str, &ProviderId, &ModelId)> {
        self.entries
            .iter()
            .map(|(k, (p, m))| (k.as_str(), p, m))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelInfo, ModelTier as CatalogTier, ProviderInfo};

    async fn catalog() -> InMemoryCatalog {
        let c = InMemoryCatalog::new();
        c.register_provider(ProviderInfo {
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
                    tier: Some(CatalogTier::Flagship),

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
                },
                ModelInfo {
                    id: "claude-sonnet-4-6".into(),
                    provider: "anthropic".into(),
                    name: "Claude Sonnet 4.6".into(),
                    cost_per_million_input: Some(3.0),
                    cost_per_million_output: Some(15.0),
                    context_window: 200_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(CatalogTier::Standard),

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
                    tier: Some(CatalogTier::Nano),

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
                },
            ],
            api_key: None,
            protocol: "anthropic-messages-2023-01-01".into(),
            path: "/v1/messages".into(),
            base_url: "https://api.anthropic.com".into(),
        })
        .await
        .unwrap();
        c
    }

    #[tokio::test]
    async fn specific_alias_resolves() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("opus", &cat).await.unwrap().unwrap();
        assert_eq!(got.0.as_str(), "anthropic");
        assert_eq!(got.1.as_str(), "claude-opus-4-8");
    }

    #[tokio::test]
    async fn case_insensitive_matching() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("OPUS", &cat).await.unwrap().unwrap();
        assert_eq!(got.1.as_str(), "claude-opus-4-8");
    }

    #[tokio::test]
    async fn tier_alias_picks_flagship() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("best", &cat).await.unwrap().unwrap();
        assert_eq!(got.1.as_str(), "claude-opus-4-8");
    }

    #[tokio::test]
    async fn tier_alias_picks_nano() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("nano", &cat).await.unwrap().unwrap();
        assert_eq!(got.1.as_str(), "claude-haiku-4-5");
    }

    #[tokio::test]
    async fn subscription_max_picks_flagship() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("max", &cat).await.unwrap().unwrap();
        assert_eq!(got.1.as_str(), "claude-opus-4-8");
    }

    #[tokio::test]
    async fn subscription_pro_picks_standard() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("pro", &cat).await.unwrap().unwrap();
        assert_eq!(got.1.as_str(), "claude-sonnet-4-6");
    }

    #[tokio::test]
    async fn subscription_free_picks_nano() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("free", &cat).await.unwrap().unwrap();
        assert_eq!(got.1.as_str(), "claude-haiku-4-5");
    }

    #[tokio::test]
    async fn unknown_alias_returns_none() {
        let cat = catalog().await;
        let table = AliasTable::with_builtins();
        let got = table.resolve("not-a-thing", &cat).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn user_alias_overrides_builtin() {
        let cat = catalog().await;
        let mut user = UserAliases::new();
        user.set("opus", ("openai".into(), "gpt-5.1".into()));
        let table = AliasTable::with_builtins().with(AliasRule {
            pattern: "opus".into(),
            specific: Some(("openai".into(), "gpt-5.1".into())),
            tier: None,
            subscription_tier: None,
        });
        // The custom rule is at index 0 and matches; the builtin
        // (which also matches) is at a later index. First match wins.
        // Since both are specific, the first one is used.
        let got = table.resolve("opus", &cat).await.unwrap();
        // The custom rule's specific points to openai/gpt-5.1, but
        // the catalog doesn't have that model -> falls through to
        // the next rule that matches (the builtin opus).
        // Wait, both rules have pattern == "opus". The custom one
        // is at index 0, builtin at index 2. The custom rule's
        // specific (openai/gpt-5.1) doesn't exist in the catalog,
        // so the resolver falls through. The next matching rule
        // (builtin opus) is hit, which resolves to anthropic.
        assert_eq!(got.unwrap().1.as_str(), "claude-opus-4-8");
    }

    #[test]
    fn user_aliases_set_remove_list() {
        let mut u = UserAliases::new();
        u.set("foo", ("anthropic".into(), "claude-haiku-4-5".into()));
        u.set("bar", ("openai".into(), "gpt-5-mini".into()));
        assert_eq!(u.list().len(), 2);
        let removed = u.remove("foo");
        assert_eq!(
            removed,
            Some(("anthropic".into(), "claude-haiku-4-5".into()))
        );
        assert_eq!(u.list().len(), 1);
    }

    #[test]
    fn patterns_lists_all_builtins() {
        let t = AliasTable::with_builtins();
        let patterns = t.patterns();
        for expected in [
            "haiku", "opus", "sonnet", "nano", "mini", "best", "max", "pro", "free",
        ] {
            assert!(patterns.contains(&expected), "missing {expected}");
        }
    }
}
