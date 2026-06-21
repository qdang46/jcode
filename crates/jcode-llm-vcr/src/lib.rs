use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::info;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A recorded outbound HTTP request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Value,
}

/// A recorded HTTP response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// A single request/response pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub request: RecordedRequest,
    pub response: RecordedResponse,
}

/// A named collection of recorded interactions, stamped with the recording
/// date so consumers can tell how fresh the cassette is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cassette {
    pub name: String,
    pub recorded_at: String,
    pub interactions: Vec<Interaction>,
}

/// The operating mode of the VCR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VcrMode {
    /// Serve responses from the cassette. Errors if no match is found.
    Replay,
    /// Forward every request to the real endpoint and save the interaction.
    Record,
    /// Forward every request to the real endpoint without recording.
    Disabled,
}

// ---------------------------------------------------------------------------
// VCR Recorder
// ---------------------------------------------------------------------------

/// A VCR (Video Cassette Recorder) that intercepts HTTP requests and either
/// replays recorded responses or forwards them to the real server.
pub struct VcrRecorder {
    pub mode: VcrMode,
    pub path: PathBuf,
    cassette: RwLock<Cassette>,
}

impl VcrRecorder {
    /// Load (or create) a cassette from `path` and set the operating `mode`.
    ///
    /// If the file already exists it is deserialised; otherwise an empty
    /// cassette is created with the file stem as its name.
    pub fn load(path: impl Into<PathBuf>, mode: VcrMode) -> Result<Self> {
        let path: PathBuf = path.into();
        let cassette = if path.exists() {
            let file = std::fs::File::open(&path)
                .with_context(|| format!("open cassette at {}", path.display()))?;
            let reader = std::io::BufReader::new(file);
            serde_json::from_reader(reader)
                .with_context(|| format!("parse cassette at {}", path.display()))?
        } else {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("cassette")
                .to_string();
            Cassette {
                name,
                recorded_at: Utc::now().to_rfc3339(),
                interactions: Vec::new(),
            }
        };

        info!(
            "VCR loaded: mode={mode:?}, path={}, interactions={}",
            path.display(),
            cassette.interactions.len()
        );

        Ok(Self {
            mode,
            path,
            cassette: RwLock::new(cassette),
        })
    }

    /// Replay a recorded response, record a new one, or pass through
    /// depending on `self.mode`.
    pub async fn record_or_replay(&self, request: &RecordedRequest) -> Result<RecordedResponse> {
        match self.mode {
            VcrMode::Replay => self.replay(request),
            VcrMode::Record => self.record(request).await,
            VcrMode::Disabled => self.passthrough(request).await,
        }
    }

    /// Write the current cassette to disk as pretty-printed JSON.
    pub fn flush(&self) -> Result<()> {
        let cassette = self
            .cassette
            .read()
            .map_err(|e| anyhow::anyhow!("RwLock poisoned: {e}"))?;

        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let file = std::fs::File::create(&self.path)
            .with_context(|| format!("create cassette at {}", self.path.display()))?;
        let writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &*cassette)
            .with_context(|| format!("write cassette at {}", self.path.display()))?;

        info!(
            "VCR flushed: {} interactions to {}",
            cassette.interactions.len(),
            self.path.display()
        );
        Ok(())
    }

    // ---- internal helpers -------------------------------------------------

    /// Look up the request in the cassette and return the first match.
    fn replay(&self, request: &RecordedRequest) -> Result<RecordedResponse> {
        let cassette = self
            .cassette
            .read()
            .map_err(|e| anyhow::anyhow!("RwLock poisoned: {e}"))?;

        for interaction in &cassette.interactions {
            if interaction.request.method == request.method
                && interaction.request.url == request.url
                && interaction.request.body == request.body
            {
                info!("VCR replay hit: {} {}", request.method, request.url);
                return Ok(interaction.response.clone());
            }
        }

        anyhow::bail!(
            "VCR replay miss: no recording for {} {} (body: {:?})",
            request.method,
            request.url,
            request.body,
        );
    }

    /// Forward the request to the real endpoint, save the interaction, and
    /// return the response.
    async fn record(&self, request: &RecordedRequest) -> Result<RecordedResponse> {
        let response = self.passthrough(request).await?;
        let interaction = Interaction {
            request: request.clone(),
            response: response.clone(),
        };

        {
            let mut cassette = self
                .cassette
                .write()
                .map_err(|e| anyhow::anyhow!("RwLock poisoned: {e}"))?;
            cassette.interactions.push(interaction);
        }

        info!("VCR recorded: {} {}", request.method, request.url);
        Ok(response)
    }

    /// Execute the real HTTP call.
    async fn passthrough(&self, request: &RecordedRequest) -> Result<RecordedResponse> {
        let client = Client::new();
        let method = request.method.to_uppercase();

        let mut builder = match method.as_str() {
            "GET" => client.get(&request.url),
            "POST" => client.post(&request.url),
            "PUT" => client.put(&request.url),
            "DELETE" => client.delete(&request.url),
            "PATCH" => client.patch(&request.url),
            "HEAD" => client.head(&request.url),
            m => anyhow::bail!("unsupported HTTP method: {m}"),
        };

        for (k, v) in &request.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }

        if request.body != Value::Null {
            let body_str = serde_json::to_string(&request.body)?;
            builder = builder.body(body_str);
        }

        let resp = builder.send().await?;
        let status = resp.status().as_u16();

        let headers: HashMap<String, String> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
            .collect();

        let body = resp.bytes().await?.to_vec();

        Ok(RecordedResponse {
            status,
            headers,
            body,
        })
    }
}

// ---- version helper (kept from the placeholder stubs) ---------------------

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: write a cassette with one interaction to `path`.
    fn seed_cassette(path: &std::path::Path) {
        let response = RecordedResponse {
            status: 200,
            headers: [("content-type".into(), "application/json".into())].into(),
            body: br#"{"ok":true}"#.to_vec(),
        };
        let request = RecordedRequest {
            method: "POST".into(),
            url: "http://example.com/api/test".into(),
            headers: [("accept".into(), "application/json".into())].into(),
            body: serde_json::json!({"query": "hello"}),
        };
        let cassette = Cassette {
            name: "test-cassette".into(),
            recorded_at: "2025-06-01T00:00:00Z".into(),
            interactions: vec![Interaction { request, response }],
        };
        let json = serde_json::to_string_pretty(&cassette).unwrap();
        std::fs::write(path, json).unwrap();
    }

    // -----------------------------------------------------------------------
    // Replay mode
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn replay_matching_request_returns_recorded_response() {
        let dir = std::env::temp_dir().join("vcr_test_replay_match");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cassette.json");
        seed_cassette(&path);

        let recorder = VcrRecorder::load(&path, VcrMode::Replay).unwrap();

        let req = RecordedRequest {
            method: "POST".into(),
            url: "http://example.com/api/test".into(),
            headers: [("accept".into(), "application/json".into())].into(),
            body: serde_json::json!({"query": "hello"}),
        };

        let resp = recorder.record_or_replay(&req).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(String::from_utf8_lossy(&resp.body), r#"{"ok":true}"#);
        assert_eq!(
            resp.headers.get("content-type").map(|s| s.as_str()),
            Some("application/json")
        );

        // cleanup
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn replay_no_match_method_returns_error() {
        let dir = std::env::temp_dir().join("vcr_test_no_match_method");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cassette.json");
        seed_cassette(&path);

        let recorder = VcrRecorder::load(&path, VcrMode::Replay).unwrap();

        // Same URL and body, but different method
        let req = RecordedRequest {
            method: "GET".into(), // cassette has POST
            url: "http://example.com/api/test".into(),
            headers: HashMap::new(),
            body: serde_json::json!({"query": "hello"}),
        };

        let err = recorder.record_or_replay(&req).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no recording"),
            "expected 'no recording' error, got: {msg}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn replay_no_match_url_returns_error() {
        let dir = std::env::temp_dir().join("vcr_test_no_match_url");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cassette.json");
        seed_cassette(&path);

        let recorder = VcrRecorder::load(&path, VcrMode::Replay).unwrap();

        let req = RecordedRequest {
            method: "POST".into(),
            url: "http://example.com/api/other".into(), // cassette has /api/test
            headers: HashMap::new(),
            body: serde_json::json!({"query": "hello"}),
        };

        let err = recorder.record_or_replay(&req).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no recording"),
            "expected 'no recording' error, got: {msg}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn replay_no_match_body_returns_error() {
        let dir = std::env::temp_dir().join("vcr_test_no_match_body");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cassette.json");
        seed_cassette(&path);

        let recorder = VcrRecorder::load(&path, VcrMode::Replay).unwrap();

        let req = RecordedRequest {
            method: "POST".into(),
            url: "http://example.com/api/test".into(),
            headers: HashMap::new(),
            body: serde_json::json!({"query": "world"}), // cassette has "hello"
        };

        let err = recorder.record_or_replay(&req).await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no recording"),
            "expected 'no recording' error, got: {msg}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // -----------------------------------------------------------------------
    // Disabled mode  (passthrough – expects real network error)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn disabled_mode_attempts_real_request() {
        let dir = std::env::temp_dir().join("vcr_test_disabled");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("cassette.json");

        // Create a cassette with data – disabled mode should NOT use it
        seed_cassette(&path);
        let recorder = VcrRecorder::load(&path, VcrMode::Disabled).unwrap();

        let req = RecordedRequest {
            method: "GET".into(),
            url: "http://127.0.0.1:1/vcr-test".into(),
            headers: HashMap::new(),
            body: Value::Null,
        };

        let err = recorder.record_or_replay(&req).await.unwrap_err();
        let msg = format!("{err:#}");

        // Must NOT be a replay-miss error – should be a real connection error
        assert!(
            !msg.contains("no recording"),
            "Disabled mode should not replay; got replay error: {msg}"
        );
        // Should mention something connection-related
        assert!(
            msg.contains("error") || msg.contains("refused") || msg.contains("Connection"),
            "expected a network-level error in disabled mode, got: {msg}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // -----------------------------------------------------------------------
    // Flush  (round-trip through a fresh file)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn flush_writes_cassette_and_can_be_reloaded() {
        let dir = std::env::temp_dir().join("vcr_test_flush");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("flush_test.json");

        // Start empty in Record mode so we can add an interaction without
        // actually making a network call.
        let recorder = VcrRecorder::load(&path, VcrMode::Replay).unwrap();

        // Manually insert an interaction to simulate what `record` would do.
        {
            let mut cass = recorder.cassette.write().unwrap();
            cass.interactions.push(Interaction {
                request: RecordedRequest {
                    method: "GET".into(),
                    url: "http://example.com/flush".into(),
                    headers: HashMap::new(),
                    body: Value::Null,
                },
                response: RecordedResponse {
                    status: 200,
                    headers: HashMap::new(),
                    body: b"flushed".to_vec(),
                },
            });
        }

        recorder.flush().unwrap();

        // Reload the same file and verify the interaction survived
        let reloaded = VcrRecorder::load(&path, VcrMode::Replay).unwrap();
        let cass = reloaded.cassette.read().unwrap();
        assert_eq!(cass.interactions.len(), 1);
        assert_eq!(cass.interactions[0].request.url, "http://example.com/flush");
        assert_eq!(cass.interactions[0].response.status, 200);

        std::fs::remove_dir_all(&dir).ok();
    }

    // -----------------------------------------------------------------------
    // Version test (from original placeholder)
    // -----------------------------------------------------------------------

    #[test]
    fn test_version() {
        assert!(!version().is_empty());
    }
}
