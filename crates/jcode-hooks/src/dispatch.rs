//! Parallel hook dispatch engine.
//!
//! Orchestrates concurrent execution of multiple hook handlers for a single
//! event using [`FuturesUnordered`] and a [`Semaphore`]-based concurrency cap.
//!
//! # Architecture
//!
//! 1. The caller provides an [`AggregatedInput`] (event + context) and a
//!    reference to the [`HookRegistry`].
//! 2. [`dispatch_hooks`] resolves matching handlers via the registry, then
//!    fans them out into a [`FuturesUnordered`] stream bounded by the
//!    configured semaphore permits.
//! 3. Each completed future yields a [`ClassifiedResult`] which is collected
//!    into [`DispatchStats`].
//! 4. For blocking events the collected results are fed through
//!    [`aggregate_decision`] to produce a single [`AggregatedDecision`]
//!    with precedence: **deny > ask > allow**.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use tokio::sync::Semaphore;

use crate::config::{HookEvent, HookHandlerConfig, HookSettings};
use crate::execute::{execute_single_hook, ExecuteError};
use crate::types::{AggregatedDecision, HookInput, HookMetrics, HookResult};

// ---------------------------------------------------------------------------
// Global metrics store
// ---------------------------------------------------------------------------

/// Global metrics store keyed by `"event_name::handler_label"`.
static HOOK_METRICS: LazyLock<Mutex<HashMap<String, HookMetrics>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// DispatchConfig
// ---------------------------------------------------------------------------

/// Configuration for a single dispatch run.
///
/// Derived from [`HookSettings`] but can be overridden per-call.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    /// Maximum number of hooks executed concurrently (default 10).
    pub max_concurrency: usize,
    /// Default per-handler timeout in seconds.
    pub timeout_secs: u64,
    /// If `true`, hook failures are treated as blocks (fail-closed).
    pub fail_closed: bool,
    /// If `true`, hooks are resolved but never actually executed (dry-run).
    pub dry_run: bool,
}

impl DispatchConfig {
    /// Build from the global [`HookSettings`].
    pub fn from_settings(settings: &HookSettings) -> Self {
        Self {
            max_concurrency: settings.max_concurrency,
            timeout_secs: settings.timeout_secs,
            fail_closed: settings.fail_closed,
            dry_run: settings.dry_run,
        }
    }
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 10,
            timeout_secs: 30,
            fail_closed: false,
            dry_run: false,
        }
    }
}

// ---------------------------------------------------------------------------
// ClassifiedResult
// ---------------------------------------------------------------------------

/// A single hook's execution result classified into a policy decision.
#[derive(Debug)]
pub struct ClassifiedResult {
    /// Label identifying the handler (command string, URL, agent id).
    pub handler_label: String,
    /// The classified outcome.
    pub outcome: ClassifiedOutcome,
    /// Wall-clock duration of the handler execution.
    pub duration: Duration,
}

/// Simplified outcome for a single hook.
#[derive(Debug)]
pub enum ClassifiedOutcome {
    /// Hook explicitly allowed the operation.
    Allow,
    /// Hook wants to ask the user.
    Ask { reason: String },
    /// Hook blocked / denied the operation.
    Deny { reason: String },
    /// Hook failed (timeout, crash, non-zero exit).
    /// Behaviour depends on [`DispatchConfig::fail_closed`].
    Failed { error: String },
}

// ---------------------------------------------------------------------------
// DispatchStats
// ---------------------------------------------------------------------------

/// Aggregated statistics collected during a dispatch run.
#[derive(Debug, Default)]
pub struct DispatchStats {
    /// Total number of handlers that were matched and dispatched.
    pub total_dispatched: u64,
    /// Number of handlers that completed successfully (any decision).
    pub completed: u64,
    /// Number of handlers that failed (timeout / crash / error).
    pub failed: u64,
    /// Number of handlers that timed out specifically.
    pub timed_out: u64,
    /// Number of handlers that returned "allow".
    pub allowed: u64,
    /// Number of handlers that returned "ask".
    pub asked: u64,
    /// Number of handlers that returned "deny".
    pub denied: u64,
    /// Per-handler results for post-mortem inspection.
    pub results: Vec<ClassifiedResult>,
    /// Total wall-clock time for the entire dispatch run.
    pub total_duration: Duration,
}

impl DispatchStats {
    /// Return `true` if every handler succeeded without failure.
    pub fn all_succeeded(&self) -> bool {
        self.failed == 0
    }

    /// Return `true` if at least one handler blocked the operation.
    pub fn any_denied(&self) -> bool {
        self.denied > 0
    }

    /// Return `true` if at least one handler wants to ask the user.
    pub fn any_asked(&self) -> bool {
        self.asked > 0
    }
}

// ---------------------------------------------------------------------------
// classify_decision
// ---------------------------------------------------------------------------

/// Classify a raw [`HookResult`] into a [`ClassifiedOutcome`].
///
/// # Rules
///
/// | HookResult           | ClassifiedOutcome |
/// |----------------------|-------------------|
/// | `Continue` with `decision = "allow"` | `Allow` |
/// | `Continue` with `decision = "ask"`   | `Ask`   |
/// | `Continue` (no decision / other)     | `Allow` |
/// | `Blocked`            | `Deny`          |
/// | `Failed`             | `Failed`        |
pub fn classify_decision(result: &HookResult) -> ClassifiedOutcome {
    match result {
        HookResult::Continue(output) => {
            match output.decision.as_deref() {
                Some("ask") => ClassifiedOutcome::Ask {
                    reason: output
                        .reason
                        .clone()
                        .or_else(|| output.stop_reason.clone())
                        .unwrap_or_default(),
                },
                Some("deny") => ClassifiedOutcome::Deny {
                    reason: output
                        .stop_reason
                        .clone()
                        .or_else(|| output.reason.clone())
                        .unwrap_or_else(|| "denied by hook".to_string()),
                },
                // "allow" or absent decision -- both mean "go ahead".
                _ => ClassifiedOutcome::Allow,
            }
        }
        HookResult::Blocked { reason, .. } => ClassifiedOutcome::Deny {
            reason: reason.clone(),
        },
        HookResult::Failed { error } => ClassifiedOutcome::Failed {
            error: error.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// aggregate_decision
// ---------------------------------------------------------------------------

/// Aggregate multiple [`ClassifiedOutcome`]s into a single
/// [`AggregatedDecision`] using precedence: **deny > ask > allow**.
///
/// - If any outcome is `Deny`, the result is `Deny` with the first
///   deny reason encountered.
/// - Else if any outcome is `Ask`, the result is `Ask` with all ask
///   reasons collected.
/// - Else the result is `Allow`.
///
/// `Failed` outcomes are **ignored** unless `fail_closed` is `true`,
/// in which case they are treated as `Deny`.
pub fn aggregate_decision(outcomes: &[ClassifiedOutcome], fail_closed: bool) -> AggregatedDecision {
    let mut ask_reasons: Vec<String> = Vec::new();
    let mut first_deny: Option<(String, &ClassifiedOutcome)> = None;

    for outcome in outcomes {
        match outcome {
            ClassifiedOutcome::Deny { reason } => {
                if first_deny.is_none() {
                    first_deny = Some((reason.clone(), outcome));
                }
            }
            ClassifiedOutcome::Ask { reason } => {
                ask_reasons.push(reason.clone());
            }
            ClassifiedOutcome::Failed { error } => {
                if fail_closed && first_deny.is_none() {
                    first_deny = Some((format!("hook failed (fail-closed): {}", error), outcome));
                }
            }
            ClassifiedOutcome::Allow => { /* no-op */ }
        }
    }

    if let Some((reason, _)) = first_deny {
        return AggregatedDecision::Deny {
            reason,
            source_hook: String::new(), // caller can enrich from stats
        };
    }

    if !ask_reasons.is_empty() {
        return AggregatedDecision::Ask {
            reasons: ask_reasons,
        };
    }

    AggregatedDecision::Allow
}

// ---------------------------------------------------------------------------
// dispatch_hooks  --  the main entry point
// ---------------------------------------------------------------------------

/// Dispatch all matching handlers for a single event in parallel.
///
/// # Arguments
///
/// * `event`     -- the [`HookEvent`] being triggered.
/// * `input`     -- the [`HookInput`] to pass to every handler.
/// * `handlers`  -- pre-filtered list of handlers (from the registry's
///   `get_matching` call).
/// * `config`    -- dispatch configuration (concurrency, timeouts, policy).
///
/// # Returns
///
/// A [`DispatchStats`] containing per-handler results and the aggregate
/// [`AggregatedDecision`].
///
/// # Concurrency
///
/// Handlers are executed inside a [`FuturesUnordered`] stream.  A shared
/// [`Semaphore`] with `config.max_concurrency` permits ensures that at most
/// N handlers run simultaneously.
pub async fn dispatch_hooks(
    event: &HookEvent,
    input: &HookInput,
    handlers: &[&HookHandlerConfig],
    config: &DispatchConfig,
) -> DispatchStats {
    let start = Instant::now();

    let mut stats = DispatchStats {
        total_dispatched: handlers.len() as u64,
        ..Default::default()
    };

    // Nothing to do.
    if handlers.is_empty() {
        stats.total_duration = start.elapsed();
        return stats;
    }

    // Semaphore bounds concurrent handler execution.
    let semaphore = Arc::new(Semaphore::new(config.max_concurrency));

    // Atomic counters for lock-free stats updates from spawned tasks.
    let completed_count = Arc::new(AtomicU64::new(0));
    let failed_count = Arc::new(AtomicU64::new(0));
    let timed_out_count = Arc::new(AtomicU64::new(0));

    // Build the FuturesUnordered stream.
    let mut futures = FuturesUnordered::new();

    for handler in handlers {
        let permit = semaphore.clone();
        let timeout = effective_timeout(handler, config.timeout_secs);
        let dry_run = config.dry_run;
        let fail_closed = config.fail_closed;
        let handler_label = handler_label(handler);

        // Clone the input and handler so the future owns them.
        let input = input.clone();
        let handler = (*handler).clone();

        futures.push(async move {
            // Acquire semaphore permit before starting execution.
            let _permit = permit
                .acquire()
                .await
                .expect("semaphore closed unexpectedly");

            let handler_start = Instant::now();

            if dry_run {
                // In dry-run mode we skip execution and report "allow".
                return ClassifiedResult {
                    handler_label,
                    outcome: ClassifiedOutcome::Allow,
                    duration: handler_start.elapsed(),
                };
            }

            // Execute with a timeout wrapper.
            let result = tokio::time::timeout(
                Duration::from_secs(timeout),
                execute_single_hook(&handler, &input, timeout),
            )
            .await;

            let duration = handler_start.elapsed();

            match result {
                Ok(Ok(hook_result)) => {
                    let outcome = classify_decision(&hook_result);
                    ClassifiedResult {
                        handler_label,
                        outcome,
                        duration,
                    }
                }
                Ok(Err(exec_err)) => {
                    // Execution-level error (spawn failure, I/O error, etc.)
                    let error = format_execute_error(&exec_err);
                    ClassifiedResult {
                        handler_label,
                        outcome: if fail_closed {
                            ClassifiedOutcome::Deny {
                                reason: format!("execution error (fail-closed): {}", error),
                            }
                        } else {
                            ClassifiedOutcome::Failed { error }
                        },
                        duration,
                    }
                }
                Err(_elapsed) => {
                    // Timeout expired.
                    ClassifiedResult {
                        handler_label,
                        outcome: if fail_closed {
                            ClassifiedOutcome::Deny {
                                reason: format!("hook timed out after {}s (fail-closed)", timeout),
                            }
                        } else {
                            ClassifiedOutcome::Failed {
                                error: format!("timed out after {}s", timeout),
                            }
                        },
                        duration,
                    }
                }
            }
        });
    }

    let event_name = event.to_string();

    // Drain the stream, collecting results.
    while let Some(result) = futures.next().await {
        record_metrics(&event_name, &result);
        match &result.outcome {
            ClassifiedOutcome::Allow => {
                stats.allowed += 1;
                completed_count.fetch_add(1, Ordering::Relaxed);
            }
            ClassifiedOutcome::Ask { .. } => {
                stats.asked += 1;
                completed_count.fetch_add(1, Ordering::Relaxed);
            }
            ClassifiedOutcome::Deny { .. } => {
                stats.denied += 1;
                completed_count.fetch_add(1, Ordering::Relaxed);
            }
            ClassifiedOutcome::Failed { error } => {
                stats.failed += 1;
                failed_count.fetch_add(1, Ordering::Relaxed);
                if error.contains("timed out") {
                    timed_out_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        stats.results.push(result);
    }

    stats.completed = completed_count.load(Ordering::Relaxed);
    stats.failed = failed_count.load(Ordering::Relaxed);
    stats.timed_out = timed_out_count.load(Ordering::Relaxed);
    stats.total_duration = start.elapsed();

    stats
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive the effective timeout for a handler: per-handler override wins,
/// falling back to the global default.
fn effective_timeout(handler: &HookHandlerConfig, default_secs: u64) -> u64 {
    match handler {
        HookHandlerConfig::Command(cmd) => cmd.timeout_secs.unwrap_or(default_secs),
        HookHandlerConfig::Http(http) => http.timeout_secs.unwrap_or(default_secs),
        HookHandlerConfig::Agent(agent) => agent.timeout_secs,
        HookHandlerConfig::Plugin(plugin) => plugin.timeout_secs,
    }
}

/// Human-readable label for a handler (used in stats and error messages).
fn handler_label(handler: &HookHandlerConfig) -> String {
    match handler {
        HookHandlerConfig::Command(cmd) => {
            if cmd.command.len() > 60 {
                format!("cmd:{}...", &cmd.command[..57])
            } else {
                format!("cmd:{}", cmd.command)
            }
        }
        HookHandlerConfig::Http(http) => {
            if http.url.len() > 60 {
                format!("http:{}...", &http.url[..57])
            } else {
                format!("http:{}", http.url)
            }
        }
        HookHandlerConfig::Agent(agent) => format!("agent:{}", agent.agent_id),
        HookHandlerConfig::Plugin(plugin) => {
            if plugin.path.len() > 60 {
                format!("plugin:{}...", &plugin.path[..57])
            } else {
                format!("plugin:{}", plugin.path)
            }
        }
    }
}

/// Format an [`ExecuteError`] into a human-readable string.
fn format_execute_error(err: &ExecuteError) -> String {
    format!("{:#}", err)
}

// ---------------------------------------------------------------------------
// Metrics helpers
// ---------------------------------------------------------------------------

/// Record execution metrics for a single handler result.
fn record_metrics(event_name: &str, result: &ClassifiedResult) {
    let key = format!("{}::{}", event_name, result.handler_label);
    let mut store = HOOK_METRICS.lock().expect("metrics lock poisoned");
    let entry = store.entry(key).or_insert_with(|| HookMetrics {
        event_name: event_name.to_string(),
        handler_label: result.handler_label.clone(),
        execution_count: 0,
        failure_count: 0,
        blocked_count: 0,
        total_duration_ms: 0,
        avg_duration_ms: 0.0,
        last_execution: None,
        last_error: None,
    });

    let duration_ms = result.duration.as_millis() as u64;
    entry.execution_count += 1;
    entry.total_duration_ms += duration_ms;
    entry.avg_duration_ms = entry.total_duration_ms as f64 / entry.execution_count as f64;
    entry.last_execution = Some(Utc::now());

    match &result.outcome {
        ClassifiedOutcome::Failed { error } => {
            entry.failure_count += 1;
            entry.last_error = Some(error.clone());
        }
        ClassifiedOutcome::Deny { .. } => {
            entry.blocked_count += 1;
        }
        _ => {}
    }
}

/// Return a snapshot of all collected hook metrics.
///
/// Each entry is keyed by `"event_name::handler_label"`.
pub fn get_hook_metrics() -> HashMap<String, HookMetrics> {
    HOOK_METRICS.lock().expect("metrics lock poisoned").clone()
}

/// Return metrics for all handlers that match the given event name.
pub fn get_hook_metrics_for_event(event_name: &str) -> Vec<HookMetrics> {
    HOOK_METRICS
        .lock()
        .expect("metrics lock poisoned")
        .values()
        .filter(|m| m.event_name == event_name)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommandHandlerConfig, HttpHandlerConfig};
    use crate::types::HookOutput;

    // -- DispatchConfig -------------------------------------------------------

    #[test]
    fn dispatch_config_defaults() {
        let cfg = DispatchConfig::default();
        assert_eq!(cfg.max_concurrency, 10);
        assert_eq!(cfg.timeout_secs, 30);
        assert!(!cfg.fail_closed);
        assert!(!cfg.dry_run);
    }

    #[test]
    fn dispatch_config_from_settings() {
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

    // -- classify_decision ----------------------------------------------------

    #[test]
    fn classify_continue_allow_explicit() {
        let result = HookResult::Continue(HookOutput::allow());
        let classified = classify_decision(&result);
        assert!(matches!(classified, ClassifiedOutcome::Allow));
    }

    #[test]
    fn classify_continue_allow_default() {
        // No decision field set -- should classify as Allow.
        let result = HookResult::Continue(HookOutput::continue_());
        let classified = classify_decision(&result);
        assert!(matches!(classified, ClassifiedOutcome::Allow));
    }

    #[test]
    fn classify_continue_ask() {
        let result = HookResult::Continue(HookOutput::ask("need approval"));
        let classified = classify_decision(&result);
        if let ClassifiedOutcome::Ask { reason } = classified {
            assert_eq!(reason, "need approval");
        } else {
            panic!("expected Ask");
        }
    }

    #[test]
    fn classify_continue_deny_via_decision_field() {
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
        let classified = classify_decision(&result);
        if let ClassifiedOutcome::Deny { reason } = classified {
            assert_eq!(reason, "blocked");
        } else {
            panic!("expected Deny");
        }
    }

    #[test]
    fn classify_blocked() {
        let result = HookResult::Blocked {
            reason: "not allowed".to_string(),
            output: HookOutput::block("not allowed"),
        };
        let classified = classify_decision(&result);
        if let ClassifiedOutcome::Deny { reason } = classified {
            assert_eq!(reason, "not allowed");
        } else {
            panic!("expected Deny");
        }
    }

    #[test]
    fn classify_failed() {
        let result = HookResult::Failed {
            error: "timeout".to_string(),
        };
        let classified = classify_decision(&result);
        if let ClassifiedOutcome::Failed { error } = classified {
            assert_eq!(error, "timeout");
        } else {
            panic!("expected Failed");
        }
    }

    // -- aggregate_decision ---------------------------------------------------

    #[test]
    fn aggregate_empty_is_allow() {
        let decision = aggregate_decision(&[], false);
        assert!(matches!(decision, AggregatedDecision::Allow));
    }

    #[test]
    fn aggregate_all_allow() {
        let outcomes = vec![ClassifiedOutcome::Allow, ClassifiedOutcome::Allow];
        let decision = aggregate_decision(&outcomes, false);
        assert!(matches!(decision, AggregatedDecision::Allow));
    }

    #[test]
    fn aggregate_single_ask() {
        let outcomes = vec![ClassifiedOutcome::Ask {
            reason: "review needed".to_string(),
        }];
        let decision = aggregate_decision(&outcomes, false);
        if let AggregatedDecision::Ask { reasons } = decision {
            assert_eq!(reasons, vec!["review needed"]);
        } else {
            panic!("expected Ask");
        }
    }

    #[test]
    fn aggregate_multiple_asks() {
        let outcomes = vec![
            ClassifiedOutcome::Allow,
            ClassifiedOutcome::Ask {
                reason: "first".to_string(),
            },
            ClassifiedOutcome::Ask {
                reason: "second".to_string(),
            },
        ];
        let decision = aggregate_decision(&outcomes, false);
        if let AggregatedDecision::Ask { reasons } = decision {
            assert_eq!(reasons.len(), 2);
        } else {
            panic!("expected Ask");
        }
    }

    #[test]
    fn aggregate_deny_takes_precedence_over_ask() {
        let outcomes = vec![
            ClassifiedOutcome::Ask {
                reason: "want to ask".to_string(),
            },
            ClassifiedOutcome::Deny {
                reason: "blocked".to_string(),
            },
        ];
        let decision = aggregate_decision(&outcomes, false);
        if let AggregatedDecision::Deny { reason, .. } = decision {
            assert_eq!(reason, "blocked");
        } else {
            panic!("expected Deny");
        }
    }

    #[test]
    fn aggregate_fail_open_ignores_failures() {
        let outcomes = vec![
            ClassifiedOutcome::Allow,
            ClassifiedOutcome::Failed {
                error: "crash".to_string(),
            },
        ];
        let decision = aggregate_decision(&outcomes, false);
        assert!(matches!(decision, AggregatedDecision::Allow));
    }

    #[test]
    fn aggregate_fail_closed_treats_failure_as_deny() {
        let outcomes = vec![
            ClassifiedOutcome::Allow,
            ClassifiedOutcome::Failed {
                error: "crash".to_string(),
            },
        ];
        let decision = aggregate_decision(&outcomes, true);
        if let AggregatedDecision::Deny { reason, .. } = decision {
            assert!(reason.contains("fail-closed"));
            assert!(reason.contains("crash"));
        } else {
            panic!("expected Deny");
        }
    }

    #[test]
    fn aggregate_first_deny_wins() {
        let outcomes = vec![
            ClassifiedOutcome::Deny {
                reason: "first".to_string(),
            },
            ClassifiedOutcome::Deny {
                reason: "second".to_string(),
            },
        ];
        let decision = aggregate_decision(&outcomes, false);
        if let AggregatedDecision::Deny { reason, .. } = decision {
            assert_eq!(reason, "first");
        } else {
            panic!("expected Deny");
        }
    }

    // -- DispatchStats --------------------------------------------------------

    #[test]
    fn stats_defaults() {
        let stats = DispatchStats::default();
        assert_eq!(stats.total_dispatched, 0);
        assert!(stats.all_succeeded());
        assert!(!stats.any_denied());
        assert!(!stats.any_asked());
    }

    // -- handler_label --------------------------------------------------------

    #[test]
    fn label_command_short() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: "check.sh".to_string(),
            ..Default::default()
        });
        assert_eq!(handler_label(&handler), "cmd:check.sh");
    }

    #[test]
    fn label_command_long_truncated() {
        let long_cmd = "a".repeat(100);
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: long_cmd,
            ..Default::default()
        });
        let label = handler_label(&handler);
        assert!(label.starts_with("cmd:"));
        assert!(label.ends_with("..."));
        assert!(label.len() < 70);
    }

    #[test]
    fn label_http() {
        let handler = HookHandlerConfig::Http(HttpHandlerConfig {
            url: "http://localhost:9090/hook".to_string(),
            ..Default::default()
        });
        assert_eq!(handler_label(&handler), "http:http://localhost:9090/hook");
    }

    // -- effective_timeout ----------------------------------------------------

    #[test]
    fn timeout_override_wins() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            timeout_secs: Some(99),
            ..Default::default()
        });
        assert_eq!(effective_timeout(&handler, 30), 99);
    }

    #[test]
    fn timeout_falls_back_to_default() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            timeout_secs: None,
            ..Default::default()
        });
        assert_eq!(effective_timeout(&handler, 30), 30);
    }

    // -- dispatch_hooks (dry-run integration) ---------------------------------

    #[tokio::test]
    async fn dispatch_dry_run_reports_allow_for_all() {
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
        ];
        let refs: Vec<&HookHandlerConfig> = handlers.iter().collect();

        let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &refs, &config).await;

        assert_eq!(stats.total_dispatched, 2);
        assert_eq!(stats.allowed, 2);
        assert_eq!(stats.failed, 0);
        assert!(stats.all_succeeded());
    }

    #[tokio::test]
    async fn dispatch_empty_handlers() {
        let config = DispatchConfig::default();
        let input = HookInput::default();
        let handlers: Vec<&HookHandlerConfig> = vec![];

        let stats = dispatch_hooks(&HookEvent::PreToolUse, &input, &handlers, &config).await;

        assert_eq!(stats.total_dispatched, 0);
        assert!(stats.all_succeeded());
        assert!(stats.results.is_empty());
    }
}
