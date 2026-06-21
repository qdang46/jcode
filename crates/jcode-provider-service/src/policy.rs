//! Provider usage policy -- deny-list based filtering.
//!
//! Mirrors opencode's `PolicyService` which evaluates `"provider.use"`
//! actions to `"allow"` / `"deny"`.  jcode simplifies this to a deny
//! list: a set of [`ProviderId`]s the user has explicitly blocked,
//! sourced from `config.toml` or the `JCODE_DENIED_PROVIDERS` env var.
//!
//! The [`CatalogService`](crate::catalog::CatalogService) calls
//! [`PolicyService::is_allowed`] from both its `finalize` step (removes
//! denied providers from the store at the end of boot registration) and
//! its [`available`](crate::catalog::CatalogService::available) view so
//! a denied provider can never appear in the "available" list even if
//! its registration somehow survives.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use jcode_provider_service::policy::{PolicyService, DenyListPolicy};
//!
//! let policy: Arc<dyn PolicyService> = Arc::new(
//!     DenyListPolicy::new(["antigravity", "copilot"]),
//! );
//! assert!(!policy.is_allowed(&"antigravity".into()));
//! assert!(policy.is_allowed(&"anthropic".into()));
//! ```

use crate::types::ProviderId;
use std::collections::HashSet;

/// Provider usage policy.
///
/// Mirrors opencode's `Policy.evaluate("provider.use", id, "allow")`.
/// Returns `true` when the provider is allowed for use, `false` when
/// denied.
pub trait PolicyService: Send + Sync {
    /// Evaluate whether `provider` may be used.
    ///
    /// `true` = allowed (the provider passes the policy gate).
    /// `false` = denied (the provider should be filtered out of
    /// `available()` and removed during `finalize()`).
    fn is_allowed(&self, provider: &ProviderId) -> bool;

    /// Whether this policy has any rules loaded at all.
    ///
    /// When `false`, the policy gate can be skipped entirely (no
    /// providers are denied).  Mirrors opencode's
    /// `policy.hasStatements()`.
    fn has_rules(&self) -> bool;
}

// ---------------------------------------------------------------------------
// Deny-list implementation
// ---------------------------------------------------------------------------

/// A deny-list policy backed by a simple set of denied provider ids.
///
/// Construct from a list of provider ids:
///
/// ```
/// use jcode_provider_service::policy::DenyListPolicy;
///
/// let policy = DenyListPolicy::new(["antigravity", "copilot"]);
/// assert!(!policy.is_allowed(&"antigravity".into()));
/// ```
///
/// Or from the `JCODE_DENIED_PROVIDERS` env var (comma-separated):
///
/// ```no_run
/// use jcode_provider_service::policy::DenyListPolicy;
///
/// let policy = DenyListPolicy::from_env();
/// ```
pub struct DenyListPolicy {
    denied: HashSet<String>,
}

impl DenyListPolicy {
    /// Create a new deny list from an iterable of provider id strings.
    ///
    /// Provider ids are lowercased and trimmed so matching is
    /// case-insensitive.
    pub fn new(providers: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let denied: HashSet<String> = providers
            .into_iter()
            .map(|p| p.as_ref().trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        Self { denied }
    }

    /// Create a deny list from the `JCODE_DENIED_PROVIDERS` environment
    /// variable.  The value is split on commas, trimmed, and lowercased.
    /// Returns an empty policy when the variable is unset or empty.
    pub fn from_env() -> Self {
        match std::env::var("JCODE_DENIED_PROVIDERS") {
            Ok(val) => Self::parse(val.as_str()),
            Err(_) => Self {
                denied: HashSet::new(),
            },
        }
    }

    /// Parse a comma-separated string of denied provider ids.
    ///
    /// Useful for testability and for callers that already hold the
    /// value (e.g. from config.toml).
    ///
    /// ```
    /// use jcode_provider_service::policy::DenyListPolicy;
    ///
    /// let policy = DenyListPolicy::parse("antigravity, copilot ");
    /// assert!(!policy.is_allowed(&"copilot".into()));
    /// assert!(policy.is_allowed(&"anthropic".into()));
    /// assert!(policy.has_rules());
    /// ```
    pub fn parse(val: &str) -> Self {
        let trimmed = val.trim();
        if trimmed.is_empty() {
            return Self {
                denied: HashSet::new(),
            };
        }
        Self::new(trimmed.split(',').map(|s| s.trim()))
    }

    /// Replace the entire deny list.  Used when config is reloaded at
    /// runtime.
    pub fn set_denied(&mut self, providers: impl IntoIterator<Item = impl AsRef<str>>) {
        self.denied = providers
            .into_iter()
            .map(|p| p.as_ref().trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
    }
}

impl PolicyService for DenyListPolicy {
    fn is_allowed(&self, provider: &ProviderId) -> bool {
        if self.denied.is_empty() {
            return true;
        }
        !self.denied.contains(provider.as_str())
            && !self
                .denied
                .contains(&provider.as_str().to_ascii_lowercase())
    }

    fn has_rules(&self) -> bool {
        !self.denied.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_policy_allows_everything() {
        let policy = DenyListPolicy::new(std::iter::empty::<&str>());
        assert!(policy.is_allowed(&"anthropic".into()));
        assert!(policy.is_allowed(&"openai".into()));
        assert!(!policy.has_rules());
    }

    #[test]
    fn denies_listed_providers() {
        let policy = DenyListPolicy::new(["antigravity", "copilot"]);
        assert!(!policy.is_allowed(&"antigravity".into()));
        assert!(!policy.is_allowed(&"copilot".into()));
        assert!(policy.is_allowed(&"anthropic".into()));
        assert!(policy.is_allowed(&"openai".into()));
        assert!(policy.has_rules());
    }

    #[test]
    fn case_insensitive_matching() {
        let policy = DenyListPolicy::new(["ANTIGRAVITY"]);
        assert!(!policy.is_allowed(&"antigravity".into()));
        assert!(!policy.is_allowed(&"ANTIGRAVITY".into()));
        assert!(!policy.is_allowed(&"AntiGravity".into()));
    }

    #[test]
    fn empty_entry_after_trim_is_ignored() {
        let policy = DenyListPolicy::new(["  antigravity  ", "  "]);
        assert!(!policy.is_allowed(&"antigravity".into()));
        // "  " is empty after trim, so it's filtered out
        assert_eq!(policy.denied.len(), 1);
    }

    #[test]
    fn set_denied_replaces_entire_list() {
        let mut policy = DenyListPolicy::new(["antigravity"]);
        assert!(!policy.is_allowed(&"antigravity".into()));
        policy.set_denied(["openai"]);
        assert!(policy.is_allowed(&"antigravity".into()));
        assert!(!policy.is_allowed(&"openai".into()));
    }

    #[test]
    fn parse_splits_on_comma_and_trims() {
        let policy = DenyListPolicy::parse("  antigravity  , copilot ,");
        assert!(!policy.is_allowed(&"antigravity".into()));
        assert!(!policy.is_allowed(&"copilot".into()));
        assert!(policy.is_allowed(&"anthropic".into()));
    }

    #[test]
    fn parse_returns_empty_policy_for_empty_string() {
        let policy = DenyListPolicy::parse("");
        assert!(!policy.has_rules());
        assert!(policy.is_allowed(&"anthropic".into()));
    }

    #[test]
    fn parse_returns_empty_policy_for_whitespace() {
        let policy = DenyListPolicy::parse("   ");
        assert!(!policy.has_rules());
        assert!(policy.is_allowed(&"anthropic".into()));
    }

    #[test]
    fn has_rules_false_when_no_denied_providers() {
        let policy = DenyListPolicy::new(std::iter::empty::<&str>());
        assert!(!policy.has_rules());
    }

    #[test]
    fn has_rules_true_when_any_denied_providers() {
        let policy = DenyListPolicy::new(["antigravity"]);
        assert!(policy.has_rules());
    }

    #[test]
    fn multiple_providers_is_allowed_and_denied() {
        let policy = DenyListPolicy::new(["antigravity", "copilot", "bedrock"]);
        assert!(!policy.is_allowed(&"antigravity".into()));
        assert!(!policy.is_allowed(&"copilot".into()));
        assert!(!policy.is_allowed(&"bedrock".into()));
        assert!(policy.is_allowed(&"anthropic".into()));
        assert!(policy.is_allowed(&"openai".into()));
        assert!(policy.is_allowed(&"gemini".into()));
    }

    #[test]
    fn set_denied_to_empty_resets_policy() {
        let mut policy = DenyListPolicy::new(["antigravity"]);
        assert!(policy.has_rules());
        policy.set_denied(std::iter::empty::<&str>());
        assert!(!policy.has_rules());
        assert!(policy.is_allowed(&"antigravity".into()));
    }
}
