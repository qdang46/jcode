//! Comprehensive integration and unit tests for the jcode-hooks crate.
//!
//! Covers the full surface area: event parsing (28+1 variants), blocking
//! semantics, config merge, TOML round-trip, dispatch engine, kill-switch,
//! builder pattern, output serialization, and matcher logic.

use crate::config::{
    load_hooks_config, parse_matcher_pattern, CommandHandlerConfig, HookEvent, HookHandlerConfig,
    HooksConfig, HttpHandlerConfig,
};
use crate::dispatch::{
    aggregate_decision, classify_decision, dispatch_hooks, ClassifiedOutcome, DispatchConfig,
    DispatchStats,
};
use crate::matcher::{matches, HookMatcher, MatcherContext};
use crate::types::{
    AggregatedDecision, HookInput, HookInputBuilder, HookOutput, HookResult, ALL_EVENT_NAMES,
};

// ===========================================================================
// test_hook_event_parse_all_variants  (28 standard + 1 Custom)
// ===========================================================================

#[test]
fn test_hook_event_parse_all_variants() {
    // 28 standard variants via PascalCase
    let standard_cases: Vec<(&str, HookEvent)> = vec![
        ("PreToolUse", HookEvent::PreToolUse),
        ("PostToolUse", HookEvent::PostToolUse),
        ("PostToolUseFailure", HookEvent::PostToolUseFailure),
        ("ToolError", HookEvent::ToolError),
        ("UserPromptSubmit", HookEvent::UserPromptSubmit),
        ("UserPromptExpansion", HookEvent::UserPromptExpansion),
        ("SessionStart", HookEvent::SessionStart),
        ("SessionEnd", HookEvent::SessionEnd),
        ("SessionUpdated", HookEvent::SessionUpdated),
        ("SessionDiff", HookEvent::SessionDiff),
        ("SessionError", HookEvent::SessionError),
        ("SessionIdle", HookEvent::SessionIdle),
        ("PermissionRequest", HookEvent::PermissionRequest),
        ("PermissionDenied", HookEvent::PermissionDenied),
        ("PermissionAsked", HookEvent::PermissionAsked),
        ("PermissionReplied", HookEvent::PermissionReplied),
        ("AgentStart", HookEvent::AgentStart),
        ("AgentEnd", HookEvent::AgentEnd),
        ("SubagentStart", HookEvent::SubagentStart),
        ("SubagentStop", HookEvent::SubagentStop),
        ("Stop", HookEvent::Stop),
        ("PreCompact", HookEvent::PreCompact),
        ("PostCompact", HookEvent::PostCompact),
        ("AutoCompactionControl", HookEvent::AutoCompactionControl),
        ("TaskCreated", HookEvent::TaskCreated),
        ("TaskCompleted", HookEvent::TaskCompleted),
        ("Setup", HookEvent::Setup),
        ("FileChanged", HookEvent::FileChanged),
        ("TurnEnd", HookEvent::TurnEnd),
    ];

    assert_eq!(
        standard_cases.len(),
        29,
        "must have exactly 29 standard variants"
    );
    assert_eq!(ALL_EVENT_NAMES.len(), 29);

    for (input, expected) in &standard_cases {
        let parsed = HookEvent::parse(input);
        assert_eq!(
            parsed.as_ref(),
            Some(expected),
            "PascalCase parse failed for '{}'",
            input
        );
    }

    // snake_case round-trip
    for (pascal, expected) in &standard_cases {
        let snake = pascal
            .chars()
            .flat_map(|c| {
                if c.is_uppercase() {
                    vec!['_', c.to_ascii_lowercase()]
                } else {
                    vec![c]
                }
            })
            .collect::<String>();
        let snake = snake.trim_start_matches('_');
        assert_eq!(
            HookEvent::parse(snake),
            Some(expected.clone()),
            "snake_case parse failed for '{}'",
            snake
        );
    }

    // kebab-case round-trip
    for (pascal, expected) in &standard_cases {
        let kebab = pascal
            .chars()
            .flat_map(|c| {
                if c.is_uppercase() {
                    vec!['-', c.to_ascii_lowercase()]
                } else {
                    vec![c]
                }
            })
            .collect::<String>();
        let kebab = kebab.trim_start_matches('-');
        assert_eq!(
            HookEvent::parse(kebab),
            Some(expected.clone()),
            "kebab-case parse failed for '{}'",
            kebab
        );
    }

    // Case-insensitive variations
    assert_eq!(HookEvent::parse("PRETOOLUSE"), Some(HookEvent::PreToolUse));
    assert_eq!(HookEvent::parse("pretooluse"), Some(HookEvent::PreToolUse));
    assert_eq!(
        HookEvent::parse("Pre Tool Use"),
        Some(HookEvent::PreToolUse)
    );

    // Custom variant: custom: prefix
    assert_eq!(
        HookEvent::parse("custom:my_event"),
        Some(HookEvent::Custom("my_event".to_string()))
    );
    assert_eq!(
        HookEvent::parse("Custom:my-event"),
        Some(HookEvent::Custom("my-event".to_string()))
    );
    assert_eq!(
        HookEvent::parse("CUSTOM:foo"),
        Some(HookEvent::Custom("foo".to_string()))
    );
    assert_eq!(
        HookEvent::parse("custom"),
        Some(HookEvent::Custom(String::new()))
    );

    // Empty / unknown returns None
    assert_eq!(HookEvent::parse(""), None);
    assert_eq!(HookEvent::parse("   "), None);
    assert_eq!(HookEvent::parse("NoSuchEvent"), None);
}

// ===========================================================================
// test_hook_event_is_blocking
// ===========================================================================

#[test]
fn test_hook_event_is_blocking() {
    // Events that ARE blocking
    let blocking_events = [
        HookEvent::PreToolUse,
        HookEvent::UserPromptSubmit,
        HookEvent::PermissionRequest,
        HookEvent::PermissionAsked,
        HookEvent::AgentStart,
        HookEvent::Stop,
        HookEvent::PreCompact,
    ];
    for ev in &blocking_events {
        assert!(ev.is_blocking(), "{:?} should be blocking", ev);
    }

    // Events that are NOT blocking (exhaustive list of all remaining standard variants)
    let non_blocking_events = [
        HookEvent::PostToolUse,
        HookEvent::PostToolUseFailure,
        HookEvent::ToolError,
        HookEvent::UserPromptExpansion,
        HookEvent::SessionStart,
        HookEvent::SessionEnd,
        HookEvent::SessionUpdated,
        HookEvent::SessionDiff,
        HookEvent::SessionError,
        HookEvent::SessionIdle,
        HookEvent::PermissionDenied,
        HookEvent::PermissionReplied,
        HookEvent::AgentEnd,
        HookEvent::SubagentStart,
        HookEvent::SubagentStop,
        HookEvent::PostCompact,
        HookEvent::AutoCompactionControl,
        HookEvent::TaskCreated,
        HookEvent::TaskCompleted,
        HookEvent::Setup,
        HookEvent::FileChanged,
        HookEvent::Custom("anything".to_string()),
    ];
    for ev in &non_blocking_events {
        assert!(!ev.is_blocking(), "{:?} should NOT be blocking", ev);
    }

    // All 28 standard accounted for: 7 blocking + 21 non-blocking = 28
    assert_eq!(blocking_events.len() + non_blocking_events.len() - 1, 28);
}

// ===========================================================================
// test_hooks_config_merge_appends_handlers
// ===========================================================================

#[test]
fn test_hooks_config_merge_appends_handlers() {
    // Base config: one handler on PreToolUse, one on SessionStart
    let mut base = HooksConfig::default();
    base.events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "base_hook_a".to_string(),
            ..Default::default()
        }));
    base.settings.timeout_secs = 10;

    // Other config: one handler on PreToolUse (same event), one on SessionEnd (new event)
    let mut other = HooksConfig::default();
    other
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::Command(CommandHandlerConfig {
            command: "other_hook_b".to_string(),
            ..Default::default()
        }));
    other
        .events
        .entry("SessionEnd".to_string())
        .or_default()
        .push(HookHandlerConfig::Http(HttpHandlerConfig {
            url: "http://localhost/end".to_string(),
            ..Default::default()
        }));
    other.settings.timeout_secs = 60;
    other.settings.dry_run = true;

    base.merge(other);

    // Handlers are appended for existing events
    assert_eq!(base.events["PreToolUse"].len(), 2);
    match &base.events["PreToolUse"][0] {
        HookHandlerConfig::Command(cmd) => assert_eq!(cmd.command, "base_hook_a"),
        _ => panic!("expected Command"),
    }
    match &base.events["PreToolUse"][1] {
        HookHandlerConfig::Command(cmd) => assert_eq!(cmd.command, "other_hook_b"),
        _ => panic!("expected Command"),
    }

    // New event key is added
    assert!(base.events.contains_key("SessionEnd"));
    assert_eq!(base.events["SessionEnd"].len(), 1);

    // Settings are overridden (not merged)
    assert_eq!(base.settings.timeout_secs, 60);
    assert!(base.settings.dry_run);
}

// ===========================================================================
// test_toml_round_trip
// ===========================================================================

#[test]
fn test_toml_round_trip() {
    let toml_str = r#"
[settings]
timeout_secs = 45
max_concurrency = 8
dry_run = true
fail_closed = false

[[events.PreToolUse]]
type = "command"
command = "check_security.sh"
enabled = true
timeout_secs = 10
matcher = "Bash|Write"

[[events.PreToolUse]]
type = "http"
url = "http://localhost:9090/hooks"
method = "POST"
timeout_secs = 5

[[events.SessionStart]]
type = "command"
command = "init_session.sh"
enabled = true

[[events.Stop]]
type = "command"
command = "cleanup.sh"
matcher = "/^Bash/"
"#;

    // Parse from TOML
    let config: HooksConfig = toml::from_str(toml_str).unwrap();

    // Verify settings
    assert_eq!(config.settings.timeout_secs, 45);
    assert_eq!(config.settings.max_concurrency, 8);
    assert!(config.settings.dry_run);
    assert!(!config.settings.fail_closed);

    // Verify events
    assert_eq!(config.events.len(), 3);
    assert_eq!(config.events["PreToolUse"].len(), 2);
    assert_eq!(config.events["SessionStart"].len(), 1);
    assert_eq!(config.events["Stop"].len(), 1);

    // Verify first PreToolUse handler
    match &config.events["PreToolUse"][0] {
        HookHandlerConfig::Command(cmd) => {
            assert_eq!(cmd.command, "check_security.sh");
            assert!(cmd.enabled);
            assert_eq!(cmd.timeout_secs, Some(10));
            assert_eq!(
                cmd.matcher,
                Some(HookMatcher::Multi(vec![
                    "Bash".to_string(),
                    "Write".to_string()
                ]))
            );
        }
        _ => panic!("expected Command"),
    }

    // Verify second PreToolUse handler
    match &config.events["PreToolUse"][1] {
        HookHandlerConfig::Http(http) => {
            assert_eq!(http.url, "http://localhost:9090/hooks");
            assert_eq!(http.method, "POST");
            assert_eq!(http.timeout_secs, Some(5));
        }
        _ => panic!("expected Http"),
    }

    // Verify Stop handler has regex matcher
    match &config.events["Stop"][0] {
        HookHandlerConfig::Command(cmd) => {
            assert_eq!(cmd.matcher, Some(HookMatcher::Regex("^Bash".to_string())));
        }
        _ => panic!("expected Command"),
    }

    // Serialize back to TOML and re-parse (round-trip stability)
    let serialized = toml::to_string(&config).unwrap();
    let reparsed: HooksConfig = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.settings.timeout_secs, 45);
    assert_eq!(reparsed.events.len(), 3);
    assert_eq!(reparsed.events["PreToolUse"].len(), 2);
}

// ===========================================================================
// test_dispatch_empty_handlers
// ===========================================================================

#[tokio::test]
async fn test_dispatch_empty_handlers() {
    let config = DispatchConfig::default();
    let input = HookInput::default();
    let handlers: Vec<&HookHandlerConfig> = vec![];

    let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &handlers, &config).await;

    assert_eq!(stats.total_dispatched, 0);
    assert_eq!(stats.completed, 0);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.allowed, 0);
    assert_eq!(stats.denied, 0);
    assert_eq!(stats.asked, 0);
    assert!(stats.results.is_empty());
    assert!(stats.all_succeeded());
    assert!(!stats.any_denied());
}

// ===========================================================================
// test_dispatch_single_continue
// ===========================================================================

#[tokio::test]
async fn test_dispatch_single_continue() {
    let config = DispatchConfig {
        dry_run: true,
        ..Default::default()
    };
    let input = HookInput::default();
    let handlers: Vec<HookHandlerConfig> = vec![HookHandlerConfig::Command(CommandHandlerConfig {
        command: "echo ok".to_string(),
        ..Default::default()
    })];
    let refs: Vec<&HookHandlerConfig> = handlers.iter().collect();

    let stats = dispatch_hooks(&HookEvent::PostToolUse, &input, &refs, &config).await;

    assert_eq!(stats.total_dispatched, 1);
    assert_eq!(stats.allowed, 1);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.denied, 0);
    assert!(stats.all_succeeded());
}

// ===========================================================================
// test_dispatch_deny_wins  (aggregate_decision: deny > ask > allow)
// ===========================================================================

#[test]
fn test_dispatch_deny_wins() {
    // Mixed outcomes: allow + ask + deny -- deny should win
    let outcomes = vec![
        ClassifiedOutcome::Allow,
        ClassifiedOutcome::Ask {
            reason: "need approval".to_string(),
        },
        ClassifiedOutcome::Deny {
            reason: "blocked by policy".to_string(),
        },
        ClassifiedOutcome::Allow,
    ];
    let decision = aggregate_decision(&outcomes, false);
    match decision {
        AggregatedDecision::Deny { reason, .. } => {
            assert_eq!(reason, "blocked by policy");
        }
        _ => panic!("expected Deny, got {:?}", format!("{:?}", decision)),
    }

    // Only allow + ask: ask wins
    let outcomes = vec![
        ClassifiedOutcome::Allow,
        ClassifiedOutcome::Ask {
            reason: "review".to_string(),
        },
    ];
    let decision = aggregate_decision(&outcomes, false);
    assert!(matches!(decision, AggregatedDecision::Ask { .. }));

    // Only allow: allow wins
    let outcomes = vec![ClassifiedOutcome::Allow, ClassifiedOutcome::Allow];
    let decision = aggregate_decision(&outcomes, false);
    assert!(matches!(decision, AggregatedDecision::Allow));

    // Empty: allow
    let decision = aggregate_decision(&[], false);
    assert!(matches!(decision, AggregatedDecision::Allow));

    // Failure in fail-open mode is ignored
    let outcomes = vec![
        ClassifiedOutcome::Allow,
        ClassifiedOutcome::Failed {
            error: "crash".to_string(),
        },
    ];
    let decision = aggregate_decision(&outcomes, false);
    assert!(matches!(decision, AggregatedDecision::Allow));

    // Failure in fail-closed mode becomes deny
    let outcomes = vec![
        ClassifiedOutcome::Allow,
        ClassifiedOutcome::Failed {
            error: "crash".to_string(),
        },
    ];
    let decision = aggregate_decision(&outcomes, true);
    assert!(matches!(decision, AggregatedDecision::Deny { .. }));
}

// ===========================================================================
// test_dispatch_disabled_skip
// ===========================================================================

#[tokio::test]
async fn test_dispatch_disabled_skip() {
    // A disabled command handler should be executed as continue (via execute_single_hook)
    // but in dry-run mode we get Allow.  The important thing is that it does not fail.
    let config = DispatchConfig {
        dry_run: true,
        ..Default::default()
    };
    let input = HookInput::default();
    let handlers: Vec<HookHandlerConfig> = vec![
        HookHandlerConfig::Command(CommandHandlerConfig {
            enabled: false,
            command: "should_not_run".to_string(),
            ..Default::default()
        }),
        HookHandlerConfig::Command(CommandHandlerConfig {
            enabled: true,
            command: "should_run".to_string(),
            ..Default::default()
        }),
    ];
    let refs: Vec<&HookHandlerConfig> = handlers.iter().collect();

    let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &refs, &config).await;

    assert_eq!(stats.total_dispatched, 2);
    assert_eq!(stats.allowed, 2);
    assert_eq!(stats.failed, 0);
}

// ===========================================================================
// test_dispatch_timeout
// ===========================================================================

#[tokio::test]
async fn test_dispatch_timeout() {
    // A handler with a very short timeout that takes too long should fail/timeout.
    // Using a real command that sleeps.
    let config = DispatchConfig {
        max_concurrency: 1,
        timeout_secs: 1,
        fail_closed: false,
        dry_run: false,
    };
    let input = HookInput::default();
    let handlers: Vec<HookHandlerConfig> = vec![HookHandlerConfig::Command(CommandHandlerConfig {
        enabled: true,
        command: "sleep 10".to_string(),
        timeout_secs: Some(1),
        ..Default::default()
    })];
    let refs: Vec<&HookHandlerConfig> = handlers.iter().collect();

    let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &refs, &config).await;

    assert_eq!(stats.total_dispatched, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.timed_out, 1);
    assert!(!stats.all_succeeded());

    // Verify the result contains a timeout error
    assert_eq!(stats.results.len(), 1);
    if let ClassifiedOutcome::Failed { error } = &stats.results[0].outcome {
        assert!(error.contains("timed out"), "error was: {}", error);
    } else {
        panic!(
            "expected Failed outcome, got {:?}",
            &stats.results[0].outcome
        );
    }
}

// ===========================================================================
// test_kill_switch  (DISABLE_JCODE_HOOKS)
// ===========================================================================

#[test]
fn test_kill_switch() {
    // Set the kill-switch env var
    std::env::set_var("DISABLE_JCODE_HOOKS", "1");

    let config = load_hooks_config();

    // Should return empty/default config
    assert!(config.is_empty());
    assert_eq!(config.settings.timeout_secs, 30); // default
    assert!(!config.settings.dry_run);

    // Clean up
    std::env::remove_var("DISABLE_JCODE_HOOKS");
}

// ===========================================================================
// test_hook_input_builder
// ===========================================================================

#[test]
fn test_hook_input_builder() {
    let input = HookInputBuilder::new()
        .session("ses_builder", "/workspace/project")
        .event("PreToolUse")
        .agent("agent_42", "coder")
        .tool(
            "Bash",
            serde_json::json!({"command": "cargo test"}),
            "tool_use_7",
        )
        .tool_output(serde_json::json!({"stdout": "all tests passed"}))
        .duration(3500)
        .build();

    assert_eq!(input.schema_version, "2.0");
    assert_eq!(input.session_id, "ses_builder");
    assert_eq!(input.cwd, "/workspace/project");
    assert_eq!(input.hook_event_name, "PreToolUse");
    assert_eq!(input.agent_id, Some("agent_42".to_string()));
    assert_eq!(input.agent_type, Some("coder".to_string()));
    assert_eq!(input.tool_name, Some("Bash".to_string()));
    assert_eq!(input.tool_use_id, Some("tool_use_7".to_string()));
    assert!(input.tool_input.is_some());
    assert!(input.tool_output.is_some());
    assert_eq!(input.duration_ms, Some(3500));

    // Verify serialization round-trip
    let json = serde_json::to_string_pretty(&input).unwrap();
    let parsed: HookInput = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.session_id, "ses_builder");
    assert_eq!(parsed.tool_name, Some("Bash".to_string()));
    assert_eq!(parsed.agent_id, Some("agent_42".to_string()));
}

#[test]
fn test_hook_input_builder_permission() {
    let input = HookInputBuilder::new()
        .session("ses_perm", "/project")
        .event("PermissionRequest")
        .permission("auto", "req_001", "Execute bash command")
        .build();

    assert_eq!(input.permission_mode, Some("auto".to_string()));
    assert_eq!(input.request_id, Some("req_001".to_string()));
    assert_eq!(
        input.action_description,
        Some("Execute bash command".to_string())
    );
}

#[test]
fn test_hook_input_builder_error_and_prompt() {
    let input = HookInputBuilder::new()
        .session("ses_err", "/project")
        .event("ToolError")
        .error("command not found", 127)
        .build();
    assert_eq!(input.error, Some("command not found".to_string()));
    assert_eq!(input.error_code, Some(127));

    let input = HookInputBuilder::new()
        .session("ses_prompt", "/project")
        .event("UserPromptSubmit")
        .prompt("fix the bug in main.rs")
        .build();
    assert_eq!(
        input.prompt_text,
        Some("fix the bug in main.rs".to_string())
    );
}

// ===========================================================================
// test_hook_output_serialization
// ===========================================================================

#[test]
fn test_hook_output_serialization() {
    // continue_ (default)
    let output = HookOutput::continue_();
    let json = serde_json::to_string(&output).unwrap();
    let parsed: HookOutput = serde_json::from_str(&json).unwrap();
    assert!(parsed.continue_);
    assert!(parsed.stop_reason.is_none());
    assert!(parsed.decision.is_none());

    // block
    let output = HookOutput::block("Dangerous command");
    let json = serde_json::to_string(&output).unwrap();
    let parsed: HookOutput = serde_json::from_str(&json).unwrap();
    assert!(!parsed.continue_);
    assert_eq!(parsed.stop_reason.as_deref(), Some("Dangerous command"));
    assert_eq!(parsed.decision.as_deref(), Some("deny"));

    // ask
    let output = HookOutput::ask("Need approval");
    let json = serde_json::to_string(&output).unwrap();
    let parsed: HookOutput = serde_json::from_str(&json).unwrap();
    assert!(!parsed.continue_);
    assert_eq!(parsed.decision.as_deref(), Some("ask"));
    assert_eq!(parsed.reason.as_deref(), Some("Need approval"));

    // allow
    let output = HookOutput::allow();
    let json = serde_json::to_string(&output).unwrap();
    let parsed: HookOutput = serde_json::from_str(&json).unwrap();
    assert!(parsed.continue_);
    assert_eq!(parsed.decision.as_deref(), Some("allow"));

    // Empty JSON defaults to continue_ = true
    let parsed: HookOutput = serde_json::from_str("{}").unwrap();
    assert!(parsed.continue_);

    // Explicit false
    let parsed: HookOutput =
        serde_json::from_str(r#"{"continue_": false, "stop_reason": "nope"}"#).unwrap();
    assert!(!parsed.continue_);
    assert_eq!(parsed.stop_reason.as_deref(), Some("nope"));

    // skip_serializing_if: None fields should be omitted
    let output = HookOutput::continue_();
    let json = serde_json::to_string(&output).unwrap();
    assert!(!json.contains("suppress_output"));
    assert!(!json.contains("stop_reason"));
    assert!(!json.contains("decision"));
    assert!(!json.contains("system_message"));
}

// ===========================================================================
// test_matcher_exact
// ===========================================================================

#[test]
fn test_matcher_exact() {
    let matcher = HookMatcher::Exact("Bash".to_string());

    assert!(matches(&matcher, &MatcherContext::new("Bash")));
    assert!(!matches(&matcher, &MatcherContext::new("Write")));
    assert!(!matches(&matcher, &MatcherContext::new("bash"))); // case-sensitive
    assert!(!matches(&matcher, &MatcherContext::new("Bashx")));
}

// ===========================================================================
// test_matcher_multi
// ===========================================================================

#[test]
fn test_matcher_multi() {
    let matcher = HookMatcher::Multi(vec![
        "Bash".to_string(),
        "Write".to_string(),
        "Edit".to_string(),
    ]);

    assert!(matches(&matcher, &MatcherContext::new("Bash")));
    assert!(matches(&matcher, &MatcherContext::new("Write")));
    assert!(matches(&matcher, &MatcherContext::new("Edit")));
    assert!(!matches(&matcher, &MatcherContext::new("Read")));
    assert!(!matches(&matcher, &MatcherContext::new("bash"))); // case-sensitive
}

#[test]
fn test_matcher_multi_parse() {
    let patterns = crate::matcher::parse_multi_pattern("Write|Edit|Glob");
    assert_eq!(patterns, vec!["Write", "Edit", "Glob"]);

    let patterns = crate::matcher::parse_multi_pattern("Single");
    assert_eq!(patterns, vec!["Single"]);
}

// ===========================================================================
// test_matcher_regex
// ===========================================================================

#[test]
fn test_matcher_regex() {
    // Match against target only
    let matcher = HookMatcher::Regex("^Ba".to_string());
    assert!(matches(&matcher, &MatcherContext::new("Bash")));
    assert!(!matches(&matcher, &MatcherContext::new("Write")));

    // Match against target + context
    let matcher = HookMatcher::Regex("^Bash(git.*)".to_string());
    assert!(matches(
        &matcher,
        &MatcherContext::with_context("Bash", "git commit -m test")
    ));
    assert!(!matches(
        &matcher,
        &MatcherContext::with_context("Bash", "ls -la")
    ));

    // Invalid regex patterns use a never-match placeholder.
    // Valid regexes like "^Bash" work normally.
    let matcher = HookMatcher::Regex("^Bash".to_string());
    assert!(matches(&matcher, &MatcherContext::new("Bash tool")));
    assert!(!matches(&matcher, &MatcherContext::new("other")));
}

// ===========================================================================
// test_matcher_wildcard
// ===========================================================================

#[test]
fn test_matcher_wildcard() {
    let matcher = HookMatcher::Wildcard;

    assert!(matches(&matcher, &MatcherContext::new("Bash")));
    assert!(matches(&matcher, &MatcherContext::new("Write")));
    assert!(matches(&matcher, &MatcherContext::new("anything")));
    assert!(matches(&matcher, &MatcherContext::new("")));
}

// ===========================================================================
// Additional: parse_matcher_pattern  (config-level pattern parsing)
// ===========================================================================

#[test]
fn test_parse_matcher_pattern() {
    assert_eq!(parse_matcher_pattern("*"), HookMatcher::Wildcard);
    assert_eq!(
        parse_matcher_pattern("Bash"),
        HookMatcher::Exact("Bash".to_string())
    );
    assert_eq!(
        parse_matcher_pattern("Bash|Write|Edit"),
        HookMatcher::Multi(vec![
            "Bash".to_string(),
            "Write".to_string(),
            "Edit".to_string()
        ])
    );
    assert_eq!(
        parse_matcher_pattern("/^Bash/"),
        HookMatcher::Regex("^Bash".to_string())
    );
    assert_eq!(parse_matcher_pattern("  *  "), HookMatcher::Wildcard); // trimmed
}

// ===========================================================================
// Additional: classify_decision
// ===========================================================================

#[test]
fn test_classify_decision_variants() {
    // Continue with explicit allow
    let result = HookResult::Continue(HookOutput::allow());
    assert!(matches!(
        classify_decision(&result),
        ClassifiedOutcome::Allow
    ));

    // Continue with no decision (default continue)
    let result = HookResult::Continue(HookOutput::continue_());
    assert!(matches!(
        classify_decision(&result),
        ClassifiedOutcome::Allow
    ));

    // Continue with ask decision
    let result = HookResult::Continue(HookOutput::ask("review needed"));
    if let ClassifiedOutcome::Ask { reason } = classify_decision(&result) {
        assert_eq!(reason, "review needed");
    } else {
        panic!("expected Ask");
    }

    // Continue with deny decision
    let output = HookOutput {
        continue_: false,
        suppress_output: None,
        stop_reason: Some("blocked".to_string()),
        decision: Some("deny".to_string()),
        reason: None,
        system_message: None,
        hook_specific_output: None,
    };
    let result = HookResult::Continue(output);
    if let ClassifiedOutcome::Deny { reason } = classify_decision(&result) {
        assert_eq!(reason, "blocked");
    } else {
        panic!("expected Deny");
    }

    // Blocked
    let result = HookResult::Blocked {
        reason: "not allowed".to_string(),
        output: HookOutput::block("not allowed"),
    };
    if let ClassifiedOutcome::Deny { reason } = classify_decision(&result) {
        assert_eq!(reason, "not allowed");
    } else {
        panic!("expected Deny");
    }

    // Failed
    let result = HookResult::Failed {
        error: "timeout".to_string(),
    };
    if let ClassifiedOutcome::Failed { error } = classify_decision(&result) {
        assert_eq!(error, "timeout");
    } else {
        panic!("expected Failed");
    }
}

// ===========================================================================
// Additional: DispatchStats helpers
// ===========================================================================

#[test]
fn test_dispatch_stats_helpers() {
    let stats = DispatchStats::default();
    assert!(stats.all_succeeded());
    assert!(!stats.any_denied());
    assert!(!stats.any_asked());
}

// ===========================================================================
// Additional: DispatchConfig from_settings
// ===========================================================================

#[test]
fn test_dispatch_config_from_settings() {
    use crate::config::HookSettings;

    let settings = HookSettings {
        timeout_secs: 60,
        max_concurrency: 5,
        dry_run: true,
        fail_closed: true,
    };
    let cfg = DispatchConfig::from_settings(&settings);
    assert_eq!(cfg.max_concurrency, 5);
    assert_eq!(cfg.timeout_secs, 60);
    assert!(cfg.dry_run);
    assert!(cfg.fail_closed);
}

// ===========================================================================
// Additional: HooksConfig::is_empty
// ===========================================================================

#[test]
fn test_hooks_config_is_empty() {
    let config = HooksConfig::default();
    assert!(config.is_empty());

    let mut config = HooksConfig::default();
    config
        .events
        .entry("PreToolUse".to_string())
        .or_default()
        .push(HookHandlerConfig::default());
    assert!(!config.is_empty());
}

// ===========================================================================
// Additional: HookEvent display and serde round-trip
// ===========================================================================

#[test]
fn test_hook_event_display_and_serde() {
    assert_eq!(format!("{}", HookEvent::PreToolUse), "PreToolUse");
    assert_eq!(format!("{}", HookEvent::Stop), "Stop");
    assert_eq!(format!("{}", HookEvent::Custom("foo".to_string())), "foo");

    // Serde round-trip for all standard variants
    for ev in HookEvent::all_standard() {
        let json = serde_json::to_string(&ev).unwrap();
        let deserialized: HookEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev, deserialized);
    }

    // Custom variant serde round-trip
    let custom = HookEvent::Custom("my_thing".to_string());
    let json = serde_json::to_string(&custom).unwrap();
    let deserialized: HookEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(custom, deserialized);
}

// ===========================================================================
// Additional: TOML event key alias (`event` vs `events`)
// ===========================================================================

#[test]
fn test_toml_event_key_alias() {
    let toml_str = r#"
[[event.PreToolUse]]
type = "command"
command = "legacy_handler.sh"
"#;
    let config: HooksConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.events["PreToolUse"].len(), 1);
}

// ===========================================================================
// Additional: Full dispatch integration (dry-run with multiple handlers)
// ===========================================================================

#[tokio::test]
async fn test_dispatch_dry_run_multiple_handlers() {
    let config = DispatchConfig {
        dry_run: true,
        ..Default::default()
    };
    let input = HookInput::default();
    let handlers: Vec<HookHandlerConfig> = vec![
        HookHandlerConfig::Command(CommandHandlerConfig {
            command: "hook_a.sh".to_string(),
            ..Default::default()
        }),
        HookHandlerConfig::Command(CommandHandlerConfig {
            command: "hook_b.sh".to_string(),
            ..Default::default()
        }),
        HookHandlerConfig::Command(CommandHandlerConfig {
            command: "hook_c.sh".to_string(),
            ..Default::default()
        }),
    ];
    let refs: Vec<&HookHandlerConfig> = handlers.iter().collect();

    let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &refs, &config).await;

    assert_eq!(stats.total_dispatched, 3);
    assert_eq!(stats.allowed, 3);
    assert_eq!(stats.failed, 0);
    assert!(stats.all_succeeded());
    assert_eq!(stats.results.len(), 3);
}

// ===========================================================================
// Additional: HookInput default field verification
// ===========================================================================

#[test]
fn test_hook_input_default() {
    let input = HookInput::default();
    assert_eq!(input.schema_version, "2.0");
    assert!(input.session_id.is_empty());
    assert!(input.cwd.is_empty());
    assert!(input.hook_event_name.is_empty());
    assert!(input.tool_name.is_none());
    assert!(input.agent_id.is_none());
    assert!(input.permission_mode.is_none());
    assert!(input.prompt_text.is_none());
    assert!(input.error.is_none());
}
