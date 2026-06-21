use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Specifies how to match or construct a URL path segment.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathSpec {
    /// Exact, static path (e.g. `/v1/messages`).
    Static(String),
    /// Dynamic path with named parameters (e.g. `/v1/projects/{project_id}/models`).
    Dynamic(String),
}

impl PathSpec {
    /// Resolve the path to a string, applying parameter substitutions for dynamic
    /// paths.
    ///
    /// For `Static`, returns the path unchanged.
    /// For `Dynamic`, substitutes `{name}` placeholders with values from `params`.
    /// Unknown placeholders are left as-is.
    pub fn resolve(&self, params: &HashMap<String, String>) -> String {
        match self {
            PathSpec::Static(p) => p.clone(),
            PathSpec::Dynamic(p) => {
                let mut result = p.clone();
                for (key, value) in params {
                    let placeholder = format!("{{{key}}}");
                    result = result.replace(&placeholder, value);
                }
                result
            }
        }
    }
}

/// Describes how to reach a provider endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Endpoint {
    /// Base URL of the provider API (e.g. `https://api.anthropic.com`).
    pub base_url: String,
    /// Path specification for the specific resource.
    pub path: PathSpec,
    /// Optional query parameters (static key-value pairs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<HashMap<String, String>>,
}

impl Endpoint {
    /// Build a full URL string from the endpoint.
    ///
    /// Substitutes dynamic path parameters and appends query parameters.
    pub fn build_url(&self, path_params: &HashMap<String, String>) -> String {
        let path = self.path.resolve(path_params);
        let base = self.base_url.trim_end_matches('/');
        let path = path.trim_start_matches('/');

        let url = format!("{base}/{path}");

        match &self.query {
            None => url,
            Some(params) if params.is_empty() => url,
            Some(params) => {
                let qs: Vec<String> = params
                    .iter()
                    .map(|(k, v)| format!("{}={}", urlencoding(k), urlencoding(v)))
                    .collect();
                format!("{url}?{}", qs.join("&"))
            }
        }
    }
}

fn urlencoding(s: &str) -> String {
    // Simple percent-encoding of common special characters
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push_str("%20"),
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_spec_static() {
        let spec = PathSpec::Static("/v1/messages".into());
        assert_eq!(spec.resolve(&HashMap::new()), "/v1/messages");
    }

    #[test]
    fn test_path_spec_dynamic() {
        let spec = PathSpec::Dynamic("/v1/projects/{project_id}/models".into());
        let mut params = HashMap::new();
        params.insert("project_id".into(), "abc123".into());
        assert_eq!(spec.resolve(&params), "/v1/projects/abc123/models");
    }

    #[test]
    fn test_path_spec_dynamic_unknown_param_left_as_is() {
        let spec = PathSpec::Dynamic("/v1/{a}/{b}".into());
        let mut params = HashMap::new();
        params.insert("a".into(), "x".into());
        assert_eq!(spec.resolve(&params), "/v1/x/{b}");
    }

    #[test]
    fn test_endpoint_build_url_static() {
        let ep = Endpoint {
            base_url: "https://api.anthropic.com".into(),
            path: PathSpec::Static("/v1/messages".into()),
            query: None,
        };
        assert_eq!(
            ep.build_url(&HashMap::new()),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn test_endpoint_build_url_with_query() {
        let mut query = HashMap::new();
        query.insert("model".into(), "claude-sonnet-4".into());
        let ep = Endpoint {
            base_url: "https://api.openai.com".into(),
            path: PathSpec::Static("/v1/chat/completions".into()),
            query: Some(query),
        };
        let url = ep.build_url(&HashMap::new());
        assert!(url.starts_with("https://api.openai.com/v1/chat/completions?"));
        assert!(url.contains("model=claude-sonnet-4"));
    }

    #[test]
    fn test_endpoint_build_url_dynamic() {
        let mut path_params = HashMap::new();
        path_params.insert("model_id".into(), "gpt-4o".into());
        let ep = Endpoint {
            base_url: "https://api.openai.com".into(),
            path: PathSpec::Dynamic("/v1/models/{model_id}/completions".into()),
            query: None,
        };
        assert_eq!(
            ep.build_url(&path_params),
            "https://api.openai.com/v1/models/gpt-4o/completions"
        );
    }
}
