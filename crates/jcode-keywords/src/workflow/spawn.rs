//! Sub-agent spawning utility for workflow execution.
//!
//! Provides helpers to spawn child agents using the same pattern as `SubagentTool`
//! in `jcode-app-core/src/tool/task.rs`.

use super::{SpawnResult, SpawnSpec};

/// Spawn a single sub-agent synchronously and return its output.
///
/// This is a placeholder that will be wired to the actual Agent spawning
/// mechanism via the `WorkflowExecutor` in `jcode-app-core`.
///
/// The actual implementation needs:
/// - `provider.fork()` to create an isolated provider
/// - `Session::create()` for a new session
/// - `Agent::new_with_session()` to build the agent
/// - `agent.run_once_capture(&prompt)` to execute
pub async fn spawn_agent(spec: &SpawnSpec) -> SpawnResult {
    // This is a stub. The real implementation is in executor.rs
    // which has access to Provider, Registry, and Session.
    SpawnResult {
        description: spec.description.clone(),
        output: format!(
            "[Workflow sub-agent '{}']: {}",
            spec.description, spec.prompt
        ),
        success: true,
    }
}

/// Spawn multiple sub-agents in parallel and collect results.
pub async fn spawn_parallel(specs: &[SpawnSpec]) -> Vec<SpawnResult> {
    let mut handles = Vec::new();
    for spec in specs {
        let spec = spec.clone();
        handles.push(tokio::spawn(async move { spawn_agent(&spec).await }));
    }
    let mut results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            results.push(result);
        }
    }
    results
}

/// Aggregate results from parallel sub-agents into a single summary.
pub fn aggregate_results(results: &[SpawnResult]) -> String {
    if results.is_empty() {
        return "No results from sub-agents.".to_string();
    }

    let mut output = String::new();
    output.push_str("# Parallel Execution Results\n\n");

    for (i, result) in results.iter().enumerate() {
        let status = if result.success { "✅" } else { "❌" };
        output.push_str(&format!(
            "## {} Task {}: {}\n\n{}\n\n",
            status, i, result.description, result.output
        ));
    }

    let success_count = results.iter().filter(|r| r.success).count();
    output.push_str(&format!(
        "---\n**Summary**: {}/{} tasks completed successfully.",
        success_count,
        results.len()
    ));

    output
}

/// Retry a failed sub-agent spawn up to max_retries times.
pub async fn spawn_with_retry(spec: &SpawnSpec, max_retries: u32) -> SpawnResult {
    for attempt in 0..=max_retries {
        let result = spawn_agent(spec).await;
        if result.success || attempt == max_retries {
            return result;
        }
        // Brief delay before retry
        tokio::time::sleep(std::time::Duration::from_millis(100 * (attempt as u64 + 1))).await;
    }
    SpawnResult {
        description: spec.description.clone(),
        output: "Max retries exceeded".to_string(),
        success: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_empty_results() {
        assert!(aggregate_results(&[]).contains("No results"));
    }

    #[test]
    fn aggregate_single_result() {
        let results = vec![SpawnResult {
            description: "test task".to_string(),
            output: "done".to_string(),
            success: true,
        }];
        let summary = aggregate_results(&results);
        assert!(summary.contains("1/1"));
        assert!(summary.contains("test task"));
    }

    #[test]
    fn aggregate_mixed_results() {
        let results = vec![
            SpawnResult {
                description: "task 1".to_string(),
                output: "ok".to_string(),
                success: true,
            },
            SpawnResult {
                description: "task 2".to_string(),
                output: "failed".to_string(),
                success: false,
            },
        ];
        let summary = aggregate_results(&results);
        assert!(summary.contains("1/2"));
    }
}
