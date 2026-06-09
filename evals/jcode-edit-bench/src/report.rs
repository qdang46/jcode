//! Report generators for edit benchmark results.
//!
//! Produces both Markdown and JSON reports. Adapted from oh-my-pi's
//! `report.ts`.

use std::collections::HashMap;

use crate::types::{
    BenchmarkResult, BenchmarkSummary, CategorySummary, DifficultySummary, TaskResult,
    TaskRunResult, TokenStats, ToolCallStats,
};

// ── Build summary from raw results ──────────────────────────────────

/// Build a `BenchmarkResult` from tasks and per-task runs.
pub fn build_benchmark_result(
    tasks: &[TaskResult],
    config: &serde_json::Value,
    start_time: &str,
    end_time: &str,
) -> BenchmarkResult {
    let summary = compute_summary(tasks);
    BenchmarkResult {
        config: config.clone(),
        tasks: tasks.to_vec(),
        summary,
        start_time: start_time.to_string(),
        end_time: end_time.to_string(),
    }
}

fn compute_summary(tasks: &[TaskResult]) -> BenchmarkSummary {
    let total_tasks = tasks.len();
    let successful_tasks = tasks.iter().filter(|t| t.success).count();

    let all_runs: Vec<&TaskRunResult> = tasks.iter().flat_map(|t| t.runs.iter()).collect();
    let non_ghost: Vec<&&TaskRunResult> = all_runs.iter().filter(|r| !is_ghost(r)).collect();
    let timeout_runs = non_ghost
        .iter()
        .filter(|r| {
            r.error
                .as_deref()
                .map(|e| e.contains("Timeout"))
                .unwrap_or(false)
        })
        .count();
    let ghost_runs = all_runs.iter().filter(|r| is_ghost(r)).count();

    // Best run aggregates
    let best_runs: Vec<&TaskRunResult> = tasks
        .iter()
        .filter_map(|t| t.runs.iter().find(|r| r.run_index == t.best_run_index))
        .collect();

    let total_tokens = TokenStats {
        input: best_runs.iter().map(|r| r.tokens.input).sum(),
        output: best_runs.iter().map(|r| r.tokens.output).sum(),
        total: best_runs.iter().map(|r| r.tokens.total).sum(),
    };

    let denom = total_tasks.max(1);
    let best_denom = best_runs.len().max(1);

    let avg_tokens_per_task = TokenStats {
        input: total_tokens.input / best_denom as u64,
        output: total_tokens.output / best_denom as u64,
        total: total_tokens.total / best_denom as u64,
    };

    let total_duration_ms: u64 = best_runs.iter().map(|r| r.duration_ms).sum();
    let avg_duration_per_task_ms = total_duration_ms / best_denom as u64;

    let indent_scores: Vec<f64> = best_runs.iter().filter_map(|r| r.indent_score).collect();
    let avg_indent_score = if indent_scores.is_empty() {
        0.0
    } else {
        indent_scores.iter().sum::<f64>() / indent_scores.len() as f64
    };

    let total_tool_calls = ToolCallStats {
        read: best_runs.iter().map(|r| r.tool_calls.read).sum(),
        edit: best_runs.iter().map(|r| r.tool_calls.edit).sum(),
        write: best_runs.iter().map(|r| r.tool_calls.write).sum(),
        edit_successes: best_runs.iter().map(|r| r.tool_calls.edit_successes).sum(),
        edit_failures: best_runs.iter().map(|r| r.tool_calls.edit_failures).sum(),
        edit_warnings: best_runs.iter().map(|r| r.tool_calls.edit_warnings).sum(),
        edit_autocorrects: best_runs
            .iter()
            .map(|r| r.tool_calls.edit_autocorrects)
            .sum(),
    };

    let total_edits = total_tool_calls.edit as f64;
    let total_successes = total_tool_calls.edit_successes as f64;
    let edit_success_rate = if total_edits > 0.0 {
        total_successes / total_edits
    } else {
        1.0
    };

    // By category
    let mut by_category: HashMap<String, Vec<&TaskResult>> = HashMap::new();
    for task in tasks {
        let cat = task.id.split('-').next().unwrap_or("unknown").to_string();
        by_category.entry(cat).or_default().push(task);
    }
    let by_category_summary: HashMap<String, CategorySummary> = by_category
        .into_iter()
        .map(|(cat, tsks)| {
            let total = tsks.len();
            let passed = tsks.iter().filter(|t| t.success).count();
            let avg_difficulty_score = tsks
                .iter()
                .filter_map(|t| t.runs.first().and_then(|r| r.difficulty_score))
                .sum::<u32>() as f64
                / total.max(1) as f64;
            (
                cat,
                CategorySummary {
                    total,
                    passed,
                    rate: passed as f64 / total.max(1) as f64,
                    avg_difficulty_score,
                },
            )
        })
        .collect();

    BenchmarkSummary {
        total_tasks,
        successful_tasks,
        task_success_rate: successful_tasks as f64 / denom as f64,
        total_tokens,
        avg_tokens_per_task,
        total_duration_ms,
        avg_duration_per_task_ms,
        avg_indent_score,
        total_tool_calls,
        edit_success_rate,
        timeout_runs,
        ghost_runs,
        by_category: by_category_summary,
        by_difficulty: HashMap::new(),
    }
}

fn is_ghost(run: &TaskRunResult) -> bool {
    !run.success
        && run.tokens.total == 0
        && run.tool_calls.read == 0
        && run.tool_calls.edit == 0
        && run.tool_calls.write == 0
}

// ── Best-run selection ─────────────────────────────────────────────

/// Select the best run from a list of runs.
///
/// Priority: 1) success, 2) non-ghost, 3) fewer tokens, 4) earlier index
pub fn pick_best_run_index(runs: &[TaskRunResult]) -> usize {
    if runs.is_empty() {
        return 0;
    }
    let mut best = 0;
    for i in 1..runs.len() {
        if is_better_run(&runs[i], &runs[best]) {
            best = i;
        }
    }
    best
}

fn is_better_run(a: &TaskRunResult, b: &TaskRunResult) -> bool {
    if a.success != b.success {
        return a.success;
    }
    let a_ghost = is_ghost(a);
    let b_ghost = is_ghost(b);
    if a_ghost != b_ghost {
        return !a_ghost;
    }
    if a.tokens.total != b.tokens.total {
        return a.tokens.total < b.tokens.total;
    }
    a.run_index < b.run_index
}

/// Summarize a task's runs into a `TaskResult`.
pub fn summarize_task(id: &str, name: &str, runs: Vec<TaskRunResult>) -> TaskResult {
    let ordered: Vec<&TaskRunResult> = {
        let mut v: Vec<&TaskRunResult> = runs.iter().collect();
        v.sort_by(|a, b| a.run_index.cmp(&b.run_index));
        v
    };

    let best_idx = pick_best_run_index(&runs);
    let best = &runs[best_idx];

    let edit_success_rate = if best.tool_calls.edit > 0 {
        best.tool_calls.edit_successes as f64 / best.tool_calls.edit as f64
    } else {
        1.0
    };

    let best_run_index = best.run_index;
    let best_success = best.success;
    let best_tokens = TokenStats {
        input: best.tokens.input,
        output: best.tokens.output,
        total: best.tokens.total,
    };
    let best_duration = best.duration_ms;
    let best_tool_calls = ToolCallStats {
        read: best.tool_calls.read,
        edit: best.tool_calls.edit,
        write: best.tool_calls.write,
        edit_successes: best.tool_calls.edit_successes,
        edit_failures: best.tool_calls.edit_failures,
        edit_warnings: best.tool_calls.edit_warnings,
        edit_autocorrects: best.tool_calls.edit_autocorrects,
    };

    TaskResult {
        id: id.to_string(),
        name: name.to_string(),
        runs,
        best_run_index,
        success: best_success,
        tokens: best_tokens,
        duration_ms: best_duration,
        tool_calls: best_tool_calls,
        edit_success_rate,
    }
}

// ── Markdown report ────────────────────────────────────────────────

/// Generate a Markdown report from benchmark results.
pub fn generate_markdown_report(result: &BenchmarkResult) -> String {
    let mut md = String::new();

    // Header
    md.push_str("# Edit Benchmark Report\n\n");
    md.push_str(&format!("**Date:** {}\n\n", result.end_time));
    md.push_str(&format!(
        "**Config:** model=`{}`, runs_per_task=`, total_tasks=`{}`\n\n",
        result
            .config
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("?"),
        result.summary.total_tasks,
    ));

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!("| Metric | Value |\n|--------|-------|\n"));
    md.push_str(&format!(
        "| Task Success Rate | {:.1}% ({} / {}) |\n",
        result.summary.task_success_rate * 100.0,
        result.summary.successful_tasks,
        result.summary.total_tasks,
    ));
    md.push_str(&format!(
        "| Edit Success Rate | {:.1}% |\n",
        result.summary.edit_success_rate * 100.0,
    ));
    md.push_str(&format!(
        "| Avg Indent Score | {:.2} |\n",
        result.summary.avg_indent_score,
    ));
    md.push_str(&format!(
        "| Total Tokens | {} in / {} out |\n",
        result.summary.total_tokens.input, result.summary.total_tokens.output,
    ));
    md.push_str(&format!(
        "| Avg Duration/Task | {}ms |\n",
        result.summary.avg_duration_per_task_ms,
    ));
    md.push_str(&format!("| Ghost Runs | {} |\n", result.summary.ghost_runs,));
    md.push_str(&format!(
        "| Timeout Runs | {} |\n\n",
        result.summary.timeout_runs,
    ));

    // Token stats
    md.push_str("### Token Statistics\n\n");
    md.push_str("| Metric | Input | Output | Total |\n|--------|-------|--------|-------|\n");
    md.push_str(&format!(
        "| Total | {} | {} | {} |\n",
        result.summary.total_tokens.input,
        result.summary.total_tokens.output,
        result.summary.total_tokens.total,
    ));
    md.push_str(&format!(
        "| Average/Task | {} | {} | {} |\n\n",
        result.summary.avg_tokens_per_task.input,
        result.summary.avg_tokens_per_task.output,
        result.summary.avg_tokens_per_task.total,
    ));

    // Tool calls
    md.push_str("### Tool Calls\n\n");
    md.push_str(
        "| Tool | Calls | Successes | Failures |\n|------|-------|-----------|----------|\n",
    );
    md.push_str(&format!(
        "| Read | {} | — | — |\n",
        result.summary.total_tool_calls.read,
    ));
    md.push_str(&format!(
        "| Edit | {} | {} | {} |\n",
        result.summary.total_tool_calls.edit,
        result.summary.total_tool_calls.edit_successes,
        result.summary.total_tool_calls.edit_failures,
    ));
    md.push_str(&format!(
        "| Write | {} | — | — |\n\n",
        result.summary.total_tool_calls.write,
    ));

    // Category breakdown
    md.push_str("### By Category\n\n");
    md.push_str("| Category | Tasks | Passed | Rate | Avg Difficulty |\n|----------|-------|--------|------|----------------|\n");
    let mut cats: Vec<_> = result.summary.by_category.iter().collect();
    cats.sort_by(|a, b| a.0.cmp(b.0));
    for (cat, summary) in &cats {
        md.push_str(&format!(
            "| {} | {} | {} | {:.1}% | {:.1} |\n",
            cat,
            summary.total,
            summary.passed,
            summary.rate * 100.0,
            summary.avg_difficulty_score,
        ));
    }
    md.push('\n');

    // Per-task breakdown
    md.push_str("### Per-Task Results\n\n");
    md.push_str("| Task | Success | Duration | Tokens | Edits |\n|------|---------|----------|--------|-------|\n");
    for task in &result.tasks {
        let status = if task.success { "✅ PASS" } else { "❌ FAIL" };
        md.push_str(&format!(
            "| {} | {} | {}ms | {} | {:.0} |\n",
            task.id, status, task.duration_ms, task.tokens.total, task.tool_calls.edit,
        ));
    }

    md
}

/// Generate a JSON report string.
pub fn generate_json_report(result: &BenchmarkResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_runs() -> Vec<TaskRunResult> {
        vec![
            TaskRunResult {
                run_index: 0,
                success: false,
                verification_passed: false,
                mutation_type: Some("swap-comparison".into()),
                mutation_category: Some("operator".into()),
                difficulty_score: Some(3),
                error: Some("Timeout".into()),
                tokens: TokenStats {
                    input: 0,
                    output: 0,
                    total: 0,
                },
                duration_ms: 1000,
                indent_score: None,
                formatted_equivalent: None,
                diff: None,
                tool_calls: ToolCallStats::default(),
                edit_failures: vec![],
                edit_autocorrect_count: 0,
                early_stopped: false,
            },
            TaskRunResult {
                run_index: 1,
                success: true,
                verification_passed: true,
                mutation_type: Some("swap-comparison".into()),
                mutation_category: Some("operator".into()),
                difficulty_score: Some(3),
                error: None,
                tokens: TokenStats {
                    input: 100,
                    output: 50,
                    total: 150,
                },
                duration_ms: 5000,
                indent_score: Some(0.5),
                formatted_equivalent: Some(true),
                diff: None,
                tool_calls: ToolCallStats {
                    read: 2,
                    edit: 1,
                    write: 0,
                    edit_successes: 1,
                    edit_failures: 0,
                    edit_warnings: 0,
                    edit_autocorrects: 0,
                },
                edit_failures: vec![],
                edit_autocorrect_count: 0,
                early_stopped: false,
            },
        ]
    }

    #[test]
    fn test_pick_best_run_selects_successful() {
        let runs = sample_runs();
        let idx = pick_best_run_index(&runs);
        assert_eq!(idx, 1); // Run 1 is successful
    }

    #[test]
    fn test_summarize_task() {
        let runs = sample_runs();
        let result = summarize_task("test-task", "Test Task", runs);
        assert!(result.success);
        assert_eq!(result.tokens.total, 150);
        assert_eq!(result.duration_ms, 5000);
    }

    #[test]
    fn test_is_ghost_detection() {
        let ghost = TaskRunResult {
            success: false,
            tokens: TokenStats {
                input: 0,
                output: 0,
                total: 0,
            },
            tool_calls: ToolCallStats::default(),
            run_index: 0,
            verification_passed: false,
            mutation_type: None,
            mutation_category: None,
            difficulty_score: None,
            error: Some("Timeout".into()),
            duration_ms: 0,
            indent_score: None,
            formatted_equivalent: None,
            diff: None,
            edit_failures: vec![],
            edit_autocorrect_count: 0,
            early_stopped: false,
        };
        assert!(is_ghost(&ghost));
    }
}
