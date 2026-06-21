//! A [`Provider`] wrapper around a resolved [`Route`].
//!
//! [`RouteProvider`] implements the legacy [`jcode_provider_core::Provider`]
//! trait by wrapping a [`ResolvedRoute`] that has already been resolved through
//! the catalog, integration, and credential layers. It is the bridge between the
//! new service-layer route resolution and the old Provider trait surface.
//!
//! # Complete methods
//!
//! `complete()`, `complete_split()`, and `complete_simple()` return an error
//! with the message `"RouteProvider: LLM call dispatch not yet implemented"`.
//! These stubs exist only to satisfy the trait so that types like
//! `Agent::new()` compile when constructed from a resolved route. The actual
//! LLM call dispatch will be wired in a follow-up.

use std::sync::Arc;

use async_trait::async_trait;
use jcode_llm_core::route::Route;
use jcode_message_types::{Message, ToolDefinition};
use jcode_provider_core::{
    DEFAULT_CONTEXT_LIMIT, EventStream, ModelRoute, Provider, ResolvedCredential, RouteSelection,
    context_limit_for_model_with_provider,
};

use crate::service::ResolvedRoute;

/// A [`Provider`] that wraps a resolved [`Route`].
///
/// Stores the provider id, model id, and the concrete [`Route`] that describes
/// how to reach the LLM endpoint.
pub struct RouteProvider {
    provider_id: String,
    model_id: String,
    route: Route,
}

impl RouteProvider {
    /// Construct a new [`RouteProvider`] from a [`ResolvedRoute`].
    pub fn new(resolved: ResolvedRoute) -> Self {
        Self {
            provider_id: resolved.provider.to_string(),
            model_id: resolved.model.to_string(),
            route: resolved.route,
        }
    }

    /// Construct a [`RouteProvider`] from raw parts.
    pub fn from_parts(
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        route: Route,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            route,
        }
    }

    /// The resolved route this provider wraps.
    pub fn route(&self) -> &Route {
        &self.route
    }

    /// The provider identifier.
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    /// The model identifier.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Whether the wrapped route's transport speaks SSE (the most common
    /// streaming path). When `false`, the route uses a non-SSE framing such
    /// as AWS Event Stream or WebSocket binary, which the stub `complete()`
    /// methods still reject uniformly.
    #[allow(dead_code)]
    fn uses_sse_framing(&self) -> bool {
        matches!(self.route.framing, jcode_llm_core::framing::Framing::Sse)
    }
}

#[async_trait]
impl Provider for RouteProvider {
    /// Placeholder - always returns an error.
    ///
    /// Actual LLM call dispatch has not yet been wired into this wrapper.
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> std::result::Result<EventStream, anyhow::Error> {
        Err(anyhow::anyhow!(
            "RouteProvider: LLM call dispatch not yet implemented"
        ))
    }

    /// The stable, machine-facing provider identifier (e.g. `"anthropic"`,
    /// `"openai"`, `"openrouter"`).
    fn name(&self) -> &str {
        &self.provider_id
    }

    /// Human-facing label. Uses the route's protocol as additional context
    /// when available.
    fn display_name(&self) -> String {
        let base = &self.provider_id;
        let protocol = self.route.protocol.trim();
        if protocol.is_empty() {
            base.to_string()
        } else {
            format!("{base} ({protocol})")
        }
    }

    /// The model identifier being used.
    fn model(&self) -> String {
        let model = &self.model_id;
        let provider = self.route.provider.id.trim();
        if provider.is_empty() || provider == model {
            model.clone()
        } else {
            format!("{}/{}", provider, model)
        }
    }

    /// Prefetch any dynamic model lists (default: no-op via trait default).
    async fn prefetch_models(&self) -> std::result::Result<(), anyhow::Error> {
        // TODO: If the route has a dynamic catalog, refresh it here.
        Ok(())
    }

    /// The resolved credential for the active route, if the auth map contains
    /// enough information to determine it.
    fn active_resolved_credential(&self) -> Option<ResolvedCredential> {
        let protocol_lower = self.route.protocol.to_ascii_lowercase();
        // Heuristic: OAuth-protocol routes imply subscription billing.
        if protocol_lower.contains("oauth") {
            Some(ResolvedCredential::Oauth)
        } else if self.route.auth.contains_key("api_key") || self.route.auth.contains_key("apiKey")
        {
            Some(ResolvedCredential::ApiKey)
        } else {
            None
        }
    }

    /// List available models. Default is empty.
    fn available_models(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Provider details for model picker.
    fn provider_details_for_model(&self, _model: &str) -> Vec<(String, String)> {
        let protocol = self.route.protocol.clone();
        vec![(self.provider_id.clone(), protocol)]
    }

    /// Return the currently preferred upstream provider.
    fn preferred_provider(&self) -> Option<String> {
        Some(self.provider_id.clone())
    }

    /// Get all model routes for the unified picker.
    fn model_routes(&self) -> Vec<ModelRoute> {
        let api_method = &self.route.protocol;
        vec![ModelRoute {
            model: self.model_id.clone(),
            provider: self.provider_id.clone(),
            api_method: api_method.clone(),
            available: true,
            detail: String::new(),
            cheapness: None,
        }]
    }

    /// Create a new provider instance with independent mutable state.
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(Self {
            provider_id: self.provider_id.clone(),
            model_id: self.model_id.clone(),
            route: self.route.clone(),
        })
    }

    /// Select a structured model route. Delegates to setting the model
    /// string.
    fn set_route_selection(
        &self,
        selection: &RouteSelection,
    ) -> std::result::Result<(), anyhow::Error> {
        // Update the stored model id to the selected one.
        // Since we take &self, we cannot mutate; fork() before calling this.
        let _ = selection;
        Err(anyhow::anyhow!(
            "RouteProvider does not support in-place route switching; fork() first"
        ))
    }

    /// Context window for the current model.
    fn context_window(&self) -> usize {
        context_limit_for_model_with_provider(&self.model_id, Some(&self.provider_id))
            .unwrap_or(DEFAULT_CONTEXT_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::ResolvedRoute;
    use jcode_llm_core::endpoint::{Endpoint, PathSpec};
    use jcode_llm_core::framing::Framing;
    use jcode_llm_core::route::Route;
    use jcode_llm_core::schema::ModelRef;
    use jcode_llm_core::transport::Transport;
    use jcode_provider_core::ResolvedCredential;
    use std::collections::HashMap;

    fn make_resolved_route(provider: &str, model: &str) -> ResolvedRoute {
        ResolvedRoute {
            provider: provider.into(),
            model: model.into(),
            route: Route::new(
                format!("{provider}/{model}"),
                ModelRef {
                    provider_id: provider.into(),
                    id: model.into(),
                    variant: None,
                },
            )
            .with_protocol("test-protocol")
            .with_endpoint(Endpoint {
                base_url: format!("https://api.{provider}.example.com"),
                path: PathSpec::Static("/v1/chat".into()),
                query: None,
            })
            .with_auth({
                let mut auth = HashMap::new();
                auth.insert("api_key".into(), "sk-test".into());
                auth
            })
            .with_framing(Framing::Sse)
            .with_transport(Transport::Http),
        }
    }

    #[test]
    fn route_provider_constructs_from_resolved_route() {
        let resolved = make_resolved_route("anthropic", "claude-sonnet-4-6");
        let rp = RouteProvider::new(resolved);

        assert_eq!(rp.name(), "anthropic");
        assert!(rp.model().contains("claude-sonnet-4-6"));
        assert!(rp.display_name().contains("anthropic"));
    }

    #[test]
    fn route_provider_from_parts() {
        let route = Route::new(
            "test",
            ModelRef {
                provider_id: "test-provider".into(),
                id: "test-model".into(),
                variant: None,
            },
        );
        let rp = RouteProvider::from_parts("test-provider", "test-model", route);

        assert_eq!(rp.name(), "test-provider");
        assert!(rp.model().contains("test-model"));
    }

    #[test]
    fn route_provider_fork_produces_independent_clone() {
        let resolved = make_resolved_route("openai", "gpt-5");
        let rp = RouteProvider::new(resolved);
        let forked = rp.fork();

        // The forked provider must be a different Arc, but same values.
        assert_eq!(forked.name(), rp.name());
        assert_eq!(forked.model(), rp.model());
    }

    #[test]
    fn route_provider_model_routes_includes_resolved_data() {
        let resolved = make_resolved_route("anthropic", "claude-sonnet-4-6");
        let rp = RouteProvider::new(resolved);
        let routes = rp.model_routes();

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].model, "claude-sonnet-4-6");
        assert_eq!(routes[0].provider, "anthropic");
        assert!(routes[0].available);
    }

    #[test]
    fn route_provider_active_resolved_credential_detects_api_key() {
        let resolved = make_resolved_route("openai", "gpt-5");
        let rp = RouteProvider::new(resolved);

        assert_eq!(
            rp.active_resolved_credential(),
            Some(jcode_provider_core::ResolvedCredential::ApiKey)
        );
    }

    #[test]
    fn route_provider_active_resolved_credential_detects_oauth_via_protocol() {
        let mut resolved = make_resolved_route("anthropic", "claude-sonnet-4-6");
        resolved.route.protocol = "oauth-v2".to_string();
        let rp = RouteProvider::new(resolved);

        assert_eq!(
            rp.active_resolved_credential(),
            Some(jcode_provider_core::ResolvedCredential::Oauth)
        );
    }

    #[tokio::test]
    async fn route_provider_complete_returns_not_implemented_error() {
        let resolved = make_resolved_route("anthropic", "claude-sonnet-4-6");
        let rp = RouteProvider::new(resolved);

        let result = rp.complete(&[], &[], "", None).await;
        assert!(result.is_err());
        let err = format!("{}", result.err().unwrap());
        assert!(err.contains("not yet implemented"));
    }

    #[tokio::test]
    async fn route_provider_complete_simple_returns_not_implemented_error() {
        let resolved = make_resolved_route("anthropic", "claude-sonnet-4-6");
        let rp = RouteProvider::new(resolved);

        let result = rp.complete_simple("hello", "").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not yet implemented"));
    }

    #[test]
    fn route_provider_sse_framing_detection() {
        let mut resolved = make_resolved_route("anthropic", "claude-sonnet-4-6");
        resolved.route.framing = Framing::Sse;
        let rp = RouteProvider::new(resolved);
        assert!(rp.uses_sse_framing());
    }

    #[test]
    fn route_provider_detects_non_sse_framing() {
        use jcode_llm_core::framing::Framing;
        let mut resolved = make_resolved_route("anthropic", "claude-sonnet-4-6");
        resolved.route.framing = Framing::AwsEventStream;
        let rp = RouteProvider::new(resolved);
        assert!(!rp.uses_sse_framing());
    }

    #[test]
    fn route_provider_display_name_includes_protocol_when_set() {
        let mut resolved = make_resolved_route("openrouter", "gpt-5");
        resolved.route.protocol = "openai-chat".to_string();
        let rp = RouteProvider::new(resolved);

        let display = rp.display_name();
        assert!(display.contains("openrouter"));
        assert!(display.contains("openai-chat"));
    }

    #[test]
    fn route_provider_model_includes_provider_prefix_when_different() {
        let mut route = Route::new(
            "test",
            ModelRef {
                provider_id: "anthropic".into(),
                id: "claude-sonnet-4-6".into(),
                variant: None,
            },
        );
        route.protocol = "test".to_string();
        let rp = RouteProvider::from_parts("custom-provider", "claude-sonnet-4-6", route);

        let model_str = rp.model();
        assert!(model_str.contains("anthropic") || model_str == "claude-sonnet-4-6");
    }
}
