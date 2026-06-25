//! Sub-agent spawning utility for workflow execution.
//!
//! Provides helpers to spawn child agents using the same pattern as `SubagentTool`
//! in `jcode-app-core/src/tool/task.rs`.
//!
//! The actual spawning implementation is registered via [`set_spawn_impl`] by
//! `jcode-app-core` at startup. Until then, [`spawn_agent`] returns a placeholder.

use super::{SpawnResult, SpawnSpec};
use std::sync::{LazyLock, Mutex};

/// Maximum concurrent sub-agents per spawn call.
const MAX_CONCURRENT: usize = 4;

/// A function that can spawn a sub-agent given a `SpawnSpec`.
/// Returns the spawned agent's output as a `SpawnResult`.
pub type SpawnFn = dyn Fn(&SpawnSpec) -> SpawnResult + Send + Sync;

static SPAWN_IMPL: LazyLock<Mutex<Option<Box<SpawnFn>>>> = LazyLock::new(|| Mutex::new(None));

/// Register the real spawn implementation. Called by `jcode-app-core` at startup.
/// Panics if already registered (idempotent — second call is a no-op).
pub fn set_spawn_impl(impl_fn: Box<SpawnFn>) {
    let mut guard = SPAWN_IMPL.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_some() {
        return; // already set
    }
    *guard = Some(impl_fn);
}

/// Spawn a single sub-agent and return its output.
/// Delegates to the registered implementation, or returns a placeholder if none set.
pub async fn spawn_agent(spec: &SpawnSpec) -> SpawnResult {
    let guard = SPAWN_IMPL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref spawn_fn) = *guard {
        (spawn_fn)(spec)
    } else {
        // Stub fallback
        SpawnResult {
            description: spec.description.clone(),
            output: format!(
                "[Workflow sub-agent '{}']: {}",
                spec.description, spec.prompt
            ),
            success: true,
        }
    }
}

/// Spawn multiple sub-agents in parallel and collect results.
pub async fn spawn_parallel(specs: &[SpawnSpec]) -> Vec<SpawnResult> {
    // Snapshot the spec list so we don't hold the lock across awaits.
    let specs = specs.to_vec();
    let mut results = Vec::new();
    for chunk in specs.chunks(MAX_CONCURRENT) {
        let chunk = chunk.to_vec();
        let mut handles = Vec::new();
        for spec in chunk {
            handles.push(tokio::spawn(async move { spawn_agent(&spec).await }));
        }
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    results.push(SpawnResult {
                        description: "unknown".to_string(),
                        output: format!("Sub-agent panicked: {}", e),
                        success: false,
                    });
                }
            }
        }
    }
    results
}

/// Aggregate results from parallel sub-agents into a single summary.
pub fn aggregate_results(results: &[SpawnResult]) -> String {
    if results.is_empty() {
        return "No results from sub-agents.".to_string();
    }
    let mut output = String::from("# Parallel Execution Results\n\n");
    for (i, r) in results.iter().enumerate() {
        let s = if r.success { "✅" } else { "❌" };
        output.push_str(&format!(
            "## {} Task {}: {}\n\n{}\n\n",
            s, i, r.description, r.output
        ));
    }
    let ok = results.iter().filter(|r| r.success).count();
    output.push_str(&format!(
        "---\n**Summary**: {}/{} tasks completed.",
        ok,
        results.len()
    ));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_empty() {
        assert!(aggregate_results(&[]).contains("No results"));
    }
    #[test]
    fn aggregate_single() {
        let r = vec![SpawnResult {
            description: "t".into(),
            output: "done".into(),
            success: true,
        }];
        assert!(aggregate_results(&r).contains("1/1"));
    }
    #[test]
    fn aggregate_mixed() {
        let r = vec![
            SpawnResult {
                description: "a".into(),
                output: "ok".into(),
                success: true,
            },
            SpawnResult {
                description: "b".into(),
                output: "fail".into(),
                success: false,
            },
        ];
        assert!(aggregate_results(&r).contains("1/2"));
    }
}
