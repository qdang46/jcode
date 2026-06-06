use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::errors::PluginError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub package_name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub kind: PluginKind,
    #[serde(default)]
    pub entry: PluginEntry,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub features: HashMap<String, PluginFeature>,
    #[serde(default)]
    pub settings: HashMap<String, SettingSchema>,
    #[serde(default)]
    pub engines: PluginEngines,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Default for PluginManifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            package_name: String::new(),
            version: "0.1.0".into(),
            description: None,
            author: None,
            license: None,
            kind: PluginKind::Server,
            entry: PluginEntry::default(),
            capabilities: PluginCapabilities::default(),
            features: HashMap::new(),
            settings: HashMap::new(),
            engines: PluginEngines::default(),
            icon: None,
            homepage: None,
            repository: None,
            tags: Vec::new(),
        }
    }
}

impl PluginManifest {
    pub fn from_package_json(value: &serde_json::Value) -> Result<Self, PluginError> {
        let section = value
            .get("jcode")
            .or_else(|| value.get("pi"))
            .ok_or_else(|| PluginError::InvalidManifest("missing 'jcode' or 'pi' field".into()))?;
        serde_json::from_value(section.clone())
            .map_err(|e| PluginError::InvalidManifest(e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PluginKind {
    #[serde(rename = "server")]
    Server,
    #[serde(rename = "tui")]
    Tui,
    #[serde(rename = "both")]
    Both,
}

impl Default for PluginKind {
    fn default() -> Self {
        Self::Server
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub tui: Option<String>,
    #[serde(default)]
    pub both: Option<String>,
}

impl Default for PluginEntry {
    fn default() -> Self {
        Self {
            server: None,
            tui: None,
            both: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub fs_read: Vec<String>,
    #[serde(default)]
    pub fs_write: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub shell: bool,
    #[serde(default)]
    pub register_tools: bool,
    #[serde(default)]
    pub register_commands: bool,
    #[serde(default)]
    pub register_providers: bool,
    #[serde(default)]
    pub read_config: bool,
    #[serde(default)]
    pub write_config: bool,
    #[serde(default)]
    pub env_vars: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub llm_access: bool,
    #[serde(default)]
    pub session_access: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginFeature {
    pub description: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub entry: Option<String>,
    #[serde(default)]
    pub additional_capabilities: Option<PluginCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SettingSchema {
    #[serde(rename = "string")]
    String {
        description: String,
        #[serde(default)]
        default: Option<String>,
        #[serde(default)]
        secret: bool,
        #[serde(default)]
        env: Option<String>,
        #[serde(default)]
        pattern: Option<String>,
        #[serde(default)]
        max_length: Option<usize>,
    },
    #[serde(rename = "number")]
    Number {
        description: String,
        #[serde(default)]
        default: Option<f64>,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
    #[serde(rename = "boolean")]
    Boolean {
        description: String,
        #[serde(default)]
        default: Option<bool>,
    },
    #[serde(rename = "enum")]
    Enum {
        description: String,
        #[serde(default)]
        default: Option<String>,
        values: Vec<String>,
    },
    #[serde(rename = "array")]
    Array {
        description: String,
        #[serde(default)]
        default: Option<Vec<serde_json::Value>>,
        items: Box<SettingSchema>,
        #[serde(default)]
        max_items: Option<usize>,
    },
    #[serde(rename = "object")]
    Object {
        description: String,
        #[serde(default)]
        default: Option<serde_json::Value>,
        #[serde(default)]
        properties: HashMap<String, SettingSchema>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEngines {
    #[serde(default)]
    pub jcode: Option<String>,
}

impl Default for PluginEngines {
    fn default() -> Self {
        Self { jcode: None }
    }
}
