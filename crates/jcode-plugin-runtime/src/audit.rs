use chrono::{DateTime, Utc};
use jcode_plugin_core::security::{AccessDecision, CapabilityAction};
use jcode_plugin_core::types::PluginId;
use std::collections::VecDeque;
use std::sync::Mutex;

pub struct AuditTrail {
    entries: Mutex<VecDeque<AuditEntry>>,
    max_entries: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub plugin_id: String,
    pub resource: String,
    pub action: String,
    pub decision: String,
    pub reason: String,
}

impl AuditTrail {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(max_entries)),
            max_entries,
        }
    }

    pub fn log_access(
        &self,
        plugin_id: &PluginId,
        resource: &str,
        action: &CapabilityAction,
        decision: &AccessDecision,
    ) {
        let (ds, reason) = match decision {
            AccessDecision::Allowed(r) => ("allowed", r.clone()),
            AccessDecision::Denied(r) => ("denied", r.clone()),
            AccessDecision::NeedsApproval(r) => ("needs_approval", r.clone()),
        };

        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= self.max_entries {
                entries.pop_front();
            }
            entries.push_back(AuditEntry {
                timestamp: Utc::now(),
                plugin_id: plugin_id.to_string(),
                resource: resource.into(),
                action: format!("{action}"),
                decision: ds.into(),
                reason,
            });
        }
    }

    pub fn get_recent(&self, count: usize) -> Vec<AuditEntry> {
        if let Ok(entries) = self.entries.lock() {
            entries.iter().rev().take(count).cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }

    pub fn entry_count(&self) -> usize {
        self.len()
    }

    pub fn len(&self) -> usize {
        if let Ok(entries) = self.entries.lock() {
            entries.len()
        } else {
            0
        }
    }
}
