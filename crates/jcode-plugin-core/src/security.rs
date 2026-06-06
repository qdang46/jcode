use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityChain {
    pub deny_list: CapabilitySet,
    pub global_deny: CapabilitySet,
    pub allow_list: CapabilitySet,
    pub global_default: AccessDefault,
    pub mode: AccessMode,
}

impl Default for CapabilityChain {
    fn default() -> Self {
        Self {
            deny_list: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            allow_list: CapabilitySet::default(),
            global_default: AccessDefault::Deny,
            mode: AccessMode::All,
        }
    }
}

impl CapabilityChain {
    /// Check if a resource access is allowed. Returns AccessDecision.
    /// Evaluation order: mode -> deny_list -> global_deny -> allow_list -> global_default
    pub fn check(&self, resource: &str, action: &CapabilityAction) -> AccessDecision {
        // Mode check
        match self.mode {
            AccessMode::None => return AccessDecision::Denied("Plugin mode is 'none'".into()),
            AccessMode::All => { /* allow further checks */ }
            AccessMode::Trusted => { /* trusted mode - only explicit deny blocks */ }
            AccessMode::Interactive => { /* interactive mode needs approval */ }
        }

        // 1. Deny list (plugin-specific)
        if self.deny_list.matches(resource, action) {
            return AccessDecision::Denied("Denied by plugin deny list".into());
        }

        // 2. Global deny
        if self.global_deny.matches(resource, action) {
            return AccessDecision::Denied("Denied by global policy".into());
        }

        // 3. Allow list (plugin-specific)
        if self.allow_list.matches(resource, action) {
            return AccessDecision::Allowed("Allowed by plugin allow list".into());
        }

        // 4. Global default
        match self.global_default {
            AccessDefault::Allow => AccessDecision::Allowed("Allowed by default".into()),
            AccessDefault::Deny => AccessDecision::Denied("Denied by default".into()),
            AccessDefault::Ask => AccessDecision::NeedsApproval("Requires user approval".into()),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitySet {
    #[serde(default)]
    pub fs_paths: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub env_vars: Vec<String>,
    #[serde(default)]
    pub shell_commands: Vec<String>,
    #[serde(default)]
    pub config_keys: Vec<String>,
    #[serde(default)]
    pub providers: Vec<String>,
}

impl CapabilitySet {
    pub fn matches(&self, resource: &str, _action: &CapabilityAction) -> bool {
        self.tools.iter().any(|t| t == resource)
            || self.hosts.iter().any(|h| host_matches(resource, h))
            || self
                .fs_paths
                .iter()
                .any(|p| resource.starts_with(p.as_str()))
            || self.env_vars.iter().any(|e| e == resource)
            || self.shell_commands.iter().any(|c| c == resource)
            || self.config_keys.iter().any(|k| k == resource)
            || self.providers.iter().any(|p| p == resource)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AccessDefault {
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "ask")]
    Ask,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AccessMode {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "trusted")]
    Trusted,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "interactive")]
    Interactive,
}

#[derive(Debug, Clone)]
pub enum CapabilityAction {
    Read,
    Write,
    Execute,
    Network,
    Config,
    Session,
    Provider,
}

impl std::fmt::Display for CapabilityAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Execute => write!(f, "execute"),
            Self::Network => write!(f, "network"),
            Self::Config => write!(f, "config"),
            Self::Session => write!(f, "session"),
            Self::Provider => write!(f, "provider"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AccessDecision {
    Allowed(String),
    Denied(String),
    NeedsApproval(String),
}

/// Check if a resource (URL or hostname) matches a host pattern.
/// Uses proper hostname matching instead of simple substring containment,
/// so "evil.com" won't accidentally match "notevil.com".
fn host_matches(resource: &str, pattern: &str) -> bool {
    // Extract hostname from URL if the resource is a full URL
    let host = if let Some(after_protocol) = resource
        .strip_prefix("http://")
        .or_else(|| resource.strip_prefix("https://"))
    {
        after_protocol
            .split('/')
            .next()
            .unwrap_or(after_protocol)
            .split(':')
            .next()
            .unwrap_or(after_protocol) // strip port
    } else {
        resource
    };

    // Exact match
    if host == pattern {
        return true;
    }

    // Subdomain match: "example.com" matches "sub.example.com"
    if host.ends_with(&format!(".{pattern}")) {
        return true;
    }

    false
}
