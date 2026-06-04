use serde::Serialize;

/// Serialize a value to a JSON string
pub fn to_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

/// Deserialize from a JSON string
pub fn from_json<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(json)
}

/// Serialize to a JSON Value
pub fn to_value<T: Serialize>(value: &T) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(value)
}
