use std::time::Duration;

pub struct PluginTimer;

impl PluginTimer {
    pub fn get_timeout(event: &str) -> Duration {
        match event {
            "PermissionRequest" | "PermissionDenied" => Duration::from_secs(3600),
            "SessionEnd" | "TurnEnd" | "PostCompact" | "AutoCompactionStart" => {
                Duration::from_millis(500)
            }
            _ => Duration::from_millis(5000),
        }
    }

    pub async fn with_timeout<T, F>(duration: Duration, future: F) -> Result<T, RuntimeTimeout>
    where
        F: std::future::Future<Output = T>,
    {
        tokio::time::timeout(duration, future)
            .await
            .map_err(|_| RuntimeTimeout(duration))
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeTimeout(pub Duration);

impl std::fmt::Display for RuntimeTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operation timed out after {:?}", self.0)
    }
}

impl std::error::Error for RuntimeTimeout {}
