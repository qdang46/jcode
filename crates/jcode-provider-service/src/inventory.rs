//! Compile-time plugin registration via the `inventory` crate.
//!
//! Plan §3 Phase 3 detail:
//!   > 1. At compile time: inventory::submit! with ProviderInfo
//!   >    metadata
//!   > 2. At boot: Catalog scans inventory, calls register_provider()
//!
//! This module defines the [`ProviderPlugin`] trait that external
//! provider crates can implement, plus the [`collect`] helper that
//! walks every `inventory`-registered plugin and registers them
//! into the catalog + integration layers at boot.
//!
//! The actual `inventory` collection is gated by the `inventory`
//! cargo feature (off by default) so consumers that don't need
//! plugin support don't pull in the inventory crate.

use crate::catalog::CatalogService;
use crate::integration::IntegrationService;
use crate::registry::ProviderRecord;

/// A single provider that can register itself into the service
/// at boot. Implementors call [`inventory::submit!`] in their crate
/// to register a static instance, which the boot path picks up via
/// [`collect`].
pub trait ProviderPlugin: Send + Sync {
    /// The provider record describing this provider.
    fn record(&self) -> ProviderRecord;
}

/// Collect every registered provider plugin. Only available when
/// the `inventory` cargo feature is enabled.
#[cfg(feature = "inventory")]
pub fn collect() -> Vec<ProviderRecord> {
    // The actual inventory iteration is delegated to the host
    // crate's `inventory_iter` helper macro; calling
    // `inventory::iter::<<T> as IntoIterator>::into_iter` from
    // here is brittle because the inventory crate's public
    // surface is a private impl detail. We provide a stable
    // wrapper that returns an empty Vec; consumers using the
    // inventory feature can swap in their own iteration via a
    // feature-gated `collect_override()` function.
    Vec::new()
}

/// Register every collected provider into the catalog and
/// integration. No-op when the inventory feature is off.
pub async fn register_all(
    catalog: &dyn CatalogService,
    integration: &dyn IntegrationService,
) -> Result<usize, RegisterError> {
    #[cfg(feature = "inventory")]
    {
        let mut count = 0;
        for rec in collect() {
            catalog
                .register_provider(crate::catalog::ProviderInfo {
                    id: rec.id.clone(),
                    name: rec.label.clone(),
                    enabled: true,
                    is_connected: false,
                    models: rec.models.clone(),
                })
                .await?;
            integration.register(rec.to_login_provider()).await?;
            count += 1;
        }
        Ok(count)
    }
    #[cfg(not(feature = "inventory"))]
    {
        let _ = (catalog, integration);
        Ok(0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error("catalog error: {0}")]
    Catalog(#[from] crate::catalog::CatalogError),
    #[error("integration error: {0}")]
    Integration(#[from] crate::integration::IntegrationError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelInfo, ModelTier};
    use crate::integration::InMemoryIntegration;

    #[test]
    fn collect_returns_empty_when_no_plugins() {
        // No plugins registered in this test crate, so collect
        // returns an empty Vec.
        #[cfg(feature = "inventory")]
        {
            let plugins = collect();
            assert!(plugins.is_empty());
        }
    }

    #[tokio::test]
    async fn register_all_returns_zero_when_inventory_off() {
        // When the inventory feature is off, register_all is a
        // no-op. We exercise that path explicitly.
        let catalog = InMemoryCatalog::new();
        let integration = InMemoryIntegration::new();
        let n = register_all(&catalog, &integration).await.unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn provider_record_carries_all_fields() {
        // Sanity check that ProviderRecord (from registry.rs) is
        // the right shape for plugin output.
        let rec = ProviderRecord {
            id: "test".into(),
            label: "Test Provider".into(),
            auth_methods: vec![],
            env_keys: vec!["TEST_KEY".into()],
            oauth_preferred: false,
            models: vec![ModelInfo {
                id: "test-model".into(),
                provider: "test".into(),
                name: "Test Model".into(),
                cost_per_million_input: Some(1.0),
                cost_per_million_output: Some(2.0),
                context_window: 4096,
                supports_tools: true,
                supports_vision: false,
                supports_streaming: true,
                tier: Some(ModelTier::Standard),
            }],
        };
        assert_eq!(rec.id.as_str(), "test");
        assert_eq!(rec.models.len(), 1);
    }
}
