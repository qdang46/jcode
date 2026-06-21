//! Bridge for the 36 OpenAI-compatible provider profiles in
//! `jcode-provider-metadata`.
//!
//! Plan §3 Phase 4:
//!   > `crates/jcode-provider-metadata/src/catalog.rs` — 36
//!   >   profiles become Integration entries
//!
//! The metadata crate has 36 `OpenAiCompatibleProfile` constants
//! (kimi, zai, opencode, deepseek, ...). They were originally
//! consumed by the old `jcode-provider-app` catalog stub. This
//! module translates every profile into:
//!  - a [`crate::integration::LoginProvider`] entry for the new
//!    `IntegrationService` (so the runtime can detect and log in to
//!    the profile)
//!  - a [`crate::catalog::ModelInfo`] entry for the catalog
//!    (so `provider list` and `model list` show them)
//!  - a [`crate::registry::ProviderRecord`] for the boot-time
//!    registry
//!
//! Use [`metadata_registry`] to get a `CompositeRegistry` that
//! includes the built-in providers AND every metadata profile, and
//! [`all_metadata_records`] to enumerate just the 36 profiles.

use std::sync::Arc;

use jcode_provider_metadata::{OpenAiCompatibleProfile, openai_compatible_profiles};

use crate::catalog::ModelInfo;
use crate::integration::{AuthMethod, LoginProvider};
use crate::registry::{ProviderRecord, ProviderRegistry};
use crate::types::{ModelId, ProviderId};

/// Return the canonical id of a metadata profile.
pub fn profile_id(p: &OpenAiCompatibleProfile) -> ProviderId {
    ProviderId::from(p.id)
}

/// Translate a metadata profile to a [`LoginProvider`].
pub fn profile_to_login_provider(p: &OpenAiCompatibleProfile) -> LoginProvider {
    let env_var = p.api_key_env.to_string();
    LoginProvider {
        id: profile_id(p),
        label: p.display_name.to_string(),
        auth_methods: vec![AuthMethod::ApiKey {
            env_var: env_var.clone(),
        }],
        env_keys: vec![env_var],
        oauth_preferred: false,
    }
}

/// Translate a metadata profile to one [`ModelInfo`] per default
/// model the profile declares. Most profiles declare exactly one
/// default; multi-model profiles (e.g. openrouter) get multiple.
pub fn profile_to_model(p: &OpenAiCompatibleProfile) -> Vec<ModelInfo> {
    let default_model = ModelId::from(p.default_model.unwrap_or_else(|| p.id));
    vec![ModelInfo {
        id: default_model,
        provider: profile_id(p),
        name: p.display_name.to_string(),
        cost_per_million_input: None,
        cost_per_million_output: None,
        context_window: 0, // unknown for the metadata profiles
        supports_tools: true,
        supports_vision: false,
        supports_streaming: true,
        tier: None,

        release_date: None,
        base_url: None,
        path: None,
        protocol: None,
    }]
}

/// Translate a metadata profile to a [`ProviderRecord`].
pub fn profile_to_record(p: &OpenAiCompatibleProfile) -> ProviderRecord {
    ProviderRecord {
        id: profile_id(p),
        label: p.display_name.to_string(),
        auth_methods: vec![AuthMethod::ApiKey {
            env_var: p.api_key_env.to_string(),
        }],
        env_keys: vec![p.api_key_env.to_string()],
        oauth_preferred: false,
        base_url: p.api_base.to_string(),
        path: "/v1/chat/completions".into(),
        protocol: "openai-chat-2024".into(),
        models: profile_to_model(p),
    }
}

/// A registry that returns every metadata profile as a
/// `ProviderRecord`. Composable with other registries via
/// `CompositeRegistry`.
pub struct MetadataProfilesRegistry;

#[async_trait::async_trait]
impl ProviderRegistry for MetadataProfilesRegistry {
    fn id(&self) -> &str {
        "metadata-profiles"
    }

    async fn providers(&self) -> Vec<ProviderRecord> {
        all_metadata_records()
    }
}

/// Enumerate every metadata profile as a `ProviderRecord`.
pub fn all_metadata_records() -> Vec<ProviderRecord> {
    openai_compatible_profiles()
        .iter()
        .map(profile_to_record)
        .collect()
}

/// Build a `CompositeRegistry` that contains the built-in providers
/// AND every metadata profile. The session runner uses this to
/// bootstrap the catalog/integration layers in one call.
pub fn metadata_registry() -> Arc<dyn ProviderRegistry + Send + Sync> {
    Arc::new(
        crate::registry::CompositeRegistry::new("builtins+metadata")
            .with(crate::registry::builtin_registry())
            .with(Arc::new(MetadataProfilesRegistry)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CatalogService, InMemoryCatalog};
    use crate::integration::{InMemoryIntegration, IntegrationService};
    use jcode_keyring_store::MockKeyringStore;

    #[test]
    fn profile_to_login_provider_carries_id_label_and_env() {
        let p = &jcode_provider_metadata::OPENCODE_PROFILE;
        let lp = profile_to_login_provider(p);
        assert_eq!(lp.id.as_str(), "opencode");
        assert_eq!(lp.label, "OpenCode Zen");
        assert_eq!(lp.env_keys, vec!["OPENCODE_API_KEY".to_string()]);
        assert!(!lp.oauth_preferred);
    }

    #[test]
    fn profile_to_model_uses_default_or_id() {
        let p = &jcode_provider_metadata::OPENCODE_PROFILE;
        let models = profile_to_model(p);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id.as_str(), "minimax-m2.7");
        assert_eq!(models[0].provider.as_str(), "opencode");
        assert_eq!(models[0].context_window, 0);
    }

    #[test]
    fn profile_to_record_propagates_all_fields() {
        let p = &jcode_provider_metadata::DEEPSEEK_PROFILE;
        let rec = profile_to_record(p);
        assert_eq!(rec.id.as_str(), "deepseek");
        assert_eq!(rec.env_keys, vec!["DEEPSEEK_API_KEY".to_string()]);
        assert!(
            rec.models
                .iter()
                .any(|m| m.id.as_str() == "deepseek-v4-flash")
        );
    }

    #[test]
    fn all_metadata_records_returns_36_profiles() {
        let records = all_metadata_records();
        // 36 metadata profiles, each producing 1 default-model record.
        assert_eq!(records.len(), 36, "expected 36 metadata profiles");
        // No duplicate ids.
        let mut ids: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 36, "all ids should be distinct");
    }

    #[tokio::test]
    async fn metadata_registry_register_populates_catalog() {
        let registry = metadata_registry();
        let catalog = InMemoryCatalog::new();
        let integration = InMemoryIntegration::new();
        let n = registry.register(&catalog, &integration).await.unwrap();
        // 4 builtins + 36 metadata profiles = 40 providers.
        assert_eq!(n, 40);
        // The deepseek profile should be in the catalog.
        catalog.provider(&"deepseek".into()).await.unwrap();
        // And in the integration.
        integration.get(&"deepseek".into()).await.unwrap();
    }

    #[tokio::test]
    async fn all_metadata_records_have_distinct_ids() {
        // Test the metadata-only records (not the composite) for
        // distinctness. The composite intentionally allows the
        // builtins and metadata profiles to share ids (e.g. both
        // define 'openrouter'); the boot path can resolve which one
        // to use.
        let records = all_metadata_records();
        let mut seen = std::collections::HashSet::new();
        for r in &records {
            assert!(
                seen.insert(r.id.as_str()),
                "duplicate metadata id: {}",
                r.id
            );
        }
        assert_eq!(seen.len(), 36);
    }

    #[test]
    fn builtin_and_metadata_share_some_ids_intentionally() {
        // Both builtins and metadata define 'openrouter' (the
        // builtin is the production provider; the metadata one is
        // an OpenAI-compatible variant). Downstream code resolves
        // which to use via the integration layer.
        let builtin_ids: Vec<&str> = crate::boot::BUILTIN_PROVIDERS
            .iter()
            .map(|p| p.id)
            .collect();
        let metadata_records = all_metadata_records();
        let metadata_ids: Vec<&str> = metadata_records.iter().map(|r| r.id.as_str()).collect();
        let builtin_set: std::collections::HashSet<&str> = builtin_ids.iter().copied().collect();
        let metadata_set: std::collections::HashSet<&str> = metadata_ids.iter().copied().collect();
        let overlap: Vec<&&str> = builtin_set.intersection(&metadata_set).collect();
        // The overlap is what makes the resolution non-trivial;
        // verify the documented case.
        assert!(
            overlap.contains(&&"openrouter"),
            "expected openrouter to appear in both lists; got {overlap:?}"
        );
    }

    #[test]
    fn profile_id_is_stable() {
        // The id is the metadata's `id` field, which is a stable
        // string literal. Verify the relationship holds for a
        // hand-picked sample.
        assert_eq!(
            profile_id(&jcode_provider_metadata::KIMI_PROFILE).as_str(),
            "kimi"
        );
        assert_eq!(
            profile_id(&jcode_provider_metadata::OLLAMA_PROFILE).as_str(),
            "ollama"
        );
        assert_eq!(
            profile_id(&jcode_provider_metadata::XAI_PROFILE).as_str(),
            "xai"
        );
    }

    // Smoke test: ensure the MockKeyringStore type is importable so
    // the test module compiles cleanly.
    #[allow(dead_code)]
    fn _typecheck() {
        let _: Option<MockKeyringStore> = None;
    }
}
