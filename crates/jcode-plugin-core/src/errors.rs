use thiserror::Error;

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Plugin manifest is invalid: {0}")]
    InvalidManifest(String),

    #[error("Plugin not found: {0}")]
    NotFound(String),

    #[error("Failed to load plugin: {0}")]
    Load(String),

    #[error("Plugin runtime error: {0}")]
    Runtime(String),

    #[error("QuickJS evaluation error: {0}")]
    Eval(String),

    #[error("QuickJS runtime error: {0}")]
    QuickJs(String),

    #[error("SWC transpilation error: {0}")]
    Transpile(String),

    #[error("Plugin operation timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("Capability denied: {0}")]
    Capability(String),

    #[error("npm error: {0}")]
    Npm(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
