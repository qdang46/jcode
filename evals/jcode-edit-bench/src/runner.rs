//! Agent runner for the edit benchmark.
//!
//! Orchestrates benchmark runs by launching `jcode agent run` subprocesses
//! and verifying results. Supports parallel runs for reliability measurement.
//!
//! Architecture follows oh-my-pi's `runner.ts` but simplified for Rust:
//! - Subprocess-based agent execution (jcode agent run)
//! - Task-level parallelism with semaphore
//! - Timeout with retry logic
//! - Best-of-N result selection

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

use crate::fixtures::{list_files, load_tasks_from_dir};
use crate::types::{BenchmarkConfig, EditTask, EditFailure, TaskResult, TaskRunResult, TokenStats, ToolCallStats};
use crate::report::{build_benchmark_result, pick_best_run_index, summarize_task};

/// Maximum concurrent agent subprocesses.
const MAX_CONCURRENCY: usize = 8;

/// Run the full benchmark.
pub async fn run_benchmark(
    fixtures_dir: &Path,
    config: &BenchmarkConfig,
) -> anyhow::Result<crate::types::BenchmarkResult> {
    let tasks = load_tasks_from_dir(fixtures_dir)?;
    if tasks.is_empty() {
        anyhow::bail!("No tasks found in {fixtures_dir:?}");
    }

    eprintln!(
        "Edit Benchmark\n==============\nModel: {}\nRuns per task: {}\nTasks: {}\n",
        config.model, config.runs_per_task, tasks.len()
    );

    let start_time = chrono_now();
    let semaphore = Arc::new(Semaphore::new(config.task_concurrency.min(MAX_CONCURRENCY)));
    let mut handles = Vec::new();

    for task in tasks {
        let s = semaphore.clone();
        let cfg = config.clone();

        handles.push(tokio::spawn(async move {
            let _permit = s.acquire().await.unwrap();
            let result = run_task_with_retries(task, &cfg).await;
            drop(_permit);
            result
        }));
    }

    let mut task_results: Vec<TaskResult> = Vec::new();
    for handle in handles {
        if let Ok(Some(tr)) = handle.await {
            task_results.push(tr);
        }
    }

    let end_time = chrono_now();
    let config_json = serde_json::json!({
        "model": config.model,
        "runs_per_task": config.runs_per_task,
        "timeout_ms": config.timeout_ms,
        "task_concurrency": config.task_concurrency,
        "auto_format": config.auto_format,
        "max_attempts": config.max_attempts,
    });

    let result = build_benchmark_result(&task_results, &config_json, &start_time, &end_time);

    Ok(result)
}

/// Run all N runs for a single task and return the summarized result.
async fn run_task_with_retries(
    task: EditTask,
    config: &BenchmarkConfig,
) -> Option<TaskResult> {
    let mut all_runs: Vec<TaskRunResult> = Vec::new();

    for run_idx in 0..config.runs_per_task {
        eprintln!("  [{}/{}] {}...", run_idx + 1, config.runs_per_task, task.id);

        // Create temp working directory
        let work_dir = tempfile::tempdir().ok()?;

        // Copy fixture files to working dir
        if let Err(e) = copy_fixtures(&task.input_dir, work_dir.path()).await {
            eprintln!("    Failed to copy fixtures: {e}");
            all_runs.push(build_failure_result(run_idx, &format!("Fixture copy error: {e}")));
            continue;
        }

        let result = run_single_attempt(&task, work_dir.path(), config, run_idx).await;

        all_runs.push(result);
    }

    Some(summarize_task(&task.id, &task.name, all_runs))
}

/// Run one attempt of the agent against a task.
async fn run_single_attempt(
    task: &EditTask,
    work_dir: &Path,
    config: &BenchmarkConfig,
    run_index: usize,
) -> TaskRunResult {
    let start_time = Instant::now();
    let mut tokens = TokenStats::default();
    let mut tool_calls = ToolCallStats::default();
    let mut edit_failures: Vec<EditFailure> = Vec::new();
    let mut error: Option<String> = None;
    let mut verification_passed = false;
    let mut indent_score: Option<f64> = None;
    let mut formatted_equivalent: Option<bool> = None;
    let mut diff: Option<String> = None;
    let mut early_stopped = false;

    for attempt in 0..config.max_attempts {
        // Build the prompt from task metadata
        let prompt = task.prompt.clone();

        // Run jcode agent
        let result = run_jcode_agent(work_dir, &prompt, config.timeout_ms).await;

        match result {
            Ok((events, agent_error)) => {
                // Parse events for tool calls and tokens
                for event in &events {
                    if event.starts_with("read:") { tool_calls.read += 1; }
                    if event.starts_with("edit:") { tool_calls.edit += 1; }
                    if event.starts_with("write:") { tool_calls.write += 1; }
                    if event.starts_with("edit_success:") { tool_calls.edit_successes += 1; }
                    if event.starts_with("edit_failure:") {
                        tool_calls.edit_failures += 1;
                        let parts: Vec<&str> = event.splitn(3, ':').collect();
                        if parts.len() >= 2 {
                            edit_failures.push(EditFailure {
                                tool_call_id: format!("attempt-{attempt}"),
                                category: categorize_edit_failure(event),
                                error: event.to_string(),
                            });
                        }
                    }
                }

                if let Some(e) = agent_error {
                    error = Some(e);
                }

                // Run verification
                let verify_result = crate::verify::verify_files(
                    &task.expected_dir,
                    work_dir,
                    Some(&task.files),
                ).await;

                match verify_result {
                    Ok(vr) => {
                        if vr.success {
                            verification_passed = true;
                            indent_score = vr.indent_score;
                            formatted_equivalent = vr.formatted_equivalent;
                            diff = vr.diff;
                            // If verification succeeded, no more attempts needed
                            break;
                        } else {
                            indent_score = vr.indent_score;
                            formatted_equivalent = vr.formatted_equivalent;
                            diff = vr.diff;
                            error = vr.error;
                            // If we have more attempts, retry with diff context
                        }
                    }
                    Err(e) => {
                        error = Some(format!("Verification error: {e}"));
                        break;
                    }
                }
            }
            Err(e) => {
                error = Some(format!("Agent error: {e}"));
                break;
            }
        }
    }

    let duration_ms = start_time.elapsed().as_millis() as u64;

    let success = verification_passed && tool_calls.edit > 0;

    TaskRunResult {
        run_index,
        success,
        verification_passed,
        mutation_type: task.metadata.as_ref().map(|m| m.mutation_type.clone()),
        mutation_category: task.metadata.as_ref().map(|m| m.mutation_category.clone()),
        difficulty_score: task.metadata.as_ref().map(|m| m.difficulty_score),
        error,
        tokens,
        duration_ms,
        indent_score,
        formatted_equivalent,
        diff,
        tool_calls,
        edit_failures,
        edit_autocorrect_count: 0,
        early_stopped,
    }
}

/// Run `jcode agent run` as a subprocess with the given prompt.
async fn run_jcode_agent(
    cwd: &Path,
    prompt: &str,
    timeout_ms: u64,
) -> Result<(Vec<String>, Option<String>), String> {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut child = Command::new("jcode")
        .args(["agent", "run"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn jcode: {e}"))?;

    // Write prompt to stdin
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(prompt.as_bytes()).await.map_err(|e| format!("stdin write error: {e}"))?;
        stdin.flush().await.map_err(|e| format!("stdin flush error: {e}"))?;
    }
    // Drop stdin to signal EOF
    drop(child.stdin.take());

    // Wait with timeout
    let timed_result = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        child.wait_with_output(),
    )
    .await;

    match timed_result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            let mut events = Vec::new();
            for line in stdout.lines() {
                if line.starts_with("read:")
                    || line.starts_with("edit:")
                    || line.starts_with("write:")
                    || line.starts_with("edit_success:")
                    || line.starts_with("edit_failure:")
                {
                    events.push(line.to_string());
                }
            }

            let error = if !output.status.success() {
                Some(if stderr.is_empty() {
                    format!("Exit code: {}", output.status.code().unwrap_or(-1))
                } else {
                    stderr.trim().to_string()
                })
            } else {
                None
            };

            Ok((events, error))
        }
        Ok(Err(e)) => Err(format!("jcode process error: {e}")),
        Err(_elapsed) => Err("Timeout waiting for jcode agent".to_string()),
    }
}

/// Copy fixture files from input_dir to work_dir.
async fn copy_fixtures(input_dir: &Path, work_dir: &Path) -> anyhow::Result<()> {
    let files = list_files(input_dir)?;
    for file in &files {
        let src = input_dir.join(file);
        let dst = work_dir.join(file);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dst)?;
    }
    Ok(())
}

/// Build a failure result for a run that couldn't start.
fn build_failure_result(run_index: usize, error: &str) -> TaskRunResult {
    TaskRunResult {
        run_index,
        success: false,
        verification_passed: false,
        mutation_type: None,
        mutation_category: None,
        difficulty_score: None,
        error: Some(error.to_string()),
        tokens: TokenStats::default(),
        duration_ms: 0,
        indent_score: None,
        formatted_equivalent: None,
        diff: None,
        tool_calls: ToolCallStats::default(),
        edit_failures: vec![],
        edit_autocorrect_count: 0,
        early_stopped: false,
    }
}

/// Categorize an edit failure from the event string.
fn categorize_edit_failure(event: &str) -> String {
    if event.contains("continuation") || event.contains("LidA") {
        "range-continuation".to_string()
    } else if event.contains("unified-diff") || event.contains("+Lid") {
        "unified-diff".to_string()
    } else if event.contains("No changes") || event.contains("no change") {
        "no-change".to_string()
    } else if event.contains("hash mismatch") || event.contains("stale") {
        "hash-mismatch".to_string()
    } else {
        "other".to_string()
    }
}

/// ISO-8601 timestamp without pulling in full chrono formatting.
fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
