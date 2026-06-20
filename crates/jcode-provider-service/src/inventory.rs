//! Compile-time plugin registration via the `inventory` crate.
//!
//! Plan §3 Phase 3 detail:
//!   > 1. At compile time: inventory::submit! with ProviderInfo
//!   >    metadata
//!   > 2. At boot: Catalog scans inventory, calls register_provider()
//!
//! This module provides a concrete [`PluginEntry`] type that
//! consumers wrap their plugin in and submit via
//! `inventory::submit!`. The boot path calls [`collect`] to walk
//! the registered entries and register them into the catalog and
//! integration layers.
//!
//! The `inventory` integration is gated by the `inventory` cargo
//! feature (off by default) so consumers that don't need plugin
//! support don't pull in the inventory crate.
//!
//! ## Usage
//!
//! In your provider crate:
//!
//! ```ignore
//! use jcode_provider_service::inventory::PluginEntry;
//! use jcode_provider_service::registry::ProviderRecord;
//!
//! fn my_record() -> ProviderRecord { ... }
//!
//! // At module scope:
//! inventory::submit!(PluginEntry::new("my-provider", my_record));
//! ```

use crate::catalog::CatalogService;
use crate::integration::IntegrationService;
use crate::registry::ProviderRecord;

/// A single, statically-allocated plugin entry. Wrap your
/// provider's `ProviderRecord` in this and submit it.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    id: String,
    record: ProviderRecord,
}

impl PluginEntry {
    /// Construct a new entry from a static id and a record.
    pub fn new(id: &'static str, record: ProviderRecord) -> Self {
        Self {
            id: id.to_string(),
            record,
        }
    }

    /// The plugin's stable id (used for diagnostics).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The provider record.
    pub fn record(&self) -> &ProviderRecord {
        &self.record
    }
}

// Register the inventory::collect! macro once, gated by the feature.
#[cfg(feature = "inventory")]
inventory::collect!(PluginEntry);

/// Collect every registered plugin entry. Only available when the
/// `inventory` cargo feature is enabled.
#[cfg(feature = "inventory")]
pub fn collect() -> Vec<ProviderRecord> {
    use ::inventory::iter;
    let mut out: Vec<ProviderRecord> = Vec::new();
    for entry in iter::<PluginEntry> {
        out.push(entry.record().clone());
    }
    out
}

/// Register every collected plugin into the catalog and
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

// Re-export the inventory::submit! macro for convenience.
#[cfg(feature = "inventory")]
pub use ::inventory::submit;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelInfo, ModelTier};
    use crate::integration::InMemoryIntegration;

    #[cfg(feature = "inventory")]
    #[test]
    fn collect_returns_empty_when_no_plugins() {
        // No PluginEntry instances are submitted in this test
        // crate, so collect returns an empty Vec.
        let plugins = collect();
        assert!(plugins.is_empty());
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

    #[test]
    fn plugin_entry_carries_id_and_record() {
        let rec = ProviderRecord {
            id: "test".into(),
            label: "Test".into(),
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

                release_date: None,
            }],
        };
        let entry = PluginEntry::new("test-plugin", rec);
        assert_eq!(entry.id(), "test-plugin");
        assert_eq!(entry.record().id.as_str(), "test");
    }
}
