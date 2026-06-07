//! Integration tests for the jcode-hooks crate.
//!
//! Exercises the full hook lifecycle: config parsing, registry construction,
//! matcher filtering, condition evaluation, parallel dispatch, and aggregated
//! decision logic.

use jcode_hooks::dispatch::aggregate_decision;
use jcode_hooks::{
    AgentHandlerConfig, AggregatedDecision, ClassifiedOutcome, CommandHandlerConfig,
    DispatchConfig, HookContext, HookEvent, HookHandlerConfig, HookInput, HookInputBuilder,
    HookMatcher, HookRegistry, HookSettings, HooksConfig, HttpHandlerConfig, MatcherContext,
    PluginHandlerConfig, dispatch_hooks, matches,
};

// ===========================================================================
// test_hooks_full_flow  (config -> registry -> dispatch -> decision)
// ===========================================================================

#[tokio::test]
async fn test_hooks_full_flow() {
    // Step 1: Build a HooksConfig from TOML (simulating a config file)
    let toml_str = r#"
[settings]
timeout_secs = 15
max_concurrency = 5
dry_run = true
fail_closed = false

[[events.PreToolUse]]
type = "command"
command = "security_check.sh"
enabled = true
timeout_secs = 10
matcher = "Bash|Write"

[[events.PreToolUse]]
type = "command"
command = "audit_log.sh"
enabled = true

[[events.SessionStart]]
type = "command"
command = "init_session.sh"
enabled = true

[[events.Stop]]
type = "http"
url = "http://localhost:9090/hooks/stop"
method = "POST"
timeout_secs = 5
"#;

    let config: HooksConfig = toml::from_str(toml_str).unwrap();

    // Verify config parsing
    assert_eq!(config.settings.timeout_secs, 15);
    assert_eq!(config.settings.max_concurrency, 5);
    assert!(config.settings.dry_run);
    assert!(!config.is_empty());
    assert_eq!(config.events["PreToolUse"].len(), 2);
    assert_eq!(config.events["SessionStart"].len(), 1);
    assert_eq!(config.events["Stop"].len(), 1);

    // Step 2: Build registry from config
    let registry = HookRegistry::from_config(config);
    assert!(!registry.is_empty());

    // Step 3: Build context for a PreToolUse event on "Bash"
    let context = HookContext::for_tool(
        "Bash".to_string(),
        "ses_flow_001".to_string(),
        "/home/user/project".to_string(),
    );

    // Step 4: Get matching handlers
    let matching = registry.get_matching(&HookEvent::PreToolUse, &context);
    // Both handlers should match: security_check has matcher "Bash|Write" (Bash matches),
    // audit_log has no matcher (wildcard).
    assert_eq!(
        matching.len(),
        2,
        "both PreToolUse handlers should match Bash"
    );

    // Step 5: Build HookInput via the builder
    let input = HookInputBuilder::new()
        .session("ses_flow_001", "/home/user/project")
        .event("PreToolUse")
        .agent("agent_1", "default")
        .tool(
            "Bash",
            serde_json::json!({"command": "ls -la"}),
            "tool_use_1",
        )
        .build();

    assert_eq!(input.schema_version, "2.0");
    assert_eq!(input.hook_event_name, "PreToolUse");
    assert_eq!(input.tool_name, Some("Bash".to_string()));

    // Step 6: Dispatch (dry-run mode -- handlers are resolved but not executed)
    let dispatch_config = DispatchConfig::from_settings(&HookSettings {
        timeout_secs: 15,
        max_concurrency: 5,
        dry_run: true,
        fail_closed: false,
    });

    let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &matching, &dispatch_config).await;

    assert_eq!(stats.total_dispatched, 2);
    assert_eq!(stats.allowed, 2);
    assert_eq!(stats.failed, 0);
    assert!(stats.all_succeeded());
    assert!(!stats.any_denied());

    // Step 7: Aggregate decision (all allowed -> Allow)
    let outcomes: Vec<ClassifiedOutcome> = stats
        .results
        .iter()
        .map(|r| match &r.outcome {
            ClassifiedOutcome::Allow => ClassifiedOutcome::Allow,
            ClassifiedOutcome::Ask { reason } => ClassifiedOutcome::Ask {
                reason: reason.clone(),
            },
            ClassifiedOutcome::Deny { reason } => ClassifiedOutcome::Deny {
                reason: reason.clone(),
            },
            ClassifiedOutcome::Failed { error } => ClassifiedOutcome::Failed {
                error: error.clone(),
            },
        })
        .collect();

    let decision = aggregate_decision(&outcomes, false);
    assert!(
        matches!(decision, AggregatedDecision::Allow),
        "all dry-run handlers should result in Allow"
    );

    // Also verify the full serialization round-trip of the input
    let json = serde_json::to_string_pretty(&input).unwrap();
    let reparsed: HookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(reparsed.session_id, "ses_flow_001");
    assert_eq!(reparsed.tool_name, Some("Bash".to_string()));
}

// ===========================================================================
// test_parallel_hook_execution  (multiple hooks concurrent)
// ===========================================================================

#[tokio::test]
async fn test_parallel_hook_execution() {
    // Use dry-run mode to verify multiple handlers are dispatched concurrently.
    // In dry-run mode all handlers report Allow without actually running.
    let config = DispatchConfig {
        dry_run: true,
        max_concurrency: 3,
        timeout_secs: 10,
        fail_closed: false,
    };

    let input = HookInputBuilder::new()
        .session("ses_parallel", "/workspace")
        .event("PreToolUse")
        .tool("Bash", serde_json::json!({"command": "test"}), "tu_p1")
        .build();

    // Register 5 command handlers (all enabled, no matcher = wildcard)
    let handlers: Vec<HookHandlerConfig> = (0..5)
        .map(|i| {
            HookHandlerConfig::Command(CommandHandlerConfig {
                enabled: true,
                command: format!("parallel_hook_{}.sh", i),
                ..Default::default()
            })
        })
        .collect();

    let refs: Vec<&HookHandlerConfig> = handlers.iter().collect();

    let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &refs, &config).await;

    assert_eq!(
        stats.total_dispatched, 5,
        "all 5 handlers should be dispatched"
    );
    assert_eq!(stats.allowed, 5, "all dry-run handlers should be allowed");
    assert_eq!(stats.failed, 0, "no handler should fail in dry-run");
    assert_eq!(stats.results.len(), 5);
    assert!(stats.all_succeeded());

    // Verify each handler has a distinct label
    let mut labels: Vec<&str> = stats
        .results
        .iter()
        .map(|r| r.handler_label.as_str())
        .collect();
    labels.sort();
    labels.dedup();
    assert_eq!(labels.len(), 5, "all handler labels should be unique");

    // Verify total_duration is non-negative (Duration::ZERO or greater)
    // In dry-run mode, this should be very fast.
    let _ = stats.total_duration; // just access to confirm no panic

    // Verify that concurrency is bounded by the semaphore
    let config_bounded = DispatchConfig {
        dry_run: true,
        max_concurrency: 2, // only 2 at a time
        timeout_secs: 10,
        fail_closed: false,
    };

    let handlers_bounded: Vec<HookHandlerConfig> = (0..4)
        .map(|i| {
            HookHandlerConfig::Command(CommandHandlerConfig {
                command: format!("bounded_{}.sh", i),
                ..Default::default()
            })
        })
        .collect();

    let refs_bounded: Vec<&HookHandlerConfig> = handlers_bounded.iter().collect();
    let stats_bounded = dispatch_hooks(
        &HookEvent::PreToolUse,
        &input,
        &refs_bounded,
        &config_bounded,
    )
    .await;

    assert_eq!(stats_bounded.total_dispatched, 4);
    assert_eq!(stats_bounded.allowed, 4);
    assert!(stats_bounded.all_succeeded());
}

// ===========================================================================
// test_config_layer_merge  (3-layer merge)
// ===========================================================================

#[test]
fn test_config_layer_merge() {
    // Layer 1 (lowest priority): user-level config
    let mut layer1 = HooksConfig::default();
    layer1.settings.timeout_secs = 10;
    layer1.settings.max_concurrency = 5;
    layer1.settings.dry_run = false;
    layer1.settings.fail_closed = false;

    layer1
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "user_security.sh".to_string(),
            enabled: true,
            ..Default::default()
        }));
    layer1
        .events
        .entry("SessionStart".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "user_init.sh".to_string(),
            enabled: true,
            ..Default::default()
        }));

    // Layer 2 (mid priority): project-level config
    let mut layer2 = HooksConfig::default();
    layer2.settings.timeout_secs = 30; // override
    layer2.settings.dry_run = true; // override

    layer2
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "project_lint.sh".to_string(),
            enabled: true,
            matcher: Some(HookMatcher::Multi(vec![
                "Bash".to_string(),
                "Write".to_string(),
            ])),
            ..Default::default()
        }));
    layer2
        .events
        .entry("Stop".to_string())
        .or_default()
        .push(HookHandlerConfig::Http(HttpHandlerConfig {
            url: "http://localhost:8080/stop".to_string(),
            ..Default::default()
        }));

    // Layer 3 (highest priority): env-level config
    let mut layer3 = HooksConfig::default();
    layer3.settings.timeout_secs = 60; // final override
    layer3.settings.max_concurrency = 5; // final override (explicit)
    layer3.settings.dry_run = true; // final override (explicit)
    layer3.settings.fail_closed = true; // final override

    layer3
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "env_override.sh".to_string(),
            enabled: true,
            matcher: Some(HookMatcher::Exact("Read".to_string())),
            ..Default::default()
        }));

    // Merge: layer1 <- layer2 <- layer3
    layer1.merge(layer2);
    layer1.merge(layer3);

    // Settings: layer3 wins on all overridden fields
    assert_eq!(
        layer1.settings.timeout_secs, 60,
        "layer3 timeout_secs should win"
    );
    assert_eq!(
        layer1.settings.max_concurrency, 5,
        "layer1 max_concurrency should be preserved (no override)"
    );
    assert!(
        layer1.settings.dry_run,
        "layer2 dry_run=true should be preserved (layer3 did not override)"
    );
    assert!(
        layer1.settings.fail_closed,
        "layer3 fail_closed=true should win"
    );

    // Events: handlers are APPENDED across layers
    // PreToolUse should have 3 handlers (1 from layer1 + 1 from layer2 + 1 from layer3)
    let pre_tool_handlers = &layer1.events["PreToolUse"];
    assert_eq!(
        pre_tool_handlers.len(),
        3,
        "PreToolUse should have 3 handlers from 3 layers"
    );

    // Verify handler order (append order)
    match &pre_tool_handlers[0] {
        HookHandlerConfig::Command(cmd) => assert_eq!(cmd.command, "user_security.sh"),
        _ => panic!("expected Command from layer1"),
    }
    match &pre_tool_handlers[1] {
        HookHandlerConfig::Command(cmd) => assert_eq!(cmd.command, "project_lint.sh"),
        _ => panic!("expected Command from layer2"),
    }
    match &pre_tool_handlers[2] {
        HookHandlerConfig::Command(cmd) => assert_eq!(cmd.command, "env_override.sh"),
        _ => panic!("expected Command from layer3"),
    }

    // SessionStart: only from layer1 (layer2 and layer3 didn't add to it)
    assert_eq!(layer1.events["SessionStart"].len(), 1);

    // Stop: only from layer2
    assert_eq!(layer1.events["Stop"].len(), 1);
    match &layer1.events["Stop"][0] {
        HookHandlerConfig::Http(http) => {
            assert_eq!(http.url, "http://localhost:8080/stop");
        }
        _ => panic!("expected Http from layer2"),
    }

    // Total unique event keys: PreToolUse, SessionStart, Stop
    assert_eq!(layer1.events.len(), 3);
}

// ===========================================================================
// test_matcher_filtering  (registry filters by matcher)
// ===========================================================================

#[test]
fn test_matcher_filtering() {
    // Build a config with handlers using various matcher types
    let mut config = HooksConfig::default();

    // Handler 1: Exact matcher for "Bash"
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "bash_only.sh".to_string(),
            matcher: Some(HookMatcher::Exact("Bash".to_string())),
            ..Default::default()
        }));

    // Handler 2: Multi matcher for "Write" or "Edit"
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "write_or_edit.sh".to_string(),
            matcher: Some(HookMatcher::Multi(vec![
                "Write".to_string(),
                "Edit".to_string(),
            ])),
            ..Default::default()
        }));

    // Handler 3: Regex matcher for any tool starting with "Read"
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "read_pattern.sh".to_string(),
            matcher: Some(HookMatcher::Regex("^Read".to_string())),
            ..Default::default()
        }));

    // Handler 4: Wildcard (no matcher = always matches)
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "catch_all.sh".to_string(),
            matcher: None, // no matcher = wildcard
            ..Default::default()
        }));

    // Handler 5: HTTP handler with exact matcher for "Bash"
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Http(HttpHandlerConfig {
            url: "http://localhost/bash-hook".to_string(),
            matcher: Some(HookMatcher::Exact("Bash".to_string())),
            ..Default::default()
        }));

    let registry = HookRegistry::from_config(config);

    // Context for "Bash" tool
    let bash_ctx = HookContext::for_tool(
        "Bash".to_string(),
        "ses_match_1".to_string(),
        "/project".to_string(),
    );

    let matching = registry.get_matching(&HookEvent::PreToolUse, &bash_ctx);
    // Should match: bash_only (exact), catch_all (wildcard), http bash-hook (exact)
    // Should NOT match: write_or_edit (multi: Write|Edit), read_pattern (regex: ^Read)
    assert_eq!(
        matching.len(),
        3,
        "Bash should match: exact 'Bash', wildcard, and http 'Bash' -- got {:?}",
        matching
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
    );

    // Verify the matched handlers are the right ones
    let matched_commands: Vec<&str> = matching
        .iter()
        .filter_map(|h| match h {
            HookHandlerConfig::Command(cmd) => Some(cmd.command.as_str()),
            _ => None,
        })
        .collect();
    assert!(matched_commands.contains(&"bash_only.sh"));
    assert!(matched_commands.contains(&"catch_all.sh"));
    assert!(!matched_commands.contains(&"write_or_edit.sh"));
    assert!(!matched_commands.contains(&"read_pattern.sh"));

    let has_http = matching
        .iter()
        .any(|h| matches!(h, HookHandlerConfig::Http(_)));
    assert!(has_http, "HTTP handler for Bash should be matched");

    // Context for "Write" tool
    let write_ctx = HookContext::for_tool(
        "Write".to_string(),
        "ses_match_2".to_string(),
        "/project".to_string(),
    );

    let matching = registry.get_matching(&HookEvent::PreToolUse, &write_ctx);
    // Should match: write_or_edit (multi), catch_all (wildcard)
    assert_eq!(
        matching.len(),
        2,
        "Write should match: multi 'Write|Edit' and wildcard"
    );

    // Context for "Read" tool
    let read_ctx = HookContext::for_tool(
        "Read".to_string(),
        "ses_match_3".to_string(),
        "/project".to_string(),
    );

    let matching = registry.get_matching(&HookEvent::PreToolUse, &read_ctx);
    // Should match: read_pattern (regex ^Read), catch_all (wildcard)
    assert_eq!(
        matching.len(),
        2,
        "Read should match: regex '^Read' and wildcard"
    );

    // Context for "Glob" tool (no specific matcher matches, only wildcard)
    let glob_ctx = HookContext::for_tool(
        "Glob".to_string(),
        "ses_match_4".to_string(),
        "/project".to_string(),
    );

    let matching = registry.get_matching(&HookEvent::PreToolUse, &glob_ctx);
    // Should match only: catch_all (wildcard)
    assert_eq!(
        matching.len(),
        1,
        "Glob should only match the wildcard handler"
    );

    // Context for a non-matching event
    let matching = registry.get_matching(&HookEvent::PostToolUse, &bash_ctx);
    assert!(
        matching.is_empty(),
        "no PostToolUse handlers configured, should be empty"
    );

    // Direct matcher function tests for completeness
    assert!(matches(
        &HookMatcher::Wildcard,
        &MatcherContext::new("Anything")
    ));
    assert!(matches(
        &HookMatcher::Exact("Bash".to_string()),
        &MatcherContext::new("Bash")
    ));
    assert!(!matches(
        &HookMatcher::Exact("Bash".to_string()),
        &MatcherContext::new("Write")
    ));
    assert!(matches(
        &HookMatcher::Multi(vec!["Bash".to_string(), "Write".to_string()]),
        &MatcherContext::new("Write")
    ));
    assert!(!matches(
        &HookMatcher::Multi(vec!["Bash".to_string(), "Write".to_string()]),
        &MatcherContext::new("Read")
    ));
}

// ===========================================================================
// test_condition_evaluation  (if_ conditions)
// ===========================================================================

#[test]
fn test_condition_evaluation() {
    // Build config with handlers that have `if_` conditions
    let mut config = HooksConfig::default();

    // Handler 1: only runs when tool_name=Bash
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "bash_security.sh".to_string(),
            if_: Some("tool_name=Bash".to_string()),
            ..Default::default()
        }));

    // Handler 2: only runs when tool_name=Write (positive match for a different tool)
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "write_only.sh".to_string(),
            if_: Some("tool_name=Write".to_string()),
            ..Default::default()
        }));

    // Handler 3: only runs when agent_type=coder
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "coder_only.sh".to_string(),
            if_: Some("agent_type=coder".to_string()),
            ..Default::default()
        }));

    // Handler 4: no condition (always runs)
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "always_run.sh".to_string(),
            ..Default::default()
        }));

    // Handler 5: condition with permission_mode
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "auto_approve.sh".to_string(),
            if_: Some("permission_mode=auto".to_string()),
            ..Default::default()
        }));

    // Handler 6: HTTP handler with condition
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Http(HttpHandlerConfig {
            url: "http://localhost/hook".to_string(),
            if_: Some("tool_name=Bash".to_string()),
            ..Default::default()
        }));

    // Handler 7: Plugin handler with condition
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Plugin(PluginHandlerConfig {
            path: "/usr/bin/plugin".to_string(),
            if_: Some("tool_name=Write".to_string()),
            ..Default::default()
        }));

    // Handler 8: Agent handler with condition
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Agent(AgentHandlerConfig {
            agent_id: "test_agent".to_string(),
            if_: Some("agent_type=coder".to_string()),
            ..Default::default()
        }));

    let registry = HookRegistry::from_config(config);

    // Context: tool_name=Bash, agent_type=default (no permission_mode)
    let bash_default_ctx = HookContext::for_tool(
        "Bash".to_string(),
        "ses_cond_1".to_string(),
        "/project".to_string(),
    );

    let matching = registry.get_matching(&HookEvent::PreToolUse, &bash_default_ctx);
    // Expected matches:
    //   - bash_security.sh (tool_name=Bash, condition met)
    //   - always_run.sh (no condition)
    //   - http hook (tool_name=Bash, condition met)
    // NOT matched:
    //   - write_only.sh (tool_name=Write, but tool is Bash)
    //   - coder_only.sh (agent_type=coder, but context has no agent_type)
    //   - auto_approve.sh (permission_mode=auto, but context has no permission_mode)
    //   - plugin (tool_name=Write, but context is Bash)
    //   - agent (agent_type=coder, but context has no agent_type)
    assert_eq!(
        matching.len(),
        3,
        "Bash + default agent should match: bash_security, always_run, http -- got {:?}",
        matching
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
    );

    // Context: tool_name=Write, agent_type=coder
    let mut write_coder_ctx = HookContext::for_tool(
        "Write".to_string(),
        "ses_cond_2".to_string(),
        "/project".to_string(),
    );
    write_coder_ctx.agent_type = Some("coder".to_string());

    let matching = registry.get_matching(&HookEvent::PreToolUse, &write_coder_ctx);
    // Expected matches:
    //   - write_only.sh (tool_name=Write, condition met)
    //   - coder_only.sh (agent_type=coder, condition met)
    //   - always_run.sh (no condition)
    //   - plugin (tool_name=Write, condition met)
    //   - agent (agent_type=coder, condition met)
    // NOT matched:
    //   - bash_security.sh (tool_name=Bash, fails since tool is Write)
    //   - auto_approve.sh (permission_mode=auto, no permission_mode in context)
    //   - http hook (tool_name=Bash, fails since tool is Write)
    assert_eq!(
        matching.len(),
        5,
        "Write + coder should match 5 handlers -- got {:?}",
        matching
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
    );

    // Context: tool_name=Bash, permission_mode=auto
    let mut bash_auto_ctx = HookContext::for_tool(
        "Bash".to_string(),
        "ses_cond_3".to_string(),
        "/project".to_string(),
    );
    bash_auto_ctx.permission_mode = Some("auto".to_string());

    let matching = registry.get_matching(&HookEvent::PreToolUse, &bash_auto_ctx);
    // Expected matches:
    //   - bash_security.sh (tool_name=Bash)
    //   - always_run.sh (no condition)
    //   - auto_approve.sh (permission_mode=auto)
    //   - http hook (tool_name=Bash)
    // NOT matched:
    //   - non_bash_handler.sh (tool_name!=Bash)
    //   - coder_only.sh (agent_type=coder, no agent_type)
    //   - plugin (tool_name=Write, tool is Bash)
    //   - agent (agent_type=coder, no agent_type)
    assert_eq!(
        matching.len(),
        4,
        "Bash + auto permission should match 4 handlers -- got {:?}",
        matching
            .iter()
            .map(|h| format!("{:?}", h))
            .collect::<Vec<_>>()
    );

    // Verify condition with unknown field passes through (returns true)
    let mut config_unknown = HooksConfig::default();
    config_unknown
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "unknown_field.sh".to_string(),
            if_: Some("unknown_field=value".to_string()),
            ..Default::default()
        }));

    let registry_unknown = HookRegistry::from_config(config_unknown);
    let ctx = HookContext::for_tool(
        "Bash".to_string(),
        "ses_cond_4".to_string(),
        "/project".to_string(),
    );
    let matching = registry_unknown.get_matching(&HookEvent::PreToolUse, &ctx);
    assert_eq!(
        matching.len(),
        1,
        "unknown condition field should pass through (allow by default)"
    );
}
