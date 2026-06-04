use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    #[serde(default)]
    pub enable: Vec<String>,
    #[serde(default)]
    pub disable: Vec<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub fail_closed: Option<bool>,
    #[serde(default)]
    pub sources: Option<Vec<PluginSource>>,
    #[serde(default)]
    pub settings: HashMap<String, HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub features: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub plugins: HashMap<String, PluginPerPluginConfig>,
    #[serde(default)]
    pub skip_hooks: bool,
    #[serde(default)]
    pub force_deny: bool,
}

impl PluginConfig {
    pub fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("JCODE_DISABLE_PLUGINS") {
            if val == "1" || val.eq_ignore_ascii_case("true") {
                self.mode = Some("none".to_string());
            }
        }
        if let Ok(val) = std::env::var("JCODE_SKIP_PLUGINS") {
            if val == "1" || val.eq_ignore_ascii_case("true") {
                self.skip_hooks = true;
            }
        }
        if let Ok(val) = std::env::var("JCODE_PLUGIN_MODE") {
            self.mode = Some(val);
        }
        if let Ok(val) = std::env::var("JCODE_TEAM_WORKER") {
            if val == "1" || val.eq_ignore_ascii_case("true") {
                self.force_deny = true;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginSource {
    #[serde(rename = "npm")]
    Npm {
        package: String,
        #[serde(default)]
        version: Option<String>,
    },
    #[serde(rename = "file")]
    File {
        path: String,
    },
    #[serde(rename = "directory")]
    Directory {
        path: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPerPluginConfig {
    #[serde(default)]
    pub enable: Option<bool>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryPaths {
    pub plugin_dirs: Vec<PathBuf>,
    pub npm_cache: PathBuf,
    pub tool_dirs: Vec<PathBuf>,
}

impl Default for DiscoveryPaths {
    fn default() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let jcode_dir = home.join(".jcode");
        Self {
            plugin_dirs: vec![jcode_dir.join("plugins")],
            npm_cache: jcode_dir.join("cache").join("packages"),
            tool_dirs: vec![jcode_dir.join("tools")],
        }
    }
}

/// Check if a package name is valid
pub fn is_valid_package_name(name: &str) -> bool {
    let re = regex::Regex::new(r"^@?[a-z0-9][a-z0-9._-]*/?[a-z0-9][a-z0-9._-]*$").unwrap();
    re.is_match(name) && !name.contains("..") && !name.contains(';') && !name.contains('|')
}

/// Sanitize a package name for filesystem use
pub fn sanitize_name(name: &str) -> String {
    name.replace('/', "__").replace('@', "")
}
