#[cfg(test)]
mod tests {
    use crate::serde;
    use crate::*;

    #[test]
    fn plugin_id_npm_creates_correct_format() {
        let id = PluginId::npm("my-plugin");
        assert_eq!(id.as_str(), "npm:my-plugin");
    }

    #[test]
    fn plugin_id_file_creates_correct_format() {
        let id = PluginId::file("/home/user/plugin.wasm");
        assert_eq!(id.as_str(), "file:/home/user/plugin.wasm");
    }

    #[test]
    fn plugin_id_bundled_creates_correct_format() {
        let id = PluginId::bundled("builtin-fs");
        assert_eq!(id.as_str(), "builtin:builtin-fs");
    }

    #[test]
    fn plugin_id_display_matches_as_str() {
        let id = PluginId::npm("hello");
        assert_eq!(format!("{id}"), "npm:hello");
    }

    #[test]
    fn plugin_id_short_name_strips_prefix() {
        let id = PluginId::npm("hello");
        assert_eq!(id.short_name(), "hello");
        let file = PluginId::file("/tmp/x.wasm");
        assert_eq!(file.short_name(), "/tmp/x.wasm");
        let bundled = PluginId::bundled("foo");
        assert_eq!(bundled.short_name(), "foo");
    }

    #[test]
    fn plugin_id_short_name_no_prefix_returns_whole() {
        let id = PluginId::from("raw".to_string());
        assert_eq!(id.short_name(), "raw");
    }

    #[test]
    fn plugin_id_from_string() {
        let id = PluginId::from("custom:test".to_string());
        assert_eq!(id.as_str(), "custom:test");
    }

    #[test]
    fn plugin_id_to_string() {
        let id = PluginId::npm("pkg");
        assert_eq!(id.to_string(), "npm:pkg");
    }

    #[test]
    fn plugin_version_fields() {
        let ver = PluginVersion {
            semver: semver::Version::new(1, 2, 3),
            jcode_min_version: Some(semver::Version::new(0, 9, 0)),
            jcode_max_version: None,
        };
        assert_eq!(ver.semver.to_string(), "1.2.3");
        assert_eq!(ver.jcode_min_version.unwrap().major, 0);
        assert!(ver.jcode_max_version.is_none());
    }

    #[test]
    fn plugin_state_variants() {
        assert_eq!(PluginState::Discovered, PluginState::Discovered);
        assert_eq!(PluginState::Loading, PluginState::Loading);
        assert_eq!(PluginState::Loaded, PluginState::Loaded);
        assert_eq!(PluginState::Active, PluginState::Active);
        assert_eq!(PluginState::Disabled, PluginState::Disabled);
        assert_eq!(PluginState::Blocked, PluginState::Blocked);
        match PluginState::Error("msg".into()) {
            PluginState::Error(msg) => assert_eq!(msg, "msg"),
            _ => panic!("expected Error variant"),
        }
    }

    #[test]
    fn plugin_origin_variants() {
        let npm = PluginOrigin::NpmPackage {
            name: "pkg".into(),
            version: "1.0".into(),
        };
        let file = PluginOrigin::LocalFile {
            path: "/p.wasm".into(),
        };
        let builtin = PluginOrigin::Builtin {
            name: "core".into(),
        };
        let remote = PluginOrigin::Remote {
            url: "https://example.com".into(),
        };
        assert_eq!(
            format!("{npm:?}"),
            r#"NpmPackage { name: "pkg", version: "1.0" }"#
        );
        assert_eq!(format!("{file:?}"), r#"LocalFile { path: "/p.wasm" }"#);
        assert_eq!(format!("{builtin:?}"), r#"Builtin { name: "core" }"#);
        assert_eq!(
            format!("{remote:?}"),
            r#"Remote { url: "https://example.com" }"#
        );
    }

    #[test]
    fn plugin_id_serde_roundtrip() {
        let id = PluginId::npm("test-plugin");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"npm:test-plugin\"");
        let deserialized: PluginId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[test]
    fn plugin_version_serde_roundtrip() {
        let ver = PluginVersion {
            semver: semver::Version::new(0, 5, 0),
            jcode_min_version: None,
            jcode_max_version: None,
        };
        let json = serde_json::to_string(&ver).unwrap();
        let deserialized: PluginVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.semver, ver.semver);
    }

    #[test]
    fn plugin_state_serde_roundtrip() {
        for state in &[
            PluginState::Active,
            PluginState::Disabled,
            PluginState::Error("fail".into()),
        ] {
            let json = serde_json::to_string(state).unwrap();
            let deserialized: PluginState = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, *state);
        }
    }

    #[test]
    fn plugin_origin_serde_roundtrip() {
        let origin = PluginOrigin::NpmPackage {
            name: "pkg".into(),
            version: "1.0".into(),
        };
        let json = serde_json::to_string(&origin).unwrap();
        let deserialized: PluginOrigin = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, origin);
    }

    #[test]
    fn capability_chain_default_is_deny() {
        let chain = CapabilityChain::default();
        let result = chain.check("anything", &CapabilityAction::Read);
        assert!(matches!(result, AccessDecision::Denied(_)));
    }

    #[test]
    fn capability_chain_allow_list_allows() {
        let mut chain = CapabilityChain::default();
        chain.allow_list.tools.push("my_tool".into());
        let result = chain.check("my_tool", &CapabilityAction::Read);
        assert!(matches!(result, AccessDecision::Allowed(_)));
    }

    #[test]
    fn capability_chain_deny_list_denies() {
        let mut chain = CapabilityChain::default();
        chain.allow_list.tools.push("my_tool".into());
        chain.deny_list.tools.push("my_tool".into());
        let result = chain.check("my_tool", &CapabilityAction::Read);
        assert!(matches!(result, AccessDecision::Denied(_)));
    }

    #[test]
    fn capability_chain_global_deny_denies() {
        let mut chain = CapabilityChain::default();
        chain.global_deny.tools.push("my_tool".into());
        let result = chain.check("my_tool", &CapabilityAction::Read);
        assert!(matches!(result, AccessDecision::Denied(_)));
    }

    #[test]
    fn capability_chain_global_allow_allows() {
        let mut chain = CapabilityChain::default();
        chain.global_default = AccessDefault::Allow;
        let result = chain.check("unknown", &CapabilityAction::Read);
        assert!(matches!(result, AccessDecision::Allowed(_)));
    }

    #[test]
    fn capability_chain_global_ask_returns_needs_approval() {
        let mut chain = CapabilityChain::default();
        chain.global_default = AccessDefault::Ask;
        let result = chain.check("unknown", &CapabilityAction::Read);
        assert!(matches!(result, AccessDecision::NeedsApproval(_)));
    }

    #[test]
    fn capability_chain_mode_none_denies_immediately() {
        let mut chain = CapabilityChain::default();
        chain.mode = AccessMode::None;
        let result = chain.check("anything", &CapabilityAction::Read);
        assert!(matches!(result, AccessDecision::Denied(_)));
    }

    #[test]
    fn capability_set_matches_tools_exactly() {
        let mut set = CapabilitySet::default();
        set.tools.push("read_file".into());
        assert!(set.matches("read_file", &CapabilityAction::Read));
        assert!(!set.matches("write_file", &CapabilityAction::Read));
    }

    #[test]
    fn capability_set_matches_hosts_by_contains() {
        let mut set = CapabilitySet::default();
        set.hosts.push("api.example.com".into());
        assert!(set.matches("https://api.example.com/v1", &CapabilityAction::Network));
        assert!(!set.matches("https://other.com", &CapabilityAction::Network));
    }

    #[test]
    fn capability_set_matches_fs_paths_by_prefix() {
        let mut set = CapabilitySet::default();
        set.fs_paths.push("/data".into());
        assert!(set.matches("/data/plugins/file.txt", &CapabilityAction::Read));
        assert!(!set.matches("/other/file.txt", &CapabilityAction::Read));
    }

    #[test]
    fn capability_set_matches_env_vars_exactly() {
        let mut set = CapabilitySet::default();
        set.env_vars.push("HOME".into());
        assert!(set.matches("HOME", &CapabilityAction::Read));
        assert!(!set.matches("PATH", &CapabilityAction::Read));
    }

    #[test]
    fn capability_set_matches_shell_commands() {
        let mut set = CapabilitySet::default();
        set.shell_commands.push("ls".into());
        assert!(set.matches("ls", &CapabilityAction::Execute));
        assert!(!set.matches("rm", &CapabilityAction::Execute));
    }

    #[test]
    fn capability_set_matches_config_keys() {
        let mut set = CapabilitySet::default();
        set.config_keys.push("theme".into());
        assert!(set.matches("theme", &CapabilityAction::Config));
        assert!(!set.matches("other", &CapabilityAction::Config));
    }

    #[test]
    fn capability_set_matches_providers() {
        let mut set = CapabilitySet::default();
        set.providers.push("openai".into());
        assert!(set.matches("openai", &CapabilityAction::Provider));
        assert!(!set.matches("anthropic", &CapabilityAction::Provider));
    }

    #[test]
    fn access_decision_debug() {
        let allowed = AccessDecision::Allowed("ok".into());
        let denied = AccessDecision::Denied("no".into());
        let ask = AccessDecision::NeedsApproval("maybe".into());
        assert_eq!(format!("{allowed:?}"), r#"Allowed("ok")"#);
        assert_eq!(format!("{denied:?}"), r#"Denied("no")"#);
        assert_eq!(format!("{ask:?}"), r#"NeedsApproval("maybe")"#);
    }

    #[test]
    fn capability_action_display() {
        assert_eq!(CapabilityAction::Read.to_string(), "read");
        assert_eq!(CapabilityAction::Write.to_string(), "write");
        assert_eq!(CapabilityAction::Execute.to_string(), "execute");
        assert_eq!(CapabilityAction::Network.to_string(), "network");
        assert_eq!(CapabilityAction::Config.to_string(), "config");
        assert_eq!(CapabilityAction::Session.to_string(), "session");
        assert_eq!(CapabilityAction::Provider.to_string(), "provider");
    }

    #[test]
    fn access_default_serde() {
        let json = serde_json::to_string(&AccessDefault::Deny).unwrap();
        assert_eq!(json, "\"deny\"");
        let deserialized: AccessDefault = serde_json::from_str("\"allow\"").unwrap();
        assert_eq!(deserialized, AccessDefault::Allow);
    }

    #[test]
    fn access_mode_serde() {
        let json = serde_json::to_string(&AccessMode::Trusted).unwrap();
        assert_eq!(json, "\"trusted\"");
        let deserialized: AccessMode = serde_json::from_str("\"interactive\"").unwrap();
        assert_eq!(deserialized, AccessMode::Interactive);
    }

    #[test]
    fn plugin_config_defaults_are_empty() {
        let cfg = PluginConfig::default();
        assert!(cfg.enable.is_empty());
        assert!(cfg.disable.is_empty());
        assert!(cfg.mode.is_none());
        assert!(cfg.fail_closed.is_none());
        assert!(cfg.sources.is_none());
        assert!(cfg.settings.is_empty());
        assert!(cfg.features.is_empty());
        assert!(cfg.plugins.is_empty());
        assert!(!cfg.skip_hooks);
        assert!(!cfg.force_deny);
    }

    #[test]
    fn plugin_config_serde_roundtrip() {
        let cfg = PluginConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: PluginConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.enable, cfg.enable);
        assert_eq!(deserialized.mode, cfg.mode);
    }

    #[test]
    fn plugin_config_custom_fields_roundtrip() {
        let mut cfg = PluginConfig::default();
        cfg.enable = vec!["my-plugin".into()];
        cfg.disable = vec!["bad-plugin".into()];
        cfg.mode = Some("strict".into());
        cfg.fail_closed = Some(true);
        cfg.skip_hooks = true;
        cfg.force_deny = true;
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let deserialized: PluginConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.enable, vec!["my-plugin"]);
        assert_eq!(deserialized.disable, vec!["bad-plugin"]);
        assert_eq!(deserialized.mode, Some("strict".into()));
        assert_eq!(deserialized.fail_closed, Some(true));
        assert!(deserialized.skip_hooks);
        assert!(deserialized.force_deny);
    }

    fn with_env_var(name: &str, value: &str, f: impl FnOnce()) {
        // SAFETY: test-only env manipulation
        unsafe { std::env::set_var(name, value) }
        f();
        unsafe { std::env::remove_var(name) }
    }

    #[test]
    fn plugin_config_apply_env_overrides_disabled() {
        with_env_var("JCODE_DISABLE_PLUGINS", "true", || {
            let mut cfg = PluginConfig::default();
            cfg.apply_env_overrides();
            assert_eq!(cfg.mode, Some("none".into()));
        });
    }

    #[test]
    fn plugin_config_apply_env_overrides_skip() {
        with_env_var("JCODE_SKIP_PLUGINS", "1", || {
            let mut cfg = PluginConfig::default();
            cfg.apply_env_overrides();
            assert!(cfg.skip_hooks);
        });
    }

    #[test]
    fn plugin_config_apply_env_overrides_mode() {
        with_env_var("JCODE_PLUGIN_MODE", "interactive", || {
            let mut cfg = PluginConfig::default();
            cfg.apply_env_overrides();
            assert_eq!(cfg.mode, Some("interactive".into()));
        });
    }

    #[test]
    fn plugin_config_apply_env_overrides_team_worker() {
        with_env_var("JCODE_TEAM_WORKER", "1", || {
            let mut cfg = PluginConfig::default();
            cfg.apply_env_overrides();
            assert!(cfg.force_deny);
        });
    }

    #[test]
    fn plugin_source_npm_serde() {
        let src = crate::config::PluginSourceConfig::Npm {
            package: "my-plugin".into(),
            version: Some("1.0.0".into()),
        };
        let json = serde_json::to_string(&src).unwrap();
        assert!(json.contains("\"npm\""));
        let deserialized: crate::config::PluginSourceConfig = serde_json::from_str(&json).unwrap();
        match deserialized {
            crate::config::PluginSourceConfig::Npm { package, version } => {
                assert_eq!(package, "my-plugin");
                assert_eq!(version, Some("1.0.0".into()));
            }
            _ => panic!("expected Npm variant"),
        }
    }

    #[test]
    fn plugin_source_file_serde() {
        let src = crate::config::PluginSourceConfig::File {
            path: "/tmp/plugin.wasm".into(),
        };
        let json = serde_json::to_string(&src).unwrap();
        let deserialized: crate::config::PluginSourceConfig = serde_json::from_str(&json).unwrap();
        match deserialized {
            crate::config::PluginSourceConfig::File { path } => {
                assert_eq!(path, "/tmp/plugin.wasm")
            }
            _ => panic!("expected File variant"),
        }
    }

    #[test]
    fn plugin_source_directory_serde() {
        let src = crate::config::PluginSourceConfig::Directory {
            path: "/plugins".into(),
        };
        let json = serde_json::to_string(&src).unwrap();
        let deserialized: crate::config::PluginSourceConfig = serde_json::from_str(&json).unwrap();
        match deserialized {
            crate::config::PluginSourceConfig::Directory { path } => assert_eq!(path, "/plugins"),
            _ => panic!("expected Directory variant"),
        }
    }

    #[test]
    fn is_valid_package_name_accepts_valid() {
        assert!(is_valid_package_name("my-plugin"));
        assert!(is_valid_package_name("@scope/pkg"));
        assert!(is_valid_package_name("a0"));
    }

    #[test]
    fn is_valid_package_name_rejects_invalid() {
        assert!(!is_valid_package_name(".."));
        assert!(!is_valid_package_name("a;b"));
        assert!(!is_valid_package_name("a|b"));
        assert!(!is_valid_package_name(""));
    }

    #[test]
    fn sanitize_name_replaces_slashes_and_at_sign() {
        assert_eq!(sanitize_name("@scope/pkg"), "scope__pkg");
        assert_eq!(sanitize_name("simple-name"), "simple-name");
    }

    #[test]
    fn plugin_per_plugin_config_defaults() {
        let pc = config::PluginPerPluginConfig::default();
        assert!(pc.enable.is_none());
        assert!(pc.timeout_ms.is_none());
    }

    #[test]
    fn plugin_per_plugin_config_serde() {
        let pc = config::PluginPerPluginConfig {
            enable: Some(true),
            timeout_ms: Some(5000),
        };
        let json = serde_json::to_string(&pc).unwrap();
        let deserialized: config::PluginPerPluginConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.enable, Some(true));
        assert_eq!(deserialized.timeout_ms, Some(5000));
    }

    #[test]
    fn plugin_event_all_returns_27_variants() {
        let all = PluginEvent::all();
        assert_eq!(all.len(), 27);
    }

    #[test]
    fn plugin_event_count_matches_variants() {
        assert_eq!(PluginEvent::COUNT, 27);
    }

    #[test]
    fn plugin_event_serde_all_variants() {
        for event in PluginEvent::all() {
            let json = serde_json::to_string(&event).unwrap();
            let deserialized: PluginEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, event);
        }
    }

    #[test]
    fn plugin_event_discriminants() {
        assert_eq!(PluginEvent::PreToolUse as u32, 0);
        assert_eq!(PluginEvent::PostToolUse as u32, 1);
        assert_eq!(PluginEvent::SessionStart as u32, 5);
        assert_eq!(PluginEvent::Stop as u32, 26);
        assert_eq!(PluginEvent::Notification as u32, 27);
    }

    #[test]
    fn plugin_event_serde_rename() {
        let json = "\"PreToolUse\"";
        let event: PluginEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event, PluginEvent::PreToolUse);
        let json = "\"SessionEnd\"";
        let event: PluginEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event, PluginEvent::SessionEnd);
    }

    #[test]
    fn event_input_pre_tool_use() {
        let input = EventInput::PreToolUse {
            tool_name: "read_file".into(),
            tool_input: serde_json::json!({"path": "/tmp/test"}),
            session_id: "sess_1".into(),
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(json.contains("PreToolUse"));
        let deserialized: EventInput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventInput::PreToolUse { tool_name, .. } => assert_eq!(tool_name, "read_file"),
            _ => panic!("expected PreToolUse"),
        }
    }

    #[test]
    fn event_input_post_tool_use() {
        let input = EventInput::PostToolUse {
            tool_name: "write_file".into(),
            tool_input: serde_json::json!({"content": "hi"}),
            tool_output: serde_json::json!({"success": true}),
            duration_ms: 100,
            success: true,
            session_id: "sess_1".into(),
        };
        let json = serde_json::to_string(&input).unwrap();
        let deserialized: EventInput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventInput::PostToolUse {
                tool_name, success, ..
            } => {
                assert_eq!(tool_name, "write_file");
                assert!(success);
            }
            _ => panic!("expected PostToolUse"),
        }
    }

    #[test]
    fn event_input_session_start() {
        let input = EventInput::SessionStart {
            session_id: "sess_1".into(),
            project_dir: "/home/user/project".into(),
            model: "claude-4".into(),
            provider: "anthropic".into(),
        };
        let json = serde_json::to_string(&input).unwrap();
        let deserialized: EventInput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventInput::SessionStart {
                model, provider, ..
            } => {
                assert_eq!(model, "claude-4");
                assert_eq!(provider, "anthropic");
            }
            _ => panic!("expected SessionStart"),
        }
    }

    #[test]
    fn event_input_stop() {
        let input = EventInput::Stop {
            session_id: "sess_1".into(),
            reason: "user_request".into(),
        };
        let json = serde_json::to_string(&input).unwrap();
        let deserialized: EventInput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventInput::Stop { reason, .. } => assert_eq!(reason, "user_request"),
            _ => panic!("expected Stop"),
        }
    }

    #[test]
    fn event_input_notification() {
        let input = EventInput::Notification {
            level: "info".into(),
            message: "hello".into(),
            session_id: Some("sess_1".into()),
        };
        let json = serde_json::to_string(&input).unwrap();
        let deserialized: EventInput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventInput::Notification { level, message, .. } => {
                assert_eq!(level, "info");
                assert_eq!(message, "hello");
            }
            _ => panic!("expected Notification"),
        }
    }

    #[test]
    fn event_output_pre_tool_use() {
        let output = EventOutput::PreToolUse {
            block: Some("reason".into()),
            modified_input: Some(serde_json::json!({"key": "val"})),
        };
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: EventOutput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventOutput::PreToolUse {
                block,
                modified_input,
                ..
            } => {
                assert_eq!(block, Some("reason".into()));
                assert!(modified_input.is_some());
            }
            _ => panic!("expected PreToolUse"),
        }
    }

    #[test]
    fn event_output_permission_request() {
        let output = EventOutput::PermissionRequest {
            decision: Some(PermissionDecision::Allow),
            message: Some("approved".into()),
        };
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: EventOutput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventOutput::PermissionRequest {
                decision, message, ..
            } => {
                assert_eq!(decision, Some(PermissionDecision::Allow));
                assert_eq!(message, Some("approved".into()));
            }
            _ => panic!("expected PermissionRequest"),
        }
    }

    #[test]
    fn event_output_agent_start() {
        let output = EventOutput::AgentStart {
            additional_system_prompt: vec!["be concise".into()],
        };
        let json = serde_json::to_string(&output).unwrap();
        let deserialized: EventOutput = serde_json::from_str(&json).unwrap();
        match deserialized {
            EventOutput::AgentStart {
                additional_system_prompt,
            } => {
                assert_eq!(additional_system_prompt, vec!["be concise"]);
            }
            _ => panic!("expected AgentStart"),
        }
    }

    #[test]
    fn permission_decision_serde() {
        let json = serde_json::to_string(&PermissionDecision::Deny).unwrap();
        assert_eq!(json, "\"deny\"");
        let deserialized: PermissionDecision = serde_json::from_str("\"ask\"").unwrap();
        assert_eq!(deserialized, PermissionDecision::Ask);
    }

    #[test]
    fn handler_result_default() {
        let result = HandlerResult::default();
        assert_eq!(result.action, HandlerAction::Continue);
        assert!(result.output.is_none());
        assert!(result.error.is_none());
    }

    #[test]
    fn handler_result_custom() {
        let result = HandlerResult {
            action: HandlerAction::Block("not allowed".into()),
            output: Some(serde_json::json!({"status": "blocked"})),
            error: Some("denied".into()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: HandlerResult = serde_json::from_str(&json).unwrap();
        match deserialized.action {
            HandlerAction::Block(msg) => assert_eq!(msg, "not allowed"),
            _ => panic!("expected Block"),
        }
        assert!(deserialized.output.is_some());
        assert_eq!(deserialized.error.unwrap(), "denied");
    }

    #[test]
    fn handler_action_serde() {
        for action in &[
            HandlerAction::Continue,
            HandlerAction::Allow,
            HandlerAction::Deny,
            HandlerAction::Error,
        ] {
            let json = serde_json::to_string(action).unwrap();
            let deserialized: HandlerAction = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, *action);
        }
    }

    #[test]
    fn handler_action_block_serde() {
        let action = HandlerAction::Block("reason".into());
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: HandlerAction = serde_json::from_str(&json).unwrap();
        match deserialized {
            HandlerAction::Block(msg) => assert_eq!(msg, "reason"),
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn plugin_error_display_invalid_manifest() {
        let err = PluginError::InvalidManifest("missing field".into());
        assert_eq!(err.to_string(), "Plugin manifest is invalid: missing field");
    }

    #[test]
    fn plugin_error_display_not_found() {
        let err = PluginError::NotFound("my-plugin".into());
        assert_eq!(err.to_string(), "Plugin not found: my-plugin");
    }

    #[test]
    fn plugin_error_display_load() {
        let err = PluginError::Load("permission denied".into());
        assert_eq!(err.to_string(), "Failed to load plugin: permission denied");
    }

    #[test]
    fn plugin_error_display_runtime() {
        let err = PluginError::Runtime("crash".into());
        assert_eq!(err.to_string(), "Plugin runtime error: crash");
    }

    #[test]
    fn plugin_error_display_eval() {
        let err = PluginError::Eval("syntax error".into());
        assert_eq!(err.to_string(), "QuickJS evaluation error: syntax error");
    }

    #[test]
    fn plugin_error_display_quickjs() {
        let err = PluginError::QuickJs("OOM".into());
        assert_eq!(err.to_string(), "QuickJS runtime error: OOM");
    }

    #[test]
    fn plugin_error_display_transpile() {
        let err = PluginError::Transpile("unexpected token".into());
        assert_eq!(err.to_string(), "SWC transpilation error: unexpected token");
    }

    #[test]
    fn plugin_error_display_timeout() {
        let dur = std::time::Duration::from_secs(30);
        let err = PluginError::Timeout(dur);
        assert_eq!(err.to_string(), "Plugin operation timed out after 30s");
    }

    #[test]
    fn plugin_error_display_capability() {
        let err = PluginError::Capability("network access".into());
        assert_eq!(err.to_string(), "Capability denied: network access");
    }

    #[test]
    fn plugin_error_display_npm() {
        let err = PluginError::Npm("404 not found".into());
        assert_eq!(err.to_string(), "npm error: 404 not found");
    }

    #[test]
    fn plugin_error_display_other() {
        let err = PluginError::Other("something went wrong".into());
        assert_eq!(err.to_string(), "something went wrong");
    }

    #[test]
    fn plugin_error_io_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: PluginError = io_err.into();
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn plugin_error_serde_conversion() {
        let serde_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let err: PluginError = serde_err.into();
        assert!(err.to_string().contains("Serde error"));
    }

    #[test]
    fn plugin_manifest_default_values() {
        let m = PluginManifest::default();
        assert_eq!(m.name, "");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.kind, PluginKind::Server);
        assert!(m.description.is_none());
        assert!(m.author.is_none());
        assert!(m.license.is_none());
        assert!(m.tags.is_empty());
        assert!(m.features.is_empty());
        assert!(m.settings.is_empty());
    }

    #[test]
    fn plugin_manifest_from_package_json_requires_jcode_or_pi_field() {
        let json = serde_json::json!({"name": "test"});
        let result = PluginManifest::from_package_json(&json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing"));
    }

    #[test]
    fn plugin_manifest_from_package_json_with_jcode_field() {
        let json = serde_json::json!({
            "name": "test-plugin",
            "jcode": {
                "name": "test-plugin",
                "package_name": "test-plugin",
                "version": "1.0.0",
                "kind": "server"
            }
        });
        let manifest = PluginManifest::from_package_json(&json).unwrap();
        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.kind, PluginKind::Server);
    }

    #[test]
    fn plugin_manifest_from_package_json_with_pi_field() {
        let json = serde_json::json!({
            "name": "test",
            "pi": {
                "name": "test",
                "package_name": "test",
                "version": "2.0.0",
                "kind": "tui"
            }
        });
        let manifest = PluginManifest::from_package_json(&json).unwrap();
        assert_eq!(manifest.version, "2.0.0");
        assert_eq!(manifest.kind, PluginKind::Tui);
    }

    #[test]
    fn plugin_manifest_from_package_json_jcode_takes_precedence() {
        let json = serde_json::json!({
            "jcode": { "name": "a", "package_name": "a", "version": "1.0.0" },
            "pi": { "name": "b", "package_name": "b", "version": "2.0.0" }
        });
        let manifest = PluginManifest::from_package_json(&json).unwrap();
        assert_eq!(manifest.version, "1.0.0");
    }

    #[test]
    fn plugin_manifest_serde_roundtrip() {
        let mut manifest = PluginManifest::default();
        manifest.name = "my-plugin".into();
        manifest.package_name = "my-plugin".into();
        manifest.version = "1.2.3".into();
        manifest.description = Some("A test plugin".into());
        manifest.author = Some("author".into());
        manifest.kind = PluginKind::Both;
        manifest.tags = vec!["test".into(), "demo".into()];
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let deserialized: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "my-plugin");
        assert_eq!(deserialized.version, "1.2.3");
        assert_eq!(deserialized.description.unwrap(), "A test plugin");
        assert_eq!(deserialized.author.unwrap(), "author");
        assert_eq!(deserialized.kind, PluginKind::Both);
        assert_eq!(deserialized.tags, vec!["test", "demo"]);
    }

    #[test]
    fn plugin_kind_default_is_server() {
        assert_eq!(PluginKind::default(), PluginKind::Server);
    }

    #[test]
    fn plugin_entry_default() {
        let entry = PluginEntry::default();
        assert!(entry.server.is_none());
        assert!(entry.tui.is_none());
        assert!(entry.both.is_none());
    }

    #[test]
    fn plugin_entry_serde() {
        let entry = PluginEntry {
            server: Some("dist/server.js".into()),
            tui: Some("dist/tui.js".into()),
            both: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: PluginEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.server.unwrap(), "dist/server.js");
        assert!(deserialized.both.is_none());
    }

    #[test]
    fn plugin_capabilities_default() {
        let caps = PluginCapabilities::default();
        assert!(caps.fs_read.is_empty());
        assert!(caps.fs_write.is_empty());
        assert!(!caps.shell);
        assert!(!caps.register_tools);
        assert!(!caps.register_commands);
        assert!(!caps.llm_access);
        assert!(!caps.session_access);
        assert!(!caps.read_config);
        assert!(!caps.write_config);
    }

    #[test]
    fn plugin_capabilities_serde() {
        let caps = PluginCapabilities {
            fs_read: vec!["/data".into()],
            network: vec!["api.example.com".into()],
            shell: true,
            register_tools: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&caps).unwrap();
        let deserialized: PluginCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.fs_read, vec!["/data"]);
        assert_eq!(deserialized.network, vec!["api.example.com"]);
        assert!(deserialized.shell);
        assert!(deserialized.register_tools);
        assert!(!deserialized.register_commands);
    }

    #[test]
    fn plugin_feature_defaults() {
        let feature = PluginFeature {
            description: "A feature".into(),
            default: false,
            entry: None,
            additional_capabilities: None,
        };
        let json = serde_json::to_string(&feature).unwrap();
        let deserialized: PluginFeature = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.description, "A feature");
        assert!(!deserialized.default);
    }

    #[test]
    fn setting_schema_string_serde() {
        let schema = SettingSchema::String {
            description: "name".into(),
            default: Some("default".into()),
            secret: true,
            env: Some("MY_VAR".into()),
            pattern: Some("^[a-z]+$".into()),
            max_length: Some(100),
        };
        let json = serde_json::to_string_pretty(&schema).unwrap();
        let deserialized: SettingSchema = serde_json::from_str(&json).unwrap();
        match deserialized {
            SettingSchema::String {
                description,
                secret,
                ..
            } => {
                assert_eq!(description, "name");
                assert!(secret);
            }
            _ => panic!("expected String variant"),
        }
    }

    #[test]
    fn setting_schema_number_serde() {
        let schema = SettingSchema::Number {
            description: "count".into(),
            default: Some(42.0),
            min: Some(0.0),
            max: Some(100.0),
        };
        let json = serde_json::to_string(&schema).unwrap();
        let deserialized: SettingSchema = serde_json::from_str(&json).unwrap();
        match deserialized {
            SettingSchema::Number {
                description,
                default: d,
                ..
            } => {
                assert_eq!(description, "count");
                assert_eq!(d, Some(42.0));
            }
            _ => panic!("expected Number variant"),
        }
    }

    #[test]
    fn setting_schema_boolean_serde() {
        let schema = SettingSchema::Boolean {
            description: "enabled".into(),
            default: Some(true),
        };
        let json = serde_json::to_string(&schema).unwrap();
        let deserialized: SettingSchema = serde_json::from_str(&json).unwrap();
        match deserialized {
            SettingSchema::Boolean {
                description,
                default: d,
            } => {
                assert_eq!(description, "enabled");
                assert_eq!(d, Some(true));
            }
            _ => panic!("expected Boolean variant"),
        }
    }

    #[test]
    fn setting_schema_enum_serde() {
        let schema = SettingSchema::Enum {
            description: "mode".into(),
            default: Some("fast".into()),
            values: vec!["fast".into(), "slow".into()],
        };
        let json = serde_json::to_string(&schema).unwrap();
        let deserialized: SettingSchema = serde_json::from_str(&json).unwrap();
        match deserialized {
            SettingSchema::Enum { values, .. } => {
                assert_eq!(values.len(), 2)
            }
            _ => panic!("expected Enum variant"),
        }
    }

    #[test]
    fn setting_schema_object_serde() {
        let inner = SettingSchema::Boolean {
            description: "flag".into(),
            default: None,
        };
        let schema = SettingSchema::Object {
            description: "config".into(),
            default: None,
            properties: [("nested".into(), inner)].into(),
        };
        let json = serde_json::to_string(&schema).unwrap();
        let deserialized: SettingSchema = serde_json::from_str(&json).unwrap();
        match deserialized {
            SettingSchema::Object { properties, .. } => {
                assert!(properties.contains_key("nested"))
            }
            _ => panic!("expected Object variant"),
        }
    }

    #[test]
    fn plugin_engines_default() {
        let engines = PluginEngines::default();
        assert!(engines.jcode.is_none());
    }

    #[test]
    fn plugin_engines_serde() {
        let engines = PluginEngines {
            jcode: Some(">=0.9.0".into()),
        };
        let json = serde_json::to_string(&engines).unwrap();
        let deserialized: PluginEngines = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.jcode.unwrap(), ">=0.9.0");
    }

    #[test]
    fn to_json_serializes_pretty() {
        let json = serde::to_json(&serde_json::json!({"key": "value"})).unwrap();
        assert!(json.contains("\"key\""));
        assert!(json.contains("value"));
    }

    #[test]
    fn from_json_deserializes() {
        let json = r#"{"key": "value"}"#;
        let value: serde_json::Value = serde::from_json(json).unwrap();
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn to_value_returns_json_value() {
        let value = serde::to_value(&"hello").unwrap();
        assert_eq!(value, serde_json::json!("hello"));
    }

    #[test]
    fn serde_roundtrip_plugin_config() {
        let cfg = PluginConfig::default();
        let json = serde::to_json(&cfg).unwrap();
        let deserialized: PluginConfig = serde::from_json(&json).unwrap();
        assert_eq!(deserialized.enable, cfg.enable);
        assert_eq!(deserialized.mode, cfg.mode);
    }

    #[test]
    fn serde_roundtrip_event_input() {
        let input = EventInput::PreToolUse {
            tool_name: "test".into(),
            tool_input: serde_json::json!({}),
            session_id: "s".into(),
        };
        let json = serde::to_json(&input).unwrap();
        let deserialized: EventInput = serde::from_json(&json).unwrap();
        match deserialized {
            EventInput::PreToolUse { tool_name, .. } => {
                assert_eq!(tool_name, "test")
            }
            _ => panic!("expected PreToolUse"),
        }
    }

    #[test]
    fn serde_roundtrip_event_output() {
        let output = EventOutput::PreToolUse {
            block: None,
            modified_input: None,
        };
        let json = serde::to_json(&output).unwrap();
        let deserialized: EventOutput = serde::from_json(&json).unwrap();
        match deserialized {
            EventOutput::PreToolUse { .. } => {}
            _ => panic!("expected PreToolUse"),
        }
    }

    #[test]
    fn integration_plugin_id_as_plugin_origin_consistency() {
        let id = PluginId::npm("my-plugin");
        let origin = PluginOrigin::NpmPackage {
            name: "my-plugin".into(),
            version: "1.0.0".into(),
        };
        assert_eq!(id.short_name(), "my-plugin");
        match origin {
            PluginOrigin::NpmPackage { ref name, .. } => {
                assert_eq!(name, "my-plugin")
            }
            _ => panic!("expected NpmPackage"),
        }
    }

    #[test]
    fn integration_capability_use_with_manifest_capabilities() {
        let mut caps = PluginCapabilities::default();
        caps.network = vec!["api.example.com".into()];
        let mut chain = CapabilityChain::default();
        chain.allow_list.hosts = caps.network.clone();
        assert!(matches!(
            chain.check("api.example.com", &CapabilityAction::Network),
            AccessDecision::Allowed(_)
        ));
    }

    #[test]
    fn integration_manifest_with_capabilities_roundtrip() {
        let mut manifest = PluginManifest::default();
        manifest.name = "demo".into();
        manifest.package_name = "demo".into();
        manifest.capabilities.fs_read = vec!["/tmp".into()];
        manifest.capabilities.network = vec!["localhost".into()];
        manifest.capabilities.shell = true;
        let json = serde_json::to_string(&manifest).unwrap();
        let deserialized: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.capabilities.fs_read, vec!["/tmp"]);
        assert_eq!(deserialized.capabilities.network, vec!["localhost"]);
        assert!(deserialized.capabilities.shell);
    }

    #[test]
    fn integration_event_to_config() {
        let _input = EventInput::PreToolUse {
            tool_name: "npm_install".into(),
            tool_input: serde_json::json!({"package": "test"}),
            session_id: "sess_1".into(),
        };
        let mut cfg = PluginConfig::default();
        cfg.mode = Some("interactive".into());
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("interactive"));
        let back: PluginConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mode.as_deref(), Some("interactive"));
    }

    #[test]
    fn integration_error_chain_compatibility() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
        let plugin_err: PluginError = io_err.into();
        let display = plugin_err.to_string();
        assert!(display.contains("I/O error") || display.contains("permission denied"));
    }

    // -----------------------------------------------------------------------
    // CapabilityChainV2 tests (5-layer chain)
    // -----------------------------------------------------------------------

    #[test]
    fn capability_chain_v2_layer1_plugin_deny_wins_over_permissive() {
        let mut chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Permissive,
            global_default: None,
        };
        chain.plugin_deny.tools.push("danger".into());
        let result = chain.check("danger", &CapabilityAction::Execute);
        match result {
            AccessDecisionV2::Deny { reason: _, layer } => assert_eq!(layer, 1),
            other => panic!("expected Deny layer 1, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_layer2_global_deny_wins_over_permissive() {
        let mut chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Permissive,
            global_default: None,
        };
        chain.global_deny.tools.push("blocked".into());
        let result = chain.check("blocked", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Deny { reason: _, layer } => assert_eq!(layer, 2),
            other => panic!("expected Deny layer 2, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_layer3_plugin_allow_wins_over_strict() {
        let mut chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Strict,
            global_default: None,
        };
        chain.plugin_allow.tools.push("permitted".into());
        let result = chain.check("permitted", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Allow { reason: _, layer } => assert_eq!(layer, 3),
            other => panic!("expected Allow layer 3, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_layer4_global_allow_wins_over_strict() {
        let mut chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Strict,
            global_default: None,
        };
        chain.global_allow.tools.push("global_whitelist".into());
        let result = chain.check("global_whitelist", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Allow { reason: _, layer } => assert_eq!(layer, 4),
            other => panic!("expected Allow layer 4, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_layer5_strict_denies_unknown() {
        let chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Strict,
            global_default: None,
        };
        let result = chain.check("unknown", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Deny { reason: _, layer } => assert_eq!(layer, 5),
            other => panic!("expected Deny layer 5, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_layer5_permissive_allows_unknown() {
        let chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Permissive,
            global_default: None,
        };
        let result = chain.check("unknown", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Allow { reason: _, layer } => assert_eq!(layer, 5),
            other => panic!("expected Allow layer 5, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_disabled_mode_denies_everything() {
        let chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Disabled,
            global_default: None,
        };
        // Even an explicitly allowed resource gets denied
        let result = chain.check("anything", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Deny { reason: _, layer } => assert_eq!(layer, 5),
            other => panic!("expected Deny layer 5, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_prompt_mode_requires_approval() {
        let chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Prompt,
            global_default: None,
        };
        let result = chain.check("unknown", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::NeedsApproval { reason: _, layer } => assert_eq!(layer, 5),
            other => panic!("expected NeedsApproval layer 5, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_global_default_allow_overrides_prompt() {
        let chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Prompt,
            global_default: Some(AccessDefault::Allow),
        };
        let result = chain.check("unknown", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Allow { reason: _, layer } => assert_eq!(layer, 5),
            other => panic!("expected Allow layer 5, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_global_default_deny_overrides_permissive() {
        let chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Permissive,
            global_default: Some(AccessDefault::Deny),
        };
        let result = chain.check("unknown", &CapabilityAction::Read);
        match result {
            AccessDecisionV2::Deny { reason: _, layer } => assert_eq!(layer, 5),
            other => panic!("expected Deny layer 5, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_layer1_takes_precedence_over_all_layers() {
        let mut chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Permissive,
            global_default: Some(AccessDefault::Allow),
        };
        // Even a globally-allowed resource should be blocked by plugin_deny at layer 1
        chain.plugin_deny.tools.push("toxic".into());
        chain.global_allow.tools.push("toxic".into());
        let result = chain.check("toxic", &CapabilityAction::Execute);
        match result {
            AccessDecisionV2::Deny { reason: _, layer } => assert_eq!(layer, 1),
            other => panic!("expected Deny layer 1, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_layer2_takes_precedence_over_allow_layers() {
        let mut chain = CapabilityChainV2 {
            plugin_deny: CapabilitySet::default(),
            global_deny: CapabilitySet::default(),
            plugin_allow: CapabilitySet::default(),
            global_allow: CapabilitySet::default(),
            mode: PolicyMode::Permissive,
            global_default: Some(AccessDefault::Allow),
        };
        // global_deny at layer 2 should block even though plugin_allow at layer 3 would allow
        chain.global_deny.tools.push("risky".into());
        chain.plugin_allow.tools.push("risky".into());
        let result = chain.check("risky", &CapabilityAction::Write);
        match result {
            AccessDecisionV2::Deny { reason: _, layer } => assert_eq!(layer, 2),
            other => panic!("expected Deny layer 2, got {other:?}"),
        }
    }

    #[test]
    fn capability_chain_v2_policy_mode_default() {
        assert_eq!(PolicyMode::default(), PolicyMode::Prompt);
    }
}
