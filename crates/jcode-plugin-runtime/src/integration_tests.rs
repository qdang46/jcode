//! Integration tests for the plugin system — load, dispatch, capability checks.
//!
//! These tests exercise the end-to-end flow: register a handler, dispatch
//! an event, verify the handler runs and returns the expected result.

use jcode_plugin_core::PluginEvent;
use jcode_plugin_core::events::{EventInput, HandlerResult, HandlerAction};
use jcode_plugin_core::types::PluginId;
use std::sync::Arc;

use crate::dispatcher::RcuDispatcher;
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
    let results = futures::executor::block_on(
        dispatcher.dispatch(PluginEvent::PreToolUse, input, None)
    );
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
    let results = futures::executor::block_on(
        dispatcher.dispatch(PluginEvent::PreToolUse, input, None)
    );
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
    let results = futures::executor::block_on(
        dispatcher.dispatch(PluginEvent::SessionStart, input, None)
    );
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

    for ev in [PluginEvent::PreToolUse, PluginEvent::PostToolUse, PluginEvent::SessionStart] {
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
    use jcode_plugin_core::preflight::PreflightAnalyzer;
    use jcode_plugin_core::manifest::PluginCapabilities;

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
    use jcode_plugin_core::preflight::PreflightAnalyzer;
    use jcode_plugin_core::manifest::PluginCapabilities;

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
    use jcode_plugin_core::security::{CapabilityAction, AccessDecision};

    let trail = AuditTrail::new(3);
    let id = PluginId::npm("test");

    for i in 0..5 {
        trail.log_access(&id, &format!("res-{i}"), &CapabilityAction::Read, &AccessDecision::Allowed("ok".into()));
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
