use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use async_trait::async_trait;

/// A simple HTTP request representation that can be modified by auth middleware.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

/// Errors that can occur during authentication of a request.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AuthError {
    /// Authentication credentials are missing.
    #[error("missing authentication credentials")]
    Missing,
    /// Authentication credentials are invalid.
    #[error("invalid authentication credentials")]
    Invalid,
    /// A custom authentication error.
    #[error("{0}")]
    Custom(String),
}

/// Auth trait -- a middleware that can apply authentication to an HTTP request.
///
/// Implementations are `Send + Sync` so they can be used across threads and
/// stored behind `Box<dyn Auth>`.
#[async_trait]
pub trait Auth: Send + Sync {
    /// Apply this authentication strategy to the request.
    ///
    /// Returns `Ok(())` on success, or an `AuthError` describing what went wrong.
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError>;

    /// A short, human-readable description of this authentication strategy.
    fn describe(&self) -> &str;
}

#[async_trait]
impl Auth for Box<dyn Auth> {
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        (**self).apply(req).await
    }

    fn describe(&self) -> &str {
        (**self).describe()
    }
}

impl dyn Auth {
    /// Combine this auth with another fallback auth.
    ///
    /// The returned `OrElseAuth` will try `self` first, and if it returns an
    /// `Err`, try `other` instead.
    pub fn or_else(self: Box<Self>, other: Box<dyn Auth>) -> OrElseAuth {
        OrElseAuth {
            primary: self,
            fallback: other,
        }
    }
}

// ---------------------------------------------------------------------------
// Implementations
// ---------------------------------------------------------------------------

/// Authenticates by adding an `Authorization: Bearer <key>` header.
pub struct BearerAuth {
    api_key: String,
}

/// Authenticates by adding a custom header with the given name and value.
pub struct HeaderAuth {
    name: String,
    key: String,
}

/// Removes the named header from the request (useful for stripping unwanted auth).
pub struct RemoveAuth {
    name: String,
}

/// Uses an arbitrary closure or function pointer to apply authentication.
pub struct CustomAuth<F: Fn(&mut Request) -> Result<(), AuthError> + Send + Sync + 'static> {
    f: Arc<F>,
}

/// Optionally adds a `Bearer` token; if the key is empty, it skips without error.
pub struct OptionalAuth {
    api_key: String,
}

/// Reads the API key from the named environment variable and adds it as a `Bearer` token.
pub struct ConfigAuth {
    env_var: String,
}

/// Tries the primary auth first; if it fails, falls back to the secondary auth.
pub struct OrElseAuth {
    primary: Box<dyn Auth>,
    fallback: Box<dyn Auth>,
}

#[async_trait]
impl Auth for BearerAuth {
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        req.headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", self.api_key),
        );
        Ok(())
    }

    fn describe(&self) -> &str {
        "Bearer token authentication"
    }
}

#[async_trait]
impl Auth for HeaderAuth {
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        req.headers.insert(self.name.clone(), self.key.clone());
        Ok(())
    }

    fn describe(&self) -> &str {
        "Custom header authentication"
    }
}

#[async_trait]
impl Auth for RemoveAuth {
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        req.headers.remove(&self.name);
        Ok(())
    }

    fn describe(&self) -> &str {
        "Header removal authentication"
    }
}

#[async_trait]
impl<F> Auth for CustomAuth<F>
where
    F: Fn(&mut Request) -> Result<(), AuthError> + Send + Sync + 'static,
{
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        (self.f)(req)
    }

    fn describe(&self) -> &str {
        "Custom authentication function"
    }
}

#[async_trait]
impl Auth for OptionalAuth {
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        if !self.api_key.is_empty() {
            req.headers.insert(
                "Authorization".to_string(),
                format!("Bearer {}", self.api_key),
            );
        }
        Ok(())
    }

    fn describe(&self) -> &str {
        "Optional Bearer token authentication"
    }
}

#[async_trait]
impl Auth for ConfigAuth {
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        let key = env::var(&self.env_var).map_err(|_| AuthError::Missing)?;
        req.headers
            .insert("Authorization".to_string(), format!("Bearer {}", key));
        Ok(())
    }

    fn describe(&self) -> &str {
        "Environment variable Bearer token authentication"
    }
}

#[async_trait]
impl Auth for OrElseAuth {
    async fn apply(&self, req: &mut Request) -> Result<(), AuthError> {
        if self.primary.apply(req).await.is_ok() {
            return Ok(());
        }
        self.fallback.apply(req).await
    }

    fn describe(&self) -> &str {
        "or_else auth combinator"
    }
}

// ---------------------------------------------------------------------------
// Constructor functions
// ---------------------------------------------------------------------------

/// Create a `BearerAuth` that adds an `Authorization: Bearer <key>` header.
pub fn bearer(api_key: String) -> BearerAuth {
    BearerAuth { api_key }
}

/// Create a `HeaderAuth` that adds a custom header.
pub fn header(name: String, key: String) -> HeaderAuth {
    HeaderAuth { name, key }
}

/// Create a `RemoveAuth` that removes the named header.
pub fn remove(name: String) -> RemoveAuth {
    RemoveAuth { name }
}

/// Create a `CustomAuth` from an arbitrary closure.
pub fn custom<F>(f: F) -> CustomAuth<F>
where
    F: Fn(&mut Request) -> Result<(), AuthError> + Send + Sync + 'static,
{
    CustomAuth { f: Arc::new(f) }
}

/// Create an `OptionalAuth` that adds a `Bearer` token only when the key is
/// non-empty.
pub fn optional(api_key: String) -> OptionalAuth {
    OptionalAuth { api_key }
}

/// Create a `ConfigAuth` that reads the API key from an environment variable.
pub fn config(env_var: String) -> ConfigAuth {
    ConfigAuth { env_var }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- BearerAuth ---------------------------------------------------------

    #[tokio::test]
    async fn test_bearer_auth_applies_header() {
        let auth = bearer("sk-secret".into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com/v1/messages".into(),
            headers: HashMap::new(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer sk-secret".to_string())
        );
    }

    #[tokio::test]
    async fn test_bearer_auth_with_empty_key() {
        let auth = bearer("".into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com/v1/messages".into(),
            headers: HashMap::new(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer ".to_string())
        );
    }

    #[tokio::test]
    async fn test_bearer_auth_describe() {
        let auth = bearer("key".into());
        assert_eq!(auth.describe(), "Bearer token authentication");
    }

    // -- HeaderAuth ---------------------------------------------------------

    #[tokio::test]
    async fn test_header_auth_applies_header() {
        let auth = header("X-API-Key".into(), "my-key".into());
        let mut req = Request {
            method: "POST".into(),
            url: "https://api.example.com/data".into(),
            headers: HashMap::new(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert_eq!(req.headers.get("X-API-Key"), Some(&"my-key".to_string()));
    }

    #[tokio::test]
    async fn test_header_auth_overwrites_existing() {
        let auth = header("Authorization".into(), "Custom val".into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: [("Authorization".into(), "original".into())].into(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Custom val".to_string())
        );
    }

    #[tokio::test]
    async fn test_header_auth_describe() {
        let auth = header("X-Key".into(), "v".into());
        assert_eq!(auth.describe(), "Custom header authentication");
    }

    // -- RemoveAuth ---------------------------------------------------------

    #[tokio::test]
    async fn test_remove_auth_removes_header() {
        let auth = remove("Authorization".into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: [("Authorization".into(), "Bearer old".into())].into(),
            body: None,
        };
        assert!(req.headers.contains_key("Authorization"));
        auth.apply(&mut req).await.unwrap();
        assert!(!req.headers.contains_key("Authorization"));
    }

    #[tokio::test]
    async fn test_remove_auth_nonexistent_header() {
        let auth = remove("X-Nonexistent".into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        // Removing a non-existent header should be a no-op, not an error.
        auth.apply(&mut req).await.unwrap();
    }

    #[tokio::test]
    async fn test_remove_auth_describe() {
        let auth = remove("Authorization".into());
        assert_eq!(auth.describe(), "Header removal authentication");
    }

    // -- CustomAuth ---------------------------------------------------------

    #[tokio::test]
    async fn test_custom_auth_closure() {
        let auth = custom(|req: &mut Request| {
            req.headers.insert("X-Custom".into(), "custom-value".into());
            Ok(())
        });
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("X-Custom"),
            Some(&"custom-value".to_string())
        );
    }

    #[tokio::test]
    async fn test_custom_auth_error() {
        let auth = custom(|_: &mut Request| Err(AuthError::Custom("nope".into())));
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        let err = auth.apply(&mut req).await.unwrap_err();
        assert!(matches!(err, AuthError::Custom(_)));
        assert_eq!(err.to_string(), "nope");
    }

    #[tokio::test]
    async fn test_custom_auth_describe() {
        let auth = custom(|_: &mut Request| Ok(()));
        assert_eq!(auth.describe(), "Custom authentication function");
    }

    // -- OptionalAuth -------------------------------------------------------

    #[tokio::test]
    async fn test_optional_auth_with_key() {
        let auth = optional("sk-secret".into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer sk-secret".to_string())
        );
    }

    #[tokio::test]
    async fn test_optional_auth_empty_key() {
        let auth = optional("".into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert!(req.headers.is_empty());
    }

    #[tokio::test]
    async fn test_optional_auth_describe() {
        let auth = optional("key".into());
        assert_eq!(auth.describe(), "Optional Bearer token authentication");
    }

    // -- ConfigAuth ---------------------------------------------------------

    #[tokio::test]
    async fn test_config_auth_success() {
        let var_name = "JCODE_TEST_AUTH_KEY_UNIQUE_001";
        // SAFETY: test-only env var mutation in a single-threaded test context.
        unsafe { env::set_var(var_name, "from-env") };
        let auth = config(var_name.into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        auth.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer from-env".to_string())
        );
        // SAFETY: test-only env var mutation in a single-threaded test context.
        unsafe { env::remove_var(var_name) };
    }

    #[tokio::test]
    async fn test_config_auth_missing_env() {
        let var_name = "JCODE_TEST_AUTH_KEY_DOES_NOT_EXIST";
        // SAFETY: test-only env var mutation in a single-threaded test context.
        unsafe { env::remove_var(var_name) };
        let auth = config(var_name.into());
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        let err = auth.apply(&mut req).await.unwrap_err();
        assert!(matches!(err, AuthError::Missing));
    }

    #[tokio::test]
    async fn test_config_auth_describe() {
        let auth = config("MY_VAR".into());
        assert_eq!(
            auth.describe(),
            "Environment variable Bearer token authentication"
        );
    }

    // -- OrElseAuth ---------------------------------------------------------

    #[tokio::test]
    async fn test_or_else_primary_succeeds() {
        let primary = bearer("primary-key".into());
        let fallback = bearer("fallback-key".into());
        let combined = Box::new(primary) as Box<dyn Auth>;
        let combined = combined.or_else(Box::new(fallback));

        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        combined.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer primary-key".to_string())
        );
    }

    #[tokio::test]
    async fn test_or_else_fallback_succeeds() {
        let primary = custom(|_: &mut Request| Err(AuthError::Missing));
        let fallback = bearer("fallback-key".into());
        let combined = Box::new(primary) as Box<dyn Auth>;
        let combined = combined.or_else(Box::new(fallback));

        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        combined.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer fallback-key".to_string())
        );
    }

    #[tokio::test]
    async fn test_or_else_both_fail() {
        let primary = custom(|_: &mut Request| Err(AuthError::Missing));
        let fallback = custom(|_: &mut Request| Err(AuthError::Invalid));
        let combined = Box::new(primary) as Box<dyn Auth>;
        let combined = combined.or_else(Box::new(fallback));

        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        let err = combined.apply(&mut req).await.unwrap_err();
        assert!(matches!(err, AuthError::Invalid));
    }

    #[tokio::test]
    async fn test_or_else_auth_describe() {
        let primary = bearer("k".into());
        let fallback = bearer("k2".into());
        let combined = Box::new(primary) as Box<dyn Auth>;
        let combined = combined.or_else(Box::new(fallback));
        assert_eq!(combined.describe(), "or_else auth combinator");
    }

    // -- Box<dyn Auth> delegation -------------------------------------------

    #[tokio::test]
    async fn test_boxed_dyn_auth_delegates_apply() {
        let inner = bearer("boxed-key".into());
        let boxed: Box<dyn Auth> = Box::new(inner);
        let mut req = Request {
            method: "GET".into(),
            url: "https://api.example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        boxed.apply(&mut req).await.unwrap();
        assert_eq!(
            req.headers.get("Authorization"),
            Some(&"Bearer boxed-key".to_string())
        );
    }

    #[tokio::test]
    async fn test_boxed_dyn_auth_delegates_describe() {
        let boxed: Box<dyn Auth> = Box::new(remove("X".into()));
        assert_eq!(boxed.describe(), "Header removal authentication");
    }

    // -- AuthError display and Debug ----------------------------------------

    #[test]
    fn test_auth_error_display() {
        assert_eq!(
            AuthError::Missing.to_string(),
            "missing authentication credentials"
        );
        assert_eq!(
            AuthError::Invalid.to_string(),
            "invalid authentication credentials"
        );
        assert_eq!(AuthError::Custom("oops".into()).to_string(), "oops");
    }

    #[test]
    fn test_auth_error_debug() {
        let err = AuthError::Custom("oops".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("Custom"));
        assert!(debug.contains("oops"));
    }

    // -- Request default construction ---------------------------------------

    #[test]
    fn test_request_default_fields() {
        let req = Request {
            method: "GET".into(),
            url: "https://example.com".into(),
            headers: HashMap::new(),
            body: None,
        };
        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://example.com");
        assert!(req.headers.is_empty());
        assert!(req.body.is_none());
    }
}
