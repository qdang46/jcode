use std::sync::Arc;
use jcode_plugin_core::events::HandlerResult;

pub use jcode_plugin_core::preflight::{PreflightResult, StaticAnalysis};

pub enum HandlerSlot {
    Rust(Arc<dyn Fn(jcode_plugin_core::events::EventInput, Option<jcode_plugin_core::events::EventOutput>) -> std::pin::Pin<Box<dyn std::future::Future<Output = HandlerResult> + Send>> + Send + Sync>),
}

impl Clone for HandlerSlot {
    fn clone(&self) -> Self {
        match self {
            Self::Rust(f) => Self::Rust(Arc::clone(f)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedEntry {
    pub path: std::path::PathBuf,
    pub manifest: jcode_plugin_core::manifest::PluginManifest,
}
