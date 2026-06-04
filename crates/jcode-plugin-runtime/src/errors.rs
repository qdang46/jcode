use thiserror::Error;

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("QuickJS error: {0}")]
    QuickJs(String),

    #[error("Plugin error: {0}")]
    Plugin(#[from] jcode_plugin_core::PluginError),

    #[error("Timeout: {0:?}")]
    Timeout(std::time::Duration),

    #[error("Capability denied: {0}")]
    Capability(String),

    #[error("Transpilation error: {0}")]
    Transpile(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already registered: {0}")]
    AlreadyRegistered(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}
