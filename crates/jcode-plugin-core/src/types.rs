use serde::{Deserialize, Serialize};

/// Unique identifier for a plugin — npm package name or file path
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginId(String);

impl PluginId {
    pub fn npm(name: &str) -> Self {
        Self(format!("npm:{name}"))
    }
    pub fn file(path: &str) -> Self {
        Self(format!("file:{path}"))
    }
    pub fn bundled(name: &str) -> Self {
        Self(format!("builtin:{name}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Extract the short name (strip prefix)
    pub fn short_name(&self) -> &str {
        self.0
            .split_once(':')
            .map(|(_, name)| name)
            .unwrap_or(&self.0)
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for PluginId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginVersion {
    pub semver: semver::Version,
    pub jcode_min_version: Option<semver::Version>,
    pub jcode_max_version: Option<semver::Version>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginState {
    Discovered,
    Loading,
    Loaded,
    Active,
    Error(String),
    Disabled,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginOrigin {
    NpmPackage { name: String, version: String },
    LocalFile { path: String },
    Builtin { name: String },
    Remote { url: String },
}
