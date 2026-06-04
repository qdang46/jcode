use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};

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

    pub fn install(&self, _ctx: &rquickjs::Ctx<'_>) -> Result<(), rquickjs::Error> {
        Ok(())
    }

    pub async fn dispatch_call(&self, method: &str, _args: &[u8]) -> Result<Vec<u8>, String> {
        match method {
            "getConfig" => Ok(br#"{}"#.to_vec()),
            "getVersion" => Ok(br#""0.1.0""#.to_vec()),
            _ => Err(format!("Unknown method: {method}")),
        }
    }
}
