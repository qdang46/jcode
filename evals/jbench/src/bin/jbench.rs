//! `jbench` CLI entry point.
//!
//! Dispatches to the [`jcode_jbench`] library for real work.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[cfg(feature = "agent-runner")]
use jcode_jbench::agent_runner::AgentRunConfig;
#[cfg(feature = "agent-runner")]
use jcode_jbench::types::EvalDataV2;
use jcode_jbench::types::EvalRun;

/// Top-level `jbench` CLI.
#[derive(Debug, Parser)]
#[command(
    name = "jbench",
    about = "JBench — jcode's git-commit-reconstruction eval framework",
    version
)]
struct Cli {
    /// Subcommand to dispatch to.
    #[command(subcommand)]
    command: Command,
}

/// JBench subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Select high-quality commits from a target repo to use as eval
    /// tasks.
    PickCommits {
        /// URL of the repository to pick commits from.
        repo_url: String,
        /// Minimum commit message length.
        #[arg(long, default_value = "10")]
        min_msg_len: usize,
        /// Maximum number of commits to pick.
        #[arg(long, default_value = "50")]
        max_picks: usize,
        /// Output file (default: stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Generate an `eval-{repo}.json` file (`EvalDataV2`) from a list
    /// of picked commits.
    GenEvals {
        /// Input commit list (from pick-commits).
        input: PathBuf,
        /// Output eval JSON file.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Run one or more agents against an eval data file and emit
    /// per-commit `EvalRun`s.
    Run {
        /// Path to eval data JSON file.
        eval_file: PathBuf,
        /// Agent ID to run (must be registered in jcode registry).
        #[arg(short, long)]
        agent_id: String,
        /// Output directory for EvalRun JSON files.
        #[arg(short, long)]
        output_dir: PathBuf,
        /// Path to jcode binary (auto-detected if not set).
        #[arg(long)]
        jcode_binary: Option<PathBuf>,
        /// Maximum turns per run.
        #[arg(long, default_value = "100")]
        max_turns: u32,
        /// Timeout per run in seconds.
        #[arg(long, default_value = "3600")]
        timeout_secs: u64,
    },
    /// Re-judge an existing run with the three-judge median pipeline.
    Judge {
        /// Directory containing EvalRun JSON files.
        runs_dir: PathBuf,
        /// API base URL.
        #[arg(long, env = "JBENCH_API_BASE")]
        api_base: Option<String>,
        /// API key.
        #[arg(long, env = "JBENCH_API_KEY")]
        api_key: Option<String>,
    },
    /// Aggregate and analyze results across all tasks for an agent.
    MetaAnalyze {
        /// Directory containing EvalRun JSON files.
        runs_dir: PathBuf,
        /// Output file for aggregated results.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::PickCommits {
            repo_url,
            min_msg_len,
            max_picks,
            output,
        } => {
            pick_commits_impl(&repo_url, min_msg_len, max_picks, output).await?;
        }
        Command::GenEvals { input, output } => {
            gen_evals_impl(&input, &output).await?;
        }
        Command::Run {
            eval_file: _eval_file,
            agent_id: _agent_id,
            output_dir: _output_dir,
            jcode_binary: _jcode_binary,
            max_turns: _max_turns,
            timeout_secs: _timeout_secs,
        } => {
            #[cfg(feature = "agent-runner")]
            {
                run_impl(
                    &_eval_file,
                    &_agent_id,
                    &_output_dir,
                    _jcode_binary.as_ref(),
                    _max_turns,
                    _timeout_secs,
                )
                .await?;
            }
            #[cfg(not(feature = "agent-runner"))]
            anyhow::bail!(
                "'jbench run' requires the 'agent-runner' feature. Enable with: cargo build --features agent-runner"
            );
        }
        Command::Judge {
            runs_dir,
            api_base,
            api_key,
        } => {
            judge_impl(&runs_dir, api_base.as_deref(), api_key.as_deref()).await?;
        }
        Command::MetaAnalyze { runs_dir, output } => {
            meta_analyze_impl(&runs_dir, output.as_ref()).await?;
        }
    }
    Ok(())
}

async fn pick_commits_impl(
    repo_path: &str,
    min_msg_len: usize,
    max_picks: usize,
    output: Option<PathBuf>,
) -> Result<()> {
    // Verify the path is a git repository.
    let check = std::process::Command::new("git")
        .args(["-C", repo_path, "rev-parse", "--is-inside-work-tree"])
        .output()
        .context("failed to run git rev-parse")?;
    if !check.status.success() {
        anyhow::bail!("{} is not a git repository", repo_path);
    }

    // Get commit log: SHA, first parent, subject, then shortstat on the
    // following line.  `COMMIT` acts as a block separator.
    let log_out = std::process::Command::new("git")
        .args([
            "-C",
            repo_path,
            "log",
            "--format=COMMIT%n%H%n%P%n%s",
            "--shortstat",
        ])
        .output()
        .context("failed to run git log")?;

    if !log_out.status.success() {
        let stderr = String::from_utf8_lossy(&log_out.stderr);
        anyhow::bail!("git log failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&log_out.stdout);
    let mut picked: Vec<serde_json::Value> = Vec::new();

    for block in stdout.split("COMMIT\n").skip(1) {
        let lines: Vec<&str> = block.lines().collect();
        if lines.len() < 3 {
            continue;
        }

        let sha = lines[0].trim();
        let parent_sha = lines[1].split_whitespace().next().unwrap_or("").to_string();
        let subject = lines[2].trim();

        // Skip root commits (no parent).
        if parent_sha.is_empty() {
            continue;
        }

        // Filter: commit message must meet minimum length.
        if subject.len() < min_msg_len {
            continue;
        }

        // Parse file count from shortstat (e.g. " 3 files changed, …").
        let file_count = lines
            .iter()
            .rev()
            .find(|l| l.contains(" file"))
            .and_then(|l| l.split_whitespace().next()?.parse::<usize>().ok())
            .unwrap_or(0);

        // Filter: bounded scope — not zero files, not a mega-commit.
        if file_count == 0 || file_count > 10 {
            continue;
        }

        picked.push(serde_json::json!({
            "sha": sha,
            "parent_sha": parent_sha,
            "spec": subject,
            "prompt": subject,
        }));

        if picked.len() >= max_picks {
            break;
        }
    }

    let json = serde_json::to_string_pretty(&picked)?;
    if let Some(path) = output {
        std::fs::write(&path, &json)?;
        eprintln!("Wrote {} commits to {}", picked.len(), path.display());
    } else {
        println!("{json}");
    }

    Ok(())
}

async fn gen_evals_impl(input: &PathBuf, output: &PathBuf) -> Result<()> {
    use jcode_jbench::types::{EvalCommit, EvalDataV2};

    // Intermediate struct matching the pick-commits output format.
    #[derive(serde::Deserialize)]
    struct PickedCommit {
        sha: String,
        parent_sha: String,
        spec: String,
        prompt: String,
    }

    // Read input JSON.
    let input_text = std::fs::read_to_string(input)
        .with_context(|| format!("failed to read input file {}", input.display()))?;
    let picked: Vec<PickedCommit> = serde_json::from_str(&input_text)
        .context("failed to parse input JSON as array of picked commits")?;

    if picked.is_empty() {
        anyhow::bail!("input file contains no commits");
    }

    // Detect repo URL from the local git remote.
    let repo_url = get_repo_url().unwrap_or_else(|| "unknown".to_owned());

    let mut eval_commits = Vec::with_capacity(picked.len());

    for pc in &picked {
        let id = format!("{}-eval", &pc.sha[..std::cmp::min(8, pc.sha.len())]);

        // git diff --name-status to get file statuses.
        let name_status = run_git(&[
            "diff",
            "--name-status",
            &format!("{}..{}", pc.parent_sha, pc.sha),
        ])
        .with_context(|| {
            format!(
                "git diff --name-status failed for {}..{}",
                pc.parent_sha, pc.sha
            )
        })?;

        // git diff to get the full unified diff.
        let full_diff = run_git(&["diff", &format!("{}..{}", pc.parent_sha, pc.sha)])
            .with_context(|| format!("git diff failed for {}..{}", pc.parent_sha, pc.sha))?;

        let file_diffs = parse_diffs(&name_status, &full_diff);

        eval_commits.push(EvalCommit {
            id,
            sha: pc.sha.clone(),
            parent_sha: pc.parent_sha.clone(),
            spec: pc.spec.clone(),
            prompt: pc.prompt.clone(),
            supplemental_files: Vec::new(),
            file_diffs,
        });
    }

    let eval_data = EvalDataV2 {
        repo_url,
        test_repo_name: None,
        generation_date: chrono_now(),
        init_command: None,
        env: std::collections::HashMap::new(),
        final_check_commands: Vec::new(),
        eval_commits,
    };

    let json =
        serde_json::to_string_pretty(&eval_data).context("failed to serialize EvalDataV2")?;
    std::fs::write(output, &json)
        .with_context(|| format!("failed to write output file {}", output.display()))?;

    println!(
        "Wrote {} eval commits to {}",
        eval_data.eval_commits.len(),
        output.display()
    );
    Ok(())
}

#[cfg(feature = "agent-runner")]
async fn run_impl(
    eval_file: &PathBuf,
    agent_id: &str,
    output_dir: &PathBuf,
    jcode_binary: Option<&PathBuf>,
    max_turns: u32,
    timeout_secs: u64,
) -> Result<()> {
    use std::fs;
    use std::time::Duration;
    use tokio::time::timeout as tk_timeout;

    // Load eval data
    let eval_data: EvalDataV2 = {
        let text = fs::read_to_string(eval_file)?;
        serde_json::from_str(&text).context("failed to parse eval JSON")?
    };

    if !output_dir.exists() {
        fs::create_dir_all(output_dir)?;
    }

    for commit in &eval_data.eval_commits {
        let config = AgentRunConfig {
            agent_id: agent_id.to_owned(),
            prompt: commit.prompt.clone(),
            repo_path: output_dir.join(&commit.id),
            max_turns,
            timeout_secs,
            env: eval_data.env.clone(),
            jcode_binary: jcode_binary.cloned(),
            ..Default::default()
        };

        let result = match tk_timeout(
            Duration::from_secs(timeout_secs),
            jcode_jbench::agent_runner::run_agent_in_repo(config),
        )
        .await
        {
            Ok(Ok(run)) => run,
            Ok(Err(err)) => EvalRun {
                commit_sha: commit.sha.clone(),
                prompt: commit.prompt.clone(),
                diff: String::new(),
                judging: Default::default(),
                cost_usd: 0.0,
                duration_ms: 0,
                error: Some(format!("Agent error: {err:#}")),
            },
            Err(_elapsed) => EvalRun {
                commit_sha: commit.sha.clone(),
                prompt: commit.prompt.clone(),
                diff: String::new(),
                judging: Default::default(),
                cost_usd: 0.0,
                duration_ms: 0,
                error: Some("Timed out waiting for run_agent_in_repo".to_owned()),
            },
        };

        let run_file = output_dir.join(format!("{}.run.json", commit.id));
        let json = serde_json::to_string_pretty(&result).context("failed to serialize EvalRun")?;
        fs::write(&run_file, json)?;
        println!("Wrote {}", run_file.display());
    }

    Ok(())
}

async fn judge_impl(
    _runs_dir: &PathBuf,
    _api_base: Option<&str>,
    _api_key: Option<&str>,
) -> Result<()> {
    todo_step(
        "Phase 5.4: load EvalRun JSONs, call judge_with_three_models, overwrite judging fields",
    )
}

async fn meta_analyze_impl(runs_dir: &PathBuf, output: Option<&PathBuf>) -> Result<()> {
    use jcode_jbench::types::AgentEvalResults;
    use std::fs;

    let mut all_runs = Vec::new();

    for entry in fs::read_dir(runs_dir)? {
        let entry = entry?;
        let path = entry.path();
        // `Path::extension` returns only the trailing component (`json`),
        // so matching against `"run.json"` never fires. Match on the full
        // file name suffix instead.
        let is_run_file = path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.ends_with(".run.json"));
        if is_run_file {
            let text = fs::read_to_string(&path)?;
            if let Ok(run) = serde_json::from_str::<EvalRun>(&text) {
                all_runs.push(run);
            }
        }
    }

    if all_runs.is_empty() {
        anyhow::bail!("No .run.json files found in {}", runs_dir.display());
    }

    let avg_score = all_runs
        .iter()
        .map(|r| r.judging.overall_score)
        .sum::<f64>()
        / all_runs.len() as f64;
    let avg_cost = all_runs.iter().map(|r| r.cost_usd).sum::<f64>() / all_runs.len() as f64;
    let avg_duration = (all_runs.iter().map(|r| r.duration_ms as f64).sum::<f64>()
        / all_runs.len() as f64)
        .round() as u64;

    let summary = AgentEvalResults {
        agent_id: "unknown".to_owned(),
        runs: all_runs,
        average_score: (avg_score * 10.0).round() / 10.0,
        average_cost: (avg_cost * 100.0).round() / 100.0,
        average_duration_ms: avg_duration,
    };

    let json = serde_json::to_string_pretty(&summary).context("failed to serialize summary")?;

    if let Some(out) = output {
        fs::write(out, &json)?;
        println!("Wrote {}", out.display());
    } else {
        println!("{json}");
    }

    Ok(())
}

fn todo_step(phase: &str) -> Result<()> {
    eprintln!("{phase}");
    std::process::exit(2);
}

/// Run a `git` subcommand and return its stdout as a `String`.
fn run_git(args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .context("failed to spawn git")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Try to detect the repo URL from `git remote get-url origin`.
fn get_repo_url() -> Option<String> {
    std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// ISO-8601 timestamp without pulling in a full datetime crate.
fn chrono_now() -> String {
    // Use a simple approach: seconds since epoch formatted manually
    // would be ideal, but for simplicity just use a debug-friendly format.
    // The `chrono` crate isn't in deps, so we format from SystemTime.
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Break into Y-M-D H:M:S (UTC, simplified leap-year handling).
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;
    // Days since 1970-01-01 -> Y/M/D via a simple civil calendar.
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).
/// Uses Howard Hinnant's algorithm.
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

/// Parse `git diff --name-status` output and the full unified diff into
/// `FileDiff` structs.
///
/// The name-status output gives us file paths and status codes; we split
/// the full diff by file to associate each chunk with the right file.
fn parse_diffs(name_status: &str, full_diff: &str) -> Vec<jcode_jbench::types::FileDiff> {
    use jcode_jbench::types::{FileDiff, FileDiffStatus};

    // Parse name-status lines: e.g. "M\tpath/to/file.rs" or "R100\told\tnew".
    let mut file_entries: Vec<(FileDiffStatus, String, Option<String>)> = Vec::new();
    for line in name_status.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let code = parts[0];
        let (status, path, old_path) = match code {
            "M" => (FileDiffStatus::Modified, parts[1].to_owned(), None),
            "A" => (FileDiffStatus::Added, parts[1].to_owned(), None),
            "D" => (FileDiffStatus::Deleted, parts[1].to_owned(), None),
            r if r.starts_with('R') => {
                // Renamed: "R100\told_path\tnew_path"
                if parts.len() >= 3 {
                    (
                        FileDiffStatus::Renamed,
                        parts[2].to_owned(),
                        Some(parts[1].to_owned()),
                    )
                } else {
                    (FileDiffStatus::Modified, parts[1].to_owned(), None)
                }
            }
            "C" => {
                // Copied — treat as Added for our purposes.
                let path = if parts.len() >= 3 { parts[2] } else { parts[1] };
                (FileDiffStatus::Added, path.to_owned(), None)
            }
            _ => (FileDiffStatus::Modified, parts[1].to_owned(), None),
        };
        file_entries.push((status, path, old_path));
    }

    // Split the full diff by "diff --git" boundaries to get per-file chunks.
    let file_diffs_map = split_diff_by_file(full_diff);

    // Build FileDiff structs, matching by path.
    let mut result = Vec::with_capacity(file_entries.len());
    for (status, path, old_path) in file_entries {
        let diff_text = file_diffs_map.get(&path).cloned().unwrap_or_default();
        result.push(FileDiff {
            path,
            status,
            old_path,
            diff: diff_text,
        });
    }

    result
}

/// Split a unified diff into per-file chunks keyed by the post-image path.
fn split_diff_by_file(full_diff: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let mut current_path: Option<String> = None;
    let mut current_chunk = String::new();

    for line in full_diff.lines() {
        if line.starts_with("diff --git ") {
            // Save previous chunk.
            if let Some(ref p) = current_path {
                map.insert(p.clone(), current_chunk.clone());
            }
            // Extract the post-image path from "diff --git a/path b/path".
            let path = line.splitn(2, " b/").nth(1).unwrap_or("").to_owned();
            current_path = Some(path);
            current_chunk.clear();
        }
        if current_path.is_some() {
            current_chunk.push_str(line);
            current_chunk.push('\n');
        }
    }
    // Don't forget the last chunk.
    if let Some(p) = current_path {
        map.insert(p, current_chunk);
    }

    map
}
