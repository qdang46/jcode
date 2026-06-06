//! Lessons extractor.
//!
//! After an eval run finishes, the lessons extractor compares the
//! agent's actual diff and trace against the ground-truth diff and
//! distills a small list of [`Lesson`]s describing what went wrong and
//! what the agent should have done instead. These can be appended to a
//! per-agent lessons file and folded back into the agent's system
//! prompt or memory graph.
//!
//! Design source: `/tmp/codebuff/evals/buffbench/lessons-extractor.ts`.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::time::Duration as TokioDuration;

/// Timeout for a lessons extraction call.
const LESSONS_TIMEOUT_SECS: u64 = 20 * 60;

/// One distilled lesson from a single eval run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub what_went_wrong: String,
    pub what_should_have_been_done: String,
}

/// Configuration for lessons extraction.
#[derive(Debug, Clone)]
pub struct LessonsConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub http_client: Option<Client>,
}

impl Default for LessonsConfig {
    fn default() -> Self {
        Self {
            api_base: std::env::var("JBENCH_API_BASE")
                .unwrap_or_else(|_| "https://api.openai.com".to_owned()),
            api_key: std::env::var("JBENCH_API_KEY").unwrap_or_default(),
            model: "gpt-5-2026-05".to_owned(),
            http_client: None,
        }
    }
}

fn render_lessons_prompt(
    prompt: &str,
    ground_truth_diff: &str,
    agent_diff: &str,
    agent_trace: &str,
    judge_summary: Option<&str>,
    error: Option<&str>,
) -> String {
    let judge_section = judge_summary
        .map(|s| format!("\n## Judge Summary\n{s}"))
        .unwrap_or_default();
    let error_section = error
        .map(|e| format!("\n## Agent Error\n{e}"))
        .unwrap_or_default();
    format!(
        "## User Prompt\n{prompt}\n\n\
         ## Ground Truth Changes (One valid implementation)\n\
         ```diff\n{ground_truth_diff}\n```\n\n\
         ## Agent's Changes\n\
         ```diff\n{agent_diff}\n```\n\n\
         ## Agent Trace\n\
         ```json\n{agent_trace}\n```\
         {judge_section}{error_section}\n\n\
         Task: Analyze what went wrong and what should have been done.",
        prompt = prompt,
        ground_truth_diff = ground_truth_diff,
        agent_diff = agent_diff,
        agent_trace = agent_trace,
        judge_section = judge_section,
        error_section = error_section
    )
}

fn lessons_system_prompt() -> &'static str {
    r#"You are a Lesson Extractor. Your job: analyze agent performance and extract actionable lessons.

Context you receive:
- User prompt (what the coding agent was asked)
- Ground truth diffs (one valid solution path)
- The agent's diffs (what they actually changed)
- A truncated agent trace showing HOW they worked
- Optional judge summary (scores, weaknesses)

You must output an array of lessons. Each lesson has two parts:

1. **whatWentWrong**: What the agent did incorrectly, misunderstood, or failed to do
2. **whatShouldHaveBeenDone**: The correct approach the agent should have taken

Rules:
- Each lesson should be a complete learning unit (problem + solution)
- Keep lessons terse but precise (~140 chars per field)
- Do not include things the agent already did correctly
- Focus on gaps that, if filled, would have improved the outcome"#
}

/// Run the lessons-extractor judge over a finished eval run and return
/// zero or more [`Lesson`]s.
pub async fn extract_lessons(
    prompt: &str,
    ground_truth_diff: &str,
    agent_diff: &str,
    agent_trace: &str,
    config: &LessonsConfig,
    judge_summary: Option<&str>,
    error: Option<&str>,
) -> Result<Vec<Lesson>> {
    let prompt_text = render_lessons_prompt(
        prompt,
        ground_truth_diff,
        agent_diff,
        agent_trace,
        judge_summary,
        error,
    );

    let http = match &config.http_client {
        Some(c) => c,
        None => {
            static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
            CLIENT.get_or_init(|| {
                reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(15))
                    .tcp_keepalive(Duration::from_secs(30))
                    .pool_idle_timeout(Duration::from_secs(90))
                    .build()
                    .expect("reqwest client must build")
            })
        }
    };

    let request_body = serde_json::json!({
        "model": &config.model,
        "input": [
            { "role": "system", "content": lessons_system_prompt() },
            { "role": "user", "content": prompt_text }
        ],
        "tools": [
            {
                "type": "function",
                "name": "set_output",
                "description": "Submit lessons derived from this evaluation",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "lessons": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "whatWentWrong": { "type": "string" },
                                    "whatShouldHaveBeenDone": { "type": "string" }
                                },
                                "required": ["whatWentWrong", "whatShouldHaveBeenDone"]
                            }
                        }
                    },
                    "required": ["lessons"]
                }
            }
        ],
        "tool_choice": { "type": "function", "name": "set_output" },
        "output_schema": {
            "type": "object",
            "properties": {
                "lessons": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "whatWentWrong": { "type": "string" },
                            "whatShouldHaveBeenDone": { "type": "string" }
                        },
                        "required": ["whatWentWrong", "whatShouldHaveBeenDone"]
                    }
                }
            },
            "required": ["lessons"]
        },
    });

    let url = format!("{}/v1/responses", config.api_base);
    let response = http
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(TokioDuration::from_secs(LESSONS_TIMEOUT_SECS))
        .send()
        .await
        .context("lessons extraction HTTP request failed")?;

    let body: serde_json::Value = response
        .json()
        .await
        .context("failed to parse lessons extractor response")?;

    let lessons_json = body
        .get("output")
        .and_then(|o| o.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .or_else(|| body.get("output").cloned())
        .unwrap_or(serde_json::json!({ "lessons": [] }));

    let lessons: Vec<Lesson> = lessons_json
        .get("lessons")
        .and_then(|l| l.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();

    Ok(lessons)
}

/// Append `lessons` to the per-agent lessons file at
/// `lessons_dir/<agent_id>.json`, creating the file (and the directory)
/// if needed.
pub fn append_lessons_to_file(
    agent_id: &str,
    lessons: &[Lesson],
    lessons_dir: &Path,
) -> Result<()> {
    if lessons.is_empty() {
        return Ok(());
    }

    if !lessons_dir.exists() {
        fs::create_dir_all(lessons_dir).context("failed to create lessons directory")?;
    }

    let safe_id = agent_id.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
    let file_path = lessons_dir.join(format!("{safe_id}.json"));

    let existing: Vec<Lesson> = if file_path.exists() {
        let contents =
            fs::read_to_string(&file_path).context("failed to read existing lessons file")?;
        serde_json::from_str(&contents).unwrap_or_default()
    } else {
        Vec::new()
    };

    let all_lessons: Vec<Lesson> = existing
        .into_iter()
        .chain(lessons.iter().cloned())
        .collect();

    let json = serde_json::to_string_pretty(&all_lessons).context("failed to serialize lessons")?;

    fs::write(&file_path, json).context("failed to write lessons file")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_lessons_to_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let result = append_lessons_to_file(
            "test-agent",
            &[Lesson {
                what_went_wrong: "forgot null check".to_owned(),
                what_should_have_been_done: "add null guard".to_owned(),
            }],
            tmp.path(),
        );
        assert!(result.is_ok());
        let contents = fs::read_to_string(tmp.path().join("test-agent.json")).unwrap();
        let lessons: Vec<Lesson> = serde_json::from_str(&contents).unwrap();
        assert_eq!(lessons.len(), 1);
    }

    #[test]
    fn append_lessons_accumulates() {
        let tmp = TempDir::new().unwrap();
        let agent = "clone-agent";

        fs::create_dir_all(tmp.path()).unwrap();
        let file_path = tmp.path().join("clone-agent.json");
        let first = vec![Lesson {
            what_went_wrong: "first mistake".to_owned(),
            what_should_have_been_done: "first fix".to_owned(),
        }];
        let json = serde_json::to_string_pretty(&first).unwrap();
        fs::write(&file_path, json).unwrap();

        let second = vec![Lesson {
            what_went_wrong: "second mistake".to_owned(),
            what_should_have_been_done: "second fix".to_owned(),
        }];
        append_lessons_to_file(agent, &second, tmp.path()).unwrap();

        let contents = fs::read_to_string(tmp.path().join("clone-agent.json")).unwrap();
        let lessons: Vec<Lesson> = serde_json::from_str(&contents).unwrap();
        assert_eq!(lessons.len(), 2);
    }
}
