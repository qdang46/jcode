use jcode_plugin_core::PluginError;

pub struct NativeBindings;

impl NativeBindings {
    pub async fn http_get(url: &str) -> Result<String, PluginError> {
        let resp = reqwest::get(url).await
            .map_err(|e| PluginError::Other(format!("HTTP GET failed: {e}")))?;
        let body = resp.text().await
            .map_err(|e| PluginError::Other(format!("HTTP response error: {e}")))?;
        Ok(body)
    }

    pub async fn http_post(url: &str, body: &str) -> Result<String, PluginError> {
        let client = reqwest::Client::new();
        let resp = client.post(url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| PluginError::Other(format!("HTTP POST failed: {e}")))?;
        let text = resp.text().await
            .map_err(|e| PluginError::Other(format!("HTTP response error: {e}")))?;
        Ok(text)
    }

    pub async fn fs_read_text(path: &str) -> Result<String, PluginError> {
        Ok(tokio::fs::read_to_string(path).await?)
    }

    pub async fn fs_write_text(path: &str, content: &str) -> Result<(), PluginError> {
        Ok(tokio::fs::write(path, content).await?)
    }

    pub async fn fs_exists(path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    pub async fn fs_list(dir: &str) -> Result<Vec<String>, PluginError> {
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            entries.push(entry.file_name().to_string_lossy().to_string());
        }
        Ok(entries)
    }
}
