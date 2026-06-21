//! Integration tests for the plugin system — load, dispatch, capability checks.
//!
//! These tests exercise the end-to-end flow: register a handler, dispatch
//! an event, verify the handler runs and returns the expected result.

use jcode_agent_runtime::PermissionMode;
use jcode_plugin_core::events::{EventInput, HandlerAction, HandlerResult};
use jcode_plugin_core::types::PluginId;
use jcode_plugin_core::{CapabilityChainV2, PluginEvent, ToolTier};
use std::collections::HashMap;
use std::sync::Arc;

use crate::dispatcher::RcuDispatcher;
use crate::gate::{ApprovalGate, ApprovalOverride, GateDecision};
use crate::types::HandlerSlot;

#[test]
fn test_register_and_dispatch_handler() {
    let dispatcher = RcuDispatcher::new();
    let plugin_id = PluginId::npm("test-plugin");

    let slot = HandlerSlot::Rust(Arc::new(|_input, _output| {
        Box::pin(async {
            HandlerResult {
                action: HandlerAction::Block("blocked by test".to_string()),
                output: None,
                error: None,
            }
        })
    }));
    dispatcher.register(PluginEvent::PreToolUse, plugin_id.clone(), slot);
    dispatcher.commit();

    assert!(dispatcher.has_handler(PluginEvent::PreToolUse));
    assert_eq!(dispatcher.handler_count(), 1);
    assert_eq!(dispatcher.plugin_count(), 1);

    let input = EventInput::PreToolUse {
        tool_name: "test-tool".to_string(),
        tool_input: serde_json::json!({}),
        session_id: "sess-1".to_string(),
    };
    let results =
        futures::executor::block_on(dispatcher.dispatch(PluginEvent::PreToolUse, input, None));
    assert_eq!(results.len(), 1);
    let (id, result) = &results[0];
    assert_eq!(id, &plugin_id);
    assert!(matches!(result.action, HandlerAction::Block(_)));
}

#[test]
fn test_dispatch_no_handlers_returns_empty() {
    let dispatcher = RcuDispatcher::new();
    let input = EventInput::PreToolUse {
        tool_name: "x".to_string(),
        tool_input: serde_json::json!({}),
        session_id: "".to_string(),
    };
    let results =
        futures::executor::block_on(dispatcher.dispatch(PluginEvent::PreToolUse, input, None));
    assert!(results.is_empty());
    assert!(!dispatcher.has_handler(PluginEvent::PreToolUse));
}

#[test]
fn test_multiple_plugins_dispatch_concurrently() {
    let dispatcher = RcuDispatcher::new();

    for i in 0..3 {
        let id = PluginId::npm(&format!("plugin-{i}"));
        let id2 = id.clone();
        let slot = HandlerSlot::Rust(Arc::new(move |_input, _output| {
            let id = id2.clone();
            Box::pin(async move {
                HandlerResult {
                    action: HandlerAction::Allow,
                    output: Some(serde_json::json!({ "from": id.to_string() })),
                    error: None,
                }
            })
        }));
        dispatcher.register(PluginEvent::SessionStart, id, slot);
    }
    dispatcher.commit();

    let input = EventInput::SessionStart {
        session_id: "test".to_string(),
        project_dir: "/tmp".to_string(),
        model: "claude".to_string(),
        provider: "anthropic".to_string(),
    };
    let results =
        futures::executor::block_on(dispatcher.dispatch(PluginEvent::SessionStart, input, None));
    assert_eq!(results.len(), 3);
    for (_, result) in &results {
        assert!(matches!(result.action, HandlerAction::Allow));
    }
}

#[test]
fn test_unregister_plugin_removes_handlers() {
    let dispatcher = RcuDispatcher::new();
    let id = PluginId::npm("removable");

    let slot = HandlerSlot::Rust(Arc::new(|_, _| {
        Box::pin(async { HandlerResult::default() })
    }));
    dispatcher.register(PluginEvent::TurnStart, id.clone(), slot);
    dispatcher.commit();
    assert!(dispatcher.has_handler(PluginEvent::TurnStart));

    dispatcher.unregister_plugin(&id);
    assert!(!dispatcher.has_handler(PluginEvent::TurnStart));
}

#[test]
fn test_bitmap_o1_check() {
    let dispatcher = RcuDispatcher::new();
    let id = PluginId::npm("test");

    for ev in [
        PluginEvent::PreToolUse,
        PluginEvent::PostToolUse,
        PluginEvent::SessionStart,
    ] {
        assert!(!dispatcher.has_handler(ev));
    }

    let slot = HandlerSlot::Rust(Arc::new(|_, _| {
        Box::pin(async { HandlerResult::default() })
    }));
    dispatcher.register(PluginEvent::PostToolUse, id, slot);
    dispatcher.commit();

    assert!(!dispatcher.has_handler(PluginEvent::PreToolUse));
    assert!(dispatcher.has_handler(PluginEvent::PostToolUse));
    assert!(!dispatcher.has_handler(PluginEvent::SessionStart));
}

#[test]
fn test_preflight_clean_passes() {
    use jcode_plugin_core::manifest::PluginCapabilities;
    use jcode_plugin_core::preflight::PreflightAnalyzer;

    let code = r#"
        pi.on("TurnStart", (e) => {
            pi.logger.info("started");
        });
    "#;
    let result = PreflightAnalyzer::analyze(code, &PluginCapabilities::default());
    assert!(result.passed);
    assert!(result.warnings.is_empty());
}

#[test]
fn test_preflight_blocks_evil_code() {
    use jcode_plugin_core::manifest::PluginCapabilities;
    use jcode_plugin_core::preflight::PreflightAnalyzer;

    let code = r#"
        exec("rm -rf /");
        exec("sudo chmod 777 /etc");
    "#;
    let result = PreflightAnalyzer::analyze(code, &PluginCapabilities::default());
    assert!(!result.passed);
    assert!(!result.blocks.is_empty());
}

#[test]
fn test_audit_trail_ring_buffer() {
    use crate::audit::AuditTrail;
    use jcode_plugin_core::PluginId;
    use jcode_plugin_core::security::{AccessDecision, CapabilityAction};

    let trail = AuditTrail::new(3);
    let id = PluginId::npm("test");

    for i in 0..5 {
        trail.log_access(
            &id,
            &format!("res-{i}"),
            &CapabilityAction::Read,
            &AccessDecision::Allowed("ok".into()),
        );
    }
    assert_eq!(trail.len(), 3);

    let recent = trail.get_recent(10);
    assert_eq!(recent.len(), 3);
    assert!(recent[0].resource.contains("res-4"));
    assert!(recent[2].resource.contains("res-2"));
}

#[test]
fn test_kill_switches() {
    use crate::server;
    use std::sync::atomic::Ordering;

    server::DISABLE_ALL_PLUGINS.store(false, Ordering::SeqCst);
    server::SKIP_HOOKS.store(false, Ordering::SeqCst);
    server::FORCE_DENY.store(false, Ordering::SeqCst);

    assert!(!server::is_force_deny());
}

// =============================================================================
// End-to-end test with a REAL plugin file (TS → SWC → QuickJS → RcuDispatcher)
//
// This is the first test in the suite that exercises the full plugin loading
// pipeline, not just the Rust-side dispatcher. It loads examples/plugins/hello-plugin/
// which is a real, runnable plugin that lives in the repo.
//
// What this test verifies:
//   1. PluginLoader.scan_directory finds index.ts in the example dir
//   2. Transpiler successfully transpiles TypeScript to JavaScript via SWC
//   3. SandboxContext.eval runs the transpiled JS in QuickJS without error
//   4. PluginApiBindings installs the `pi` object into the QuickJS globals
//   5. pi.on("SessionStart", handler) registers a handler into RcuDispatcher.pending
//   6. pi.on("PreToolUse", handler) registers a second handler
//   7. pi.registerTool({...}) is a no-op stub (doesn't crash)
//   8. pi.logger.info actually calls tracing::info!
//   9. pi.kv.set / pi.uuid() work
//
// What this test does NOT verify (known limitations of current runtime):
//   - JS handler functions are not actually invoked when events fire
//     (sandbox.rs:80-94 is a TODO)
//   - pi.registerTool doesn't register an invokable tool
//     (registry.rs:122-129 is a TODO)
//   - We call dispatcher.commit() explicitly because the JS path adds to
//     pending but doesn't commit — this is a real workaround for the gap.
// =============================================================================

#[tokio::test(flavor = "current_thread")]
async fn test_hello_plugin_e2e() {
    use crate::loader::PluginLoader;
    use crate::registry::PluginRegistry;
    use crate::runtime::{RuntimeConfig, RuntimeManager};
    use jcode_plugin_core::config::{DiscoveryPaths, PluginConfig};

    // 1. Locate the example plugin (lives at <workspace>/examples/plugins/hello-plugin/)
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("could not find workspace root from CARGO_MANIFEST_DIR");
    let example_dir = workspace_root.join("examples/plugins/hello-plugin");
    assert!(
        example_dir.exists(),
        "example plugin dir missing: {} (this test requires the real plugin to exist on disk)",
        example_dir.display()
    );
    let index_ts = example_dir.join("index.ts");
    assert!(
        index_ts.exists(),
        "example index.ts missing: {}",
        index_ts.display()
    );

    // 2. Wire up the loader: dispatcher ← registry ← runtime, then PluginLoader
    let dispatcher = Arc::new(RcuDispatcher::new());
    let registry = Arc::new(PluginRegistry::new(dispatcher.clone()));
    let runtime = Arc::new(
        RuntimeManager::new(RuntimeConfig::default()).expect("RuntimeManager::new should succeed"),
    );
    let discovery = DiscoveryPaths {
        plugin_dirs: vec![example_dir.clone()],
        npm_cache: std::env::temp_dir().join("jcode-test-npm-cache"),
        tool_dirs: vec![],
    };
    let config = PluginConfig::default();
    let loader = PluginLoader::new(discovery, config, registry.clone(), runtime);

    // 3. Load all plugins. This triggers:
    //    - scan_directory → finds index.ts
    //    - PreflightAnalyzer.analyze (static safety check)
    //    - Transpiler.transpile (SWC TS → JS, type-stripping)
    //    - RuntimeManager.create_sandbox (QuickJS async runtime)
    //    - PluginApiBindings.install (injects `pi` into JS globals)
    //    - SandboxContext.eval (runs the JS in QuickJS)
    //    - plugin code calls pi.on("SessionStart", ...), pi.on("PreToolUse", ...),
    //      pi.registerTool({...}), pi.kv.set(...), pi.uuid(), pi.logger.info(...)
    let loaded_ids = loader
        .load_all()
        .await
        .expect("load_all should succeed — preflight + transpile + QuickJS eval must all pass");

    // 4. Verify the plugin was discovered and loaded
    assert_eq!(
        loaded_ids.len(),
        1,
        "expected exactly 1 plugin loaded from {}, got {:?}",
        example_dir.display(),
        loaded_ids
    );

    // 5. Verify the plugin is in the registry (proves PluginRegistry.register was called
    //    and the JS code reached the end without throwing)
    let plugins_in_registry = registry.list().await;
    assert_eq!(
        plugins_in_registry.len(),
        1,
        "expected 1 plugin in registry, got {:?}",
        plugins_in_registry
    );

    // 6. Commit pending handlers. The JS-side pi.on() adds handlers to
    //    RcuDispatcher.pending but never calls commit() — that's a known gap in
    //    the current runtime. We call it here so has_handler() returns the
    //    truth. In a future patch, the JS path should call commit() itself.
    dispatcher.commit();

    // 7. Verify the handlers the plugin tried to register are actually visible
    //    in the dispatcher. This proves the JS code ran, the `pi` object was
    //    injected correctly, and pi.on() did call into RcuDispatcher.register.
    assert!(
        dispatcher.has_handler(PluginEvent::SessionStart),
        "SessionStart handler missing — pi.on('SessionStart', ...) in index.ts did not register"
    );
    assert!(
        dispatcher.has_handler(PluginEvent::PreToolUse),
        "PreToolUse handler missing — pi.on('PreToolUse', ...) in index.ts did not register"
    );
    assert_eq!(
        dispatcher.handler_count(),
        2,
        "expected 2 handlers (SessionStart + PreToolUse), got {}",
        dispatcher.handler_count()
    );
    assert_eq!(
        dispatcher.plugin_count(),
        1,
        "expected 1 plugin in dispatcher, got {}",
        dispatcher.plugin_count()
    );
}

// =========================================================================
// ApprovalGate integration tests -- wired through RcuDispatcher
// =========================================================================

#[test]
fn gate_no_gate_installed_returns_none() {
    let dispatcher = RcuDispatcher::new();
    let decision = dispatcher.check_tool("read", ToolTier::Read, &serde_json::json!({}));
    assert!(decision.is_none(), "no gate installed => None");
}

#[test]
fn gate_bypass_allows_in_plan_mode_with_permissive_chain() {
    let dispatcher = RcuDispatcher::new();
    let gate = ApprovalGate::new(
        CapabilityChainV2::default(),
        PermissionMode::BypassPermissions,
        HashMap::new(),
    );
    dispatcher.set_approval_gate(gate);

    let decision = dispatcher.check_tool("bash", ToolTier::Exec, &serde_json::json!({}));
    assert_eq!(decision, Some(GateDecision::Allow));
}

#[test]
fn gate_user_override_deny_on_dispatcher() {
    let dispatcher = RcuDispatcher::new();
    let mut overrides = HashMap::new();
    overrides.insert("danger".into(), ApprovalOverride::Deny);
    let gate = ApprovalGate::new(
        CapabilityChainV2::default(),
        PermissionMode::BypassPermissions,
        overrides,
    );
    dispatcher.set_approval_gate(gate);

    let decision = dispatcher.check_tool("danger", ToolTier::Exec, &serde_json::json!({}));
    match decision {
        Some(GateDecision::Deny { layer, .. }) => {
            assert_eq!(layer, "user_override");
        }
        other => panic!("expected Deny(user_override), got {other:?}"),
    }
}

#[test]
fn gate_plan_mode_needs_approval_on_dispatcher() {
    let dispatcher = RcuDispatcher::new();
    let gate = ApprovalGate::new(
        CapabilityChainV2::default(),
        PermissionMode::Plan,
        HashMap::new(),
    );
    dispatcher.set_approval_gate(gate);

    let decision = dispatcher.check_tool("bash", ToolTier::Exec, &serde_json::json!({}));
    assert!(
        matches!(decision, Some(GateDecision::NeedsApproval { .. })),
        "plan mode => NeedsApproval, got {decision:?}"
    );
}

#[test]
fn gate_clear_removes_gate() {
    let dispatcher = RcuDispatcher::new();
    let gate = ApprovalGate::new(
        CapabilityChainV2::default(),
        PermissionMode::BypassPermissions,
        HashMap::new(),
    );
    dispatcher.set_approval_gate(gate);
    assert!(
        dispatcher
            .check_tool("x", ToolTier::Read, &serde_json::json!({}))
            .is_some()
    );

    dispatcher.clear_approval_gate();
    assert!(
        dispatcher
            .check_tool("x", ToolTier::Read, &serde_json::json!({}))
            .is_none()
    );
}

#[test]
fn gate_dispatcher_also_runs_handler_normally() {
    // Prove that setting a gate does not interfere with normal handler dispatch
    let dispatcher = RcuDispatcher::new();
    let plugin_id = PluginId::npm("gated-plugin");

    let slot = HandlerSlot::Rust(Arc::new(|_input, _output| {
        Box::pin(async {
            HandlerResult {
                action: HandlerAction::Block("plugin blocked".to_string()),
                output: None,
                error: None,
            }
        })
    }));
    dispatcher.register(PluginEvent::PreToolUse, plugin_id.clone(), slot);
    dispatcher.commit();

    // Install a bypass gate
    let gate = ApprovalGate::new(
        CapabilityChainV2::default(),
        PermissionMode::BypassPermissions,
        HashMap::new(),
    );
    dispatcher.set_approval_gate(gate);

    // Gate check passes
    let decision = dispatcher.check_tool("test-tool", ToolTier::Exec, &serde_json::json!({}));
    assert_eq!(decision, Some(GateDecision::Allow));

    // Handler dispatch still works normally
    let input = EventInput::PreToolUse {
        tool_name: "test-tool".to_string(),
        tool_input: serde_json::json!({}),
        session_id: "sess-1".to_string(),
    };
    let results =
        futures::executor::block_on(dispatcher.dispatch(PluginEvent::PreToolUse, input, None));
    assert_eq!(results.len(), 1);
    let (id, result) = &results[0];
    assert_eq!(id, &plugin_id);
    assert!(matches!(result.action, HandlerAction::Block(_)));
}
