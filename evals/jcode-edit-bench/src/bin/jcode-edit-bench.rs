//! `jcode-edit-bench` CLI entry point.
//!
//! Dispatches to generate, run, list, and check subcommands.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use jcode_edit_bench::{
    fixtures::{load_tasks_from_dir, validate_fixtures},
    generate::generate_tasks,
    types::{BenchmarkConfig, GenerateConfig},
    runner::run_benchmark,
    report::{generate_markdown_report, generate_json_report},
};

/// JCode Edit Benchmark — mutation-based edit-tool quality measurement.
#[derive(Debug, Parser)]
#[command(
    name = "jcode-edit-bench",
    about = "Mutation-based edit benchmark harness for measuring edit-tool quality",
    version = "0.1.0"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate benchmark fixtures from Rust source files
    Generate {
        /// Directories containing Rust source files to mutate
        #[arg(long, required = true)]
        source_dirs: Vec<PathBuf>,

        /// Output directory for generated fixtures
        #[arg(short, long, default_value = "fixtures")]
        output: PathBuf,

        /// Cases per mutation type (default: 20)
        #[arg(long, default_value = "20")]
        count_per_type: usize,

        /// RNG seed (default: 42)
        #[arg(long, default_value = "42")]
        seed: u64,

        /// Comma-separated categories (e.g. operator,literal,structural)
        #[arg(long)]
        categories: Option<String>,

        /// Comma-separated difficulty levels (default: easy,medium,hard,nightmare)
        #[arg(long, default_value = "easy,medium,hard,nightmare")]
        difficulty: String,

        /// Minimum difficulty score threshold
        #[arg(long)]
        min_score: Option<u32>,

        /// Print statistics without writing fixtures
        #[arg(long)]
        dry_run: bool,
    },

    /// Run benchmark against fixtures
    Run {
        /// Fixtures directory containing generated tasks
        #[arg(short, long)]
        fixtures: PathBuf,

        /// Model name (e.g. anthropic/claude-sonnet-4-6)
        #[arg(long, default_value = "anthropic/claude-sonnet-4-6")]
        model: String,

        /// Runs per task (best-of-N)
        #[arg(long, default_value = "2")]
        runs: usize,

        /// Timeout per run in milliseconds
        #[arg(long, default_value = "120000")]
        timeout: u64,

        /// Task concurrency
        #[arg(long, default_value = "8")]
        task_concurrency: usize,

        /// Output report path
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Output format: markdown (default) or json
        #[arg(long, default_value = "markdown")]
        format: String,

        /// Maximum prompt attempts per run
        #[arg(long, default_value = "3")]
        max_attempts: usize,

        /// Auto-format output files after verify
        #[arg(long)]
        auto_format: bool,
    },

    /// List available tasks in fixtures
    List {
        /// Path to fixtures directory
        #[arg(short, long)]
        fixtures: PathBuf,
    },

    /// Validate fixture integrity
    Check {
        /// Path to fixtures directory
        #[arg(required = true)]
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Generate {
            source_dirs,
            output,
            count_per_type,
            seed,
            categories,
            difficulty,
            min_score,
            dry_run,
        } => {
            let categories = categories
                .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

            let config = GenerateConfig {
                source_dirs,
                output,
                count_per_type,
                seed,
                categories,
                difficulties: difficulty.split(',').map(|s| s.trim().to_string()).collect(),
                min_score,
                dry_run,
            };

            let tasks = generate_tasks(&config).await?;

            if dry_run {
                println!("Dry run completed: {} potential tasks", tasks.len());
            } else {
                println!("Generated {} tasks in {:?}", tasks.len(), config.output);
            }
        }

        Command::Run {
            fixtures,
            model,
            runs,
            timeout,
            task_concurrency,
            output,
            format,
            max_attempts,
            auto_format,
        } => {
            let config = BenchmarkConfig {
                model,
                runs_per_task: runs.max(1),
                task_concurrency: task_concurrency.max(1),
                timeout_ms: timeout,
                auto_format,
                max_attempts,
            };

            let result = run_benchmark(&fixtures, &config).await?;

            let report = if format == "json" {
                generate_json_report(&result)
            } else {
                generate_markdown_report(&result)
            };

            match output {
                Some(path) => {
                    std::fs::write(&path, &report)?;
                    println!("Report written to {}", path.display());
                }
                None => {
                    println!("{}", report);
                }
            }
        }

        Command::List { fixtures } => {
            let tasks = load_tasks_from_dir(&fixtures)?;

            if tasks.is_empty() {
                println!("No tasks found in {:?}", fixtures);
                return Ok(());
            }

            println!("Available Tasks ({} total):\n", tasks.len());
            for task in &tasks {
                println!("  {}", task.id);
                println!("    Name: {}", task.name);
                println!("    Files: {}", task.files.join(", "));
                if let Some(ref meta) = task.metadata {
                    println!("    Mutation: {} ({})", meta.mutation_type, meta.mutation_category);
                    println!("    Difficulty: {} (score: {})", meta.difficulty, meta.difficulty_score);
                    println!("    Target: {}:{}", meta.file_name, meta.line_number);
                }
                println!();
            }
        }

        Command::Check { path } => {
            let issues = validate_fixtures(&path);
            if issues.is_empty() {
                println!("Fixtures OK — {} tasks validated", load_tasks_from_dir(&path)?.len());
            } else {
                for issue in &issues {
                    eprintln!("  [{}] {}", issue.task_id, issue.message);
                }
                anyhow::bail!("{} fixture issue(s) found", issues.len());
            }
        }
    }

    Ok(())
}
