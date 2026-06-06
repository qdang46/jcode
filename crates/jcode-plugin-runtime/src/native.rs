use jcode_plugin_core::PluginError;
use jcode_plugin_core::security::{AccessDecision, CapabilityAction, CapabilityChain};

pub struct NativeBindings;

impl NativeBindings {
    /// Check if a capability action is allowed, returning an error if denied.
    fn require_capability(
        chain: &CapabilityChain,
        resource: &str,
        action: &CapabilityAction,
    ) -> Result<(), PluginError> {
        match chain.check(resource, action) {
            AccessDecision::Allowed(_) => Ok(()),
            AccessDecision::Denied(reason) => Err(PluginError::Other(format!(
                "Capability denied for {action} on '{resource}': {reason}"
            ))),
            AccessDecision::NeedsApproval(reason) => {
                // In native bindings context, we cannot prompt for approval.
                // Log and deny — plugins should declare capabilities in their manifest.
                tracing::warn!(
                    "Capability needs approval for {action} on '{resource}': {reason} — denying (no interactive prompt available)"
                );
                Err(PluginError::Other(format!(
                    "Capability requires approval for {action} on '{resource}': {reason}"
                )))
            }
        }
    }

    pub async fn http_get(chain: &CapabilityChain, url: &str) -> Result<String, PluginError> {
        Self::require_capability(chain, url, &CapabilityAction::Network)?;
        let resp = reqwest::get(url)
            .await
            .map_err(|e| PluginError::Other(format!("HTTP GET failed: {e}")))?;
        let body = resp
            .text()
            .await
            .map_err(|e| PluginError::Other(format!("HTTP response error: {e}")))?;
        Ok(body)
    }

    pub async fn http_post(
        chain: &CapabilityChain,
        url: &str,
        body: &str,
    ) -> Result<String, PluginError> {
        Self::require_capability(chain, url, &CapabilityAction::Network)?;
        let client = reqwest::Client::new();
        let resp = client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| PluginError::Other(format!("HTTP POST failed: {e}")))?;
        let text = resp
            .text()
            .await
            .map_err(|e| PluginError::Other(format!("HTTP response error: {e}")))?;
        Ok(text)
    }

    pub async fn fs_read_text(chain: &CapabilityChain, path: &str) -> Result<String, PluginError> {
        Self::require_capability(chain, path, &CapabilityAction::Read)?;
        Ok(tokio::fs::read_to_string(path).await?)
    }

    pub async fn fs_write_text(
        chain: &CapabilityChain,
        path: &str,
        content: &str,
    ) -> Result<(), PluginError> {
        Self::require_capability(chain, path, &CapabilityAction::Write)?;
        Ok(tokio::fs::write(path, content).await?)
    }

    pub async fn fs_exists(chain: &CapabilityChain, path: &str) -> Result<bool, PluginError> {
        Self::require_capability(chain, path, &CapabilityAction::Read)?;
        Ok(std::path::Path::new(path).exists())
    }

    pub async fn fs_list(chain: &CapabilityChain, dir: &str) -> Result<Vec<String>, PluginError> {
        Self::require_capability(chain, dir, &CapabilityAction::Read)?;
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            entries.push(entry.file_name().to_string_lossy().to_string());
        }
        Ok(entries)
    }
}
