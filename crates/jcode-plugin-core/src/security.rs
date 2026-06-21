use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyMode {
    /// Deny by default
    Strict,
    /// Allow by default
    Permissive,
    /// Prompt for ambiguous
    #[default]
    Prompt,
    /// Kill switch — deny everything
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecisionV2 {
    Allow { reason: String, layer: u8 },
    Deny { reason: String, layer: u8 },
    NeedsApproval { reason: String, layer: u8 },
}

/// 5-layer capability chain. Adapted from pi-agent-rust's ExtensionPolicy.
/// Extends the existing 4-layer chain with a mode fallback layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityChainV2 {
    pub plugin_deny: CapabilitySet,
    pub global_deny: CapabilitySet,
    pub plugin_allow: CapabilitySet,
    pub global_allow: CapabilitySet,
    pub mode: PolicyMode,
    pub global_default: Option<AccessDefault>,
}

impl CapabilityChainV2 {
    /// Check if a resource access is allowed. Returns AccessDecisionV2.
    /// Evaluation order (5 layers):
    ///   1. plugin_deny    — plugin-specific deny list
    ///   2. global_deny    — global deny policy
    ///   3. plugin_allow   — plugin-specific allow list
    ///   4. global_allow   — global allow list
    ///   5. mode fallback  — PolicyMode / global_default
    pub fn check(&self, resource: &str, action: &CapabilityAction) -> AccessDecisionV2 {
        // Layer 1: plugin_deny
        if self.plugin_deny.matches(resource, action) {
            return AccessDecisionV2::Deny {
                reason: "denied by plugin deny list".into(),
                layer: 1,
            };
        }
        // Layer 2: global_deny
        if self.global_deny.matches(resource, action) {
            return AccessDecisionV2::Deny {
                reason: "denied by global policy".into(),
                layer: 2,
            };
        }
        // Layer 3: plugin_allow
        if self.plugin_allow.matches(resource, action) {
            return AccessDecisionV2::Allow {
                reason: "allowed by plugin allow list".into(),
                layer: 3,
            };
        }
        // Layer 4: global_allow
        if self.global_allow.matches(resource, action) {
            return AccessDecisionV2::Allow {
                reason: "allowed by global allow list".into(),
                layer: 4,
            };
        }
        // Layer 5: mode fallback
        match (self.mode, self.global_default) {
            (PolicyMode::Disabled, _) => AccessDecisionV2::Deny {
                reason: "disabled (kill switch)".into(),
                layer: 5,
            },
            (_, Some(AccessDefault::Deny)) => AccessDecisionV2::Deny {
                reason: "denied by default".into(),
                layer: 5,
            },
            (_, Some(AccessDefault::Allow)) => AccessDecisionV2::Allow {
                reason: "allowed by default".into(),
                layer: 5,
            },
            (_, Some(AccessDefault::Ask)) => AccessDecisionV2::NeedsApproval {
                reason: "requires approval".into(),
                layer: 5,
            },
            (PolicyMode::Strict, _) => AccessDecisionV2::Deny {
                reason: "strict mode".into(),
                layer: 5,
            },
            (PolicyMode::Permissive, _) => AccessDecisionV2::Allow {
                reason: "permissive mode".into(),
                layer: 5,
            },
            (PolicyMode::Prompt, _) => AccessDecisionV2::NeedsApproval {
                reason: "prompt mode".into(),
                layer: 5,
            },
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
