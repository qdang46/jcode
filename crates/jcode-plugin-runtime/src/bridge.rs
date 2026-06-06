use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::sync::{Mutex, oneshot};

#[derive(Default)]
#[allow(dead_code)]
pub struct PromiseBridge {
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Vec<u8>>>>>,
}

impl PromiseBridge {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// TODO(WIP): Install promise bridge functions into the QuickJS context.
    /// Currently a no-op — the bridge between async Rust futures and JS Promises
    /// is not yet implemented. Requires injecting `__jcode_resolve(id, data)` and
    /// `__jcode_reject(id, error)` globals and wiring them to the oneshot channels.
    pub fn install(&self, _ctx: &rquickjs::Ctx<'_>) -> Result<(), rquickjs::Error> {
        Ok(())
    }

    /// TODO(WIP): Dispatch an async call from JS to Rust.
    /// Currently only handles hardcoded stub methods. Full implementation should
    /// allocate a oneshot channel, return the pending ID to JS as a Promise, and
    /// resolve/reject when the Rust future completes.
    pub async fn dispatch_call(&self, method: &str, _args: &[u8]) -> Result<Vec<u8>, String> {
        match method {
            "getConfig" => Ok(br#"{}"#.to_vec()),
            "getVersion" => Ok(br#""0.1.0""#.to_vec()),
            _ => Err(format!("Unknown method: {method}")),
        }
    }
}
