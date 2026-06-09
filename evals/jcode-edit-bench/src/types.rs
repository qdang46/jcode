//! Shared data types for the edit benchmark harness.
//!
//! All types are serializable for JSON round-trip with fixtures and reports.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ── Generation types ────────────────────────────────────────────────

/// Line number and snippet information for a single mutation application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationInfo {
    pub line_number: usize,
    pub original_snippet: String,
    pub mutated_snippet: String,
}

/// A source-level edit operation (byte-range replacement).
#[derive(Debug, Clone)]
pub struct SourceEdit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

/// Analyzed file entry with computed metadata for difficulty scoring.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub content: String,
    pub line_count: usize,
    pub repeated_lines: HashMap<String, Vec<usize>>,
    pub function_count: usize,
    pub density: f64,
    pub max_indent: usize,
}

/// Configuration for the generate subcommand.
#[derive(Debug, Clone)]
pub struct GenerateConfig {
    pub source_dirs: Vec<PathBuf>,
    pub output: PathBuf,
    pub count_per_type: usize,
    pub seed: u64,
    pub categories: Option<Vec<String>>,
    pub difficulties: Vec<String>,
    pub min_score: Option<u32>,
    pub dry_run: bool,
}

// ── Fixture / task types ────────────────────────────────────────────

/// Metadata about a single edit benchmark task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMetadata {
    pub seed: u64,
    pub mutation_type: String,
    pub mutation_category: String,
    pub difficulty: String,
    pub difficulty_score: u32,
    pub file_path: String,
    pub file_name: String,
    pub line_number: usize,
    pub original_snippet: String,
    pub mutated_snippet: String,
}

/// One benchmark task: a buggy source file + prompt + ground-truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditTask {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub files: Vec<String>,
    pub metadata: Option<TaskMetadata>,
    pub input_dir: PathBuf,
    pub expected_dir: PathBuf,
}

// ── Result types ────────────────────────────────────────────────────

/// Token usage statistics for a single run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenStats {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

/// Tool call statistics for a single run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallStats {
    pub read: u64,
    pub edit: u64,
    pub write: u64,
    pub edit_successes: u64,
    pub edit_failures: u64,
    pub edit_warnings: u64,
    pub edit_autocorrects: u64,
}

/// One edit failure event during a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFailure {
    pub tool_call_id: String,
    pub category: String,
    pub error: String,
}

/// Result of running one agent on one task (a single run).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunResult {
    pub run_index: usize,
    pub success: bool,
    pub verification_passed: bool,
    pub mutation_type: Option<String>,
    pub mutation_category: Option<String>,
    pub difficulty_score: Option<u32>,
    pub error: Option<String>,
    pub tokens: TokenStats,
    pub duration_ms: u64,
    pub indent_score: Option<f64>,
    pub formatted_equivalent: Option<bool>,
    pub diff: Option<String>,
    pub tool_calls: ToolCallStats,
    pub edit_failures: Vec<EditFailure>,
    pub edit_autocorrect_count: u64,
    pub early_stopped: bool,
}

/// Aggregated result for one task across N runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub id: String,
    pub name: String,
    pub runs: Vec<TaskRunResult>,
    pub best_run_index: usize,
    pub success: bool,
    pub tokens: TokenStats,
    pub duration_ms: u64,
    pub tool_calls: ToolCallStats,
    pub edit_success_rate: f64,
}

// ── Benchmark summary types ─────────────────────────────────────────

/// Summary for one mutation category.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CategorySummary {
    pub total: usize,
    pub passed: usize,
    pub rate: f64,
    pub avg_difficulty_score: f64,
}

/// Summary for one difficulty level.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DifficultySummary {
    pub total: usize,
    pub passed: usize,
    pub rate: f64,
}

/// Top-level benchmark summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSummary {
    pub total_tasks: usize,
    pub successful_tasks: usize,
    pub task_success_rate: f64,
    pub total_tokens: TokenStats,
    pub avg_tokens_per_task: TokenStats,
    pub total_duration_ms: u64,
    pub avg_duration_per_task_ms: u64,
    pub avg_indent_score: f64,
    pub total_tool_calls: ToolCallStats,
    pub edit_success_rate: f64,
    pub timeout_runs: usize,
    pub ghost_runs: usize,
    pub by_category: HashMap<String, CategorySummary>,
    pub by_difficulty: HashMap<String, DifficultySummary>,
}

/// Configuration for benchmark execution.
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub model: String,
    pub runs_per_task: usize,
    pub task_concurrency: usize,
    pub timeout_ms: u64,
    pub auto_format: bool,
    pub max_attempts: usize,
}

/// Complete benchmark output (config + results + summary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub config: serde_json::Value,
    pub tasks: Vec<TaskResult>,
    pub summary: BenchmarkSummary,
    pub start_time: String,
    pub end_time: String,
}
