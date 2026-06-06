use crate::sandbox::SandboxContext;
use jcode_plugin_core::PluginError;
use jcode_plugin_core::manifest::PluginManifest;
use jcode_plugin_core::types::PluginId;
use rquickjs::AsyncRuntime;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

pub struct RuntimeConfig {
    pub max_concurrent: usize,
    pub max_runtimes: usize,
    pub max_stack_size: usize,
    pub memory_limit: usize,
    pub gc_threshold: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            max_runtimes: 8,
            max_stack_size: 512 * 1024,
            memory_limit: 50 * 1024 * 1024,
            gc_threshold: 10 * 1024 * 1024,
        }
    }
}

pub struct RuntimeManager {
    #[allow(dead_code)]
    main_runtime: Arc<AsyncRuntime>,
    pool: Arc<Mutex<RuntimePool>>,
    _semaphore: Arc<Semaphore>,
    config: RuntimeConfig,
}

struct RuntimePool {
    available: Vec<AsyncRuntime>,
    max_runtimes: usize,
}

impl RuntimeManager {
    pub fn new(config: RuntimeConfig) -> Result<Self, PluginError> {
        let rt = AsyncRuntime::new().map_err(|e| PluginError::Runtime(e.to_string()))?;
        drop(rt.set_max_stack_size(config.max_stack_size));
        drop(rt.set_gc_threshold(config.gc_threshold));
        drop(rt.set_memory_limit(config.memory_limit));
        Ok(Self {
            main_runtime: Arc::new(rt),
            pool: Arc::new(Mutex::new(RuntimePool {
                available: Vec::new(),
                max_runtimes: config.max_runtimes,
            })),
            _semaphore: Arc::new(Semaphore::new(config.max_concurrent)),
            config,
        })
    }

    pub fn create_sandbox(
        &self,
        _id: PluginId,
        _manifest: PluginManifest,
    ) -> Result<SandboxContext, PluginError> {
        let runtime = self.acquire_runtime()?;
        SandboxContext::new(_id, _manifest, runtime)
    }

    fn acquire_runtime(&self) -> Result<AsyncRuntime, PluginError> {
        if let Ok(mut pool) = self.pool.try_lock()
            && let Some(rt) = pool.available.pop()
        {
            return Ok(rt);
        }
        AsyncRuntime::new().map_err(|e| PluginError::Runtime(e.to_string()))
    }

    pub fn release(&self, runtime: AsyncRuntime) {
        if let Ok(mut pool) = self.pool.try_lock()
            && pool.available.len() < pool.max_runtimes
        {
            pool.available.push(runtime);
        }
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }
}
