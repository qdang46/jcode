use super::best_of_n::{
    BestOfNRunner, build_allowed_tool_set, build_file_touch_preview, format_show_result,
};
use super::{Registry, ToolContext};
use crate::provider::Provider;
use anyhow::Result;
use async_trait::async_trait;
use jcode_best_of_n::BestOfNConfig;
use jcode_best_of_n::BestOfNMode;
use jcode_best_of_n::config::TemperatureStrategyConfig;
use jcode_best_of_n::strategies;
use jcode_best_of_n::types::*;
use jcode_message_types::ToolDefinition;
use jcode_provider_core::EventStream;
use std::sync::Arc;

struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn complete(
        &self,
        _messages: &[crate::message::Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> Result<EventStream> {
        Err(anyhow::anyhow!(
            "MockProvider should not be called in tests"
        ))
    }

    fn name(&self) -> &str {
        "mock"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(MockProvider)
    }

    fn set_temperature(&self, _temperature: f32) -> Result<()> {
        Ok(())
    }
}

fn make_strategy(label: &str, temperature: f64) -> CandidateStrategy {
    CandidateStrategy {
        label: label.to_string(),
        temperature,
        model: None,
    }
}

fn make_file_diff(file_path: &str, old: &str, new: &str) -> FileDiff {
    let unified_diff = format!(
        "--- a/{}\n+++ b/{}\n@@ -1 +1 @@\n-{}\n+{}\n",
        file_path, file_path, old, new
    );
    FileDiff {
        file_path: file_path.to_string(),
        unified_diff,
        old_content: old.to_string(),
        new_content: new.to_string(),
        is_new_file: false,
    }
}

#[test]
fn test_build_file_touch_preview_returns_none_for_empty() {
    assert!(build_file_touch_preview("").is_none());
    assert!(build_file_touch_preview("   ").is_none());
}

#[test]
fn test_build_file_touch_preview_returns_preview() {
    let preview = build_file_touch_preview("-old\n+new\n context\n").unwrap();
    assert!(preview.contains("-old"));
    assert!(preview.contains("+new"));
}

#[test]
fn test_format_show_result_includes_run_id() {
    let result = BestOfNResult {
        run_id: RunId("test-run-123".to_string()),
        candidates: vec![],
        winner_index: None,
        winner: None,
        selection_reason: Some("no candidates".to_string()),
    };
    let output = format_show_result(&result);
    assert!(output.output.contains("test-run-123"));
    assert!(output.output.contains("no candidates"));
}

#[test]
fn test_format_show_result_lists_candidates() {
    let candidates = vec![
        CandidateDiff {
            candidate_id: CandidateId("cand-0".to_string()),
            strategy: make_strategy("precise", 0.3),
            status: CandidateStatus::Success,
            file_diffs: vec![make_file_diff("src/main.rs", "old", "new")],
            total_tokens: Some(500),
            error: None,
        },
        CandidateDiff {
            candidate_id: CandidateId("cand-1".to_string()),
            strategy: make_strategy("creative", 0.8),
            status: CandidateStatus::Failed,
            file_diffs: vec![],
            total_tokens: None,
            error: Some("timeout".to_string()),
        },
    ];
    let result = BestOfNResult {
        run_id: RunId("r1".to_string()),
        winner_index: Some(0),
        winner: Some(candidates[0].clone()),
        candidates,
        selection_reason: Some("precise won".to_string()),
    };
    let output = format_show_result(&result);
    assert!(output.output.contains("precise"));
    assert!(output.output.contains("creative"));
    assert!(output.output.contains("timeout"));
    assert!(output.output.contains("★"));
}

fn make_tool_context() -> ToolContext {
    ToolContext {
        session_id: "test".to_string(),
        message_id: "m1".to_string(),
        tool_call_id: "tc1".to_string(),
        working_dir: None,
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        execution_mode: super::ToolExecutionMode::AgentTurn,
        best_of_n_run_id: None,
        best_of_n_candidate_id: None,
    }
}

#[test]
fn test_apply_winner_no_winner_returns_error() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let config = BestOfNConfig {
            mode: BestOfNMode::Auto,
            ..BestOfNConfig::default()
        };
        let runner = BestOfNRunner::new(config);
        let result = BestOfNResult {
            run_id: RunId("r1".to_string()),
            candidates: vec![],
            winner_index: None,
            winner: None,
            selection_reason: Some("no winner".to_string()),
        };
        let registry = Registry::empty();
        let err = runner
            .apply_winner(
                &result,
                &vec!["src/main.rs".to_string()],
                &registry,
                &make_tool_context(),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no winner"));
    });
}

#[test]
fn test_apply_winner_empty_diffs_returns_ok() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let config = BestOfNConfig::default();
        let runner = BestOfNRunner::new(config);
        let winner_candidate = CandidateDiff {
            candidate_id: CandidateId("cand-0".to_string()),
            strategy: make_strategy("default", 0.5),
            status: CandidateStatus::Success,
            file_diffs: vec![],
            total_tokens: None,
            error: None,
        };
        let result = BestOfNResult {
            run_id: RunId("r1".to_string()),
            candidates: vec![winner_candidate.clone()],
            winner_index: Some(0),
            winner: Some(winner_candidate),
            selection_reason: Some("only candidate".to_string()),
        };
        let registry = Registry::empty();
        let output = runner
            .apply_winner(&result, &vec![], &registry, &make_tool_context())
            .await
            .unwrap();
        assert!(true);
    });
}

#[test]
fn test_build_allowed_tool_set_excludes_forbidden() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let provider: Arc<dyn Provider> = Arc::new(MockProvider);
        let registry = Registry::new(provider).await;
        let allowed = build_allowed_tool_set(&registry).await;

        assert!(!allowed.contains("edit"));
        assert!(!allowed.contains("write"));
        assert!(!allowed.contains("subagent"));
        assert!(!allowed.contains("batch"));
        assert!(!allowed.contains("best_of_n"));

        assert!(allowed.contains("propose_edit"));
        assert!(allowed.contains("propose_write"));
        assert!(allowed.contains("propose_hashline"));

        assert!(allowed.contains("read"));
        assert!(allowed.contains("grep"));
    });
}

#[test]
fn test_config_effective_count_default() {
    let config = BestOfNConfig::default();
    assert!(config.enabled());
    assert_eq!(config.effective_count(), 4);
}

#[test]
fn test_config_off_mode_disabled() {
    let mut config = BestOfNConfig::default();
    config.mode = BestOfNMode::Off;
    assert!(!config.enabled());
}

#[test]
fn test_strategy_temperature_spread() {
    let temps = TemperatureStrategyConfig {
        min: 0.2,
        max: 0.8,
        values: vec![],
    };
    let strategies = strategies::generate_strategies(4, &temps);
    assert_eq!(strategies.len(), 4);
    let temp_values: Vec<f64> = strategies.iter().map(|s| s.temperature).collect();
    assert!(temp_values.windows(2).all(|w| w[0] < w[1]));
    assert!((temp_values[0] - 0.2).abs() < 0.01);
    assert!((temp_values[3] - 0.8).abs() < 0.01);
}
