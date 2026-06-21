use jcode_plugin_core::PluginError;
use jcode_plugin_core::events::{EventInput, EventOutput, HandlerResult, PluginEvent};
use jcode_plugin_core::manifest::PluginManifest;
use jcode_plugin_core::security::CapabilityChain;
use jcode_plugin_core::types::PluginId;
use rquickjs::{AsyncContext, AsyncRuntime};
use std::sync::Arc;
use std::time::Duration;

use crate::api::PluginApiBindings;
use crate::bridge::PromiseBridge;
use crate::registry::PluginRegistry;

#[derive(Debug, Clone)]
pub struct DualTimeout {
    pub info: Duration,
    pub actionable: Duration,
    pub permission: Option<Duration>,
}

impl Default for DualTimeout {
    fn default() -> Self {
        Self {
            info: Duration::from_millis(500),
            actionable: Duration::from_millis(5000),
            permission: None,
        }
    }
}

pub struct SandboxContext {
    runtime: AsyncRuntime,
    _id: PluginId,
    _manifest: PluginManifest,
    #[allow(dead_code)]
    capability_chain: Arc<CapabilityChain>,
    timeout: DualTimeout,
}

impl SandboxContext {
    pub fn new(
        id: PluginId,
        manifest: PluginManifest,
        runtime: AsyncRuntime,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            runtime,
            _id: id,
            _manifest: manifest,
            capability_chain: Arc::new(CapabilityChain::default()),
            timeout: DualTimeout::default(),
        })
    }

    pub async fn eval(&self, code: &str) -> Result<(), PluginError> {
        let ctx = AsyncContext::full(&self.runtime)
            .await
            .map_err(|e| PluginError::Runtime(format!("Failed to create context: {e}")))?;

        ctx.with(|ctx| {
            ctx.eval::<(), _>(code)
                .map_err(|e| PluginError::Eval(e.to_string()))
        })
        .await
        .map_err(|e| PluginError::Eval(e.to_string()))?;

        Ok(())
    }

    /// Evaluate plugin JavaScript code in a QuickJS context with the `pi` API
    /// injected. This is the method used by PluginLoader to make `pi.on`,
    /// `pi.registerTool`, `pi.logger`, etc. available to plugin code.
    ///
    /// Previously, the loader called `eval()` without setting up the `pi` object,
    /// causing ReferenceErrors on `pi.xxx` calls. The `pi` global is set via
    /// PluginApiBindings::install, which creates the `__jcode_pi` global and
    /// injects all the JS-accessible functions (on, registerTool, logger, kv, etc.).
    /// See crates/jcode-plugin-runtime/src/api.rs for the full API surface.
    ///
    /// This method replaces the pattern of manually creating PluginApiBindings
    /// in the loader: the loader now passes a `registry` ref and the bindings
    /// are wired here.
    pub async fn eval_with_pi(
        &self,
        code: &str,
        registry: Arc<PluginRegistry>,
    ) -> Result<(), PluginError> {
        let ctx = AsyncContext::full(&self.runtime)
            .await
            .map_err(|e| PluginError::Runtime(format!("Failed to create QuickJS context: {e}")))?;

        let id = self._id.clone();
        let manifest = self._manifest.clone();
        let chain = self.capability_chain.clone();

        ctx.with(|ctx| {
            // Step 1: install the `pi` API into the global scope.
            let bridge = PromiseBridge::new();
            let api = PluginApiBindings::new(id, manifest, chain, registry, Arc::new(bridge));
            api.install(&ctx)
                .map_err(|e| PluginError::Eval(format!("api install: {e:?}")))?;

            // Step 2: evaluate the plugin code.
            ctx.eval::<rquickjs::Value, _>(code)
                .map_err(|e| PluginError::Eval(format!("eval: {e:?}")))
                .map(|_| ())
        })
        .await
        .map_err(|e| PluginError::Eval(format!("outer: {e:?}")))?;

        Ok(())
    }

    pub async fn call_handler(
        &self,
        event: PluginEvent,
        input: EventInput,
        output: Option<EventOutput>,
    ) -> Result<HandlerResult, PluginError> {
        let timeout = self.get_timeout(event);
        match tokio::time::timeout(timeout, self.call_inner(event, input, output)).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(PluginError::Timeout(timeout)),
        }
    }

    /// TODO(WIP): Invoke the actual JS handler for this event.
    /// Currently returns a default result. Full implementation should:
    ///   1. Serialize EventInput to JSON
    ///   2. Call the stored JS function reference via QuickJS context
    ///   3. Deserialize the JS return value into HandlerResult
    ///
    /// This is blocked on storing JS function references across the Rust boundary.
    async fn call_inner(
        &self,
        _event: PluginEvent,
        _input: EventInput,
        _output: Option<EventOutput>,
    ) -> Result<HandlerResult, PluginError> {
        Ok(HandlerResult::default())
    }

    fn get_timeout(&self, event: PluginEvent) -> Duration {
        match event {
            PluginEvent::PermissionRequest | PluginEvent::PermissionDenied => {
                self.timeout.permission.unwrap_or(Duration::from_secs(3600))
            }
            PluginEvent::SessionEnd
            | PluginEvent::TurnEnd
            | PluginEvent::PostCompact
            | PluginEvent::AutoCompactionStart => self.timeout.info,
            _ => self.timeout.actionable,
        }
    }
}
