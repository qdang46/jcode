use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The underlying transport protocol used to communicate with the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// Plain HTTP/1.1 or HTTP/2 request-response.
    Http,
    /// Bidirectional WebSocket connection.
    WebSocket,
}
