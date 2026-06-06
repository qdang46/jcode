//! Three-judge median pipeline.
//!
//! Each agent diff is graded by **three** frontier models in parallel
//! (planned slate: `gpt-5`, `gemini-pro`, `claude-sonnet`); the median
//! `overall_score` selects which judge's qualitative analysis is
//! reported, while the per-dimension scores are averaged across all
//! valid judges. This mirrors the design of BuffBench's
//! `judgeCommitResult` in `/tmp/codebuff/evals/buffbench/judge.ts`.
//!
//! Judge prompts are rendered from fixed templates (deduced from the TS
//! original); the judge agent definitions are embedded here so the
//! pipeline stays self-contained and does not depend on the full jcode
//! agent runtime at evaluation time.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;

// Re-export JudgingResult so callers get it from the public types.
pub use crate::types::JudgingResult;

use crate::types::{EvalCommit, JudgingResult as Scorecard};

/// Timeout for a single judge call.
const JUDGE_TIMEOUT_SECS: u64 = 20 * 60;

/// How many judges must succeed for the pipeline to produce a result.
/// If fewer succeed, we return a zero-score error result.
const MIN_JUDGE_SUCCESS_COUNT: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgeProviderKind {
    OpenAI,    // OpenAI Responses API + output_schema
    Anthropic, // Anthropic Messages API + structured_outputs
}

impl JudgeProviderKind {
    pub fn for_model(model: &str) -> Self {
        if model.contains("claude") || model.contains("anthropic") {
            Self::Anthropic
        } else {
            Self::OpenAI
        }
    }
}

/// Configuration for the judging pipeline.
#[derive(Debug, Clone)]
pub struct JudgeConfig {
    /// API base URL for the OpenAI-compatible judge backend.
    pub api_base: String,
    /// API key for the OpenAI-compatible judge backend.
    pub api_key: String,
    /// Optional separate base URL for Anthropic-routed judges (e.g.
    /// `https://api.anthropic.com`). Falls back to `api_base` when
    /// `None`, which only makes sense if the OpenAI-compatible host
    /// proxies the Anthropic Messages API too.
    pub anthropic_api_base: Option<String>,
    /// Optional separate API key for Anthropic-routed judges. Falls
    /// back to `api_key` when `None`.
    pub anthropic_api_key: Option<String>,
    /// Model IDs for the three judges. Order determines the median
    /// computation.
    pub models: [String; 3],
    /// Optional override for judge timeout per call.
    pub timeout_secs: Option<u64>,
    /// Custom HTTP client (uses shared client if None).
    pub http_client: Option<Client>,
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            // Sensible defaults — override before use in production
            api_base: std::env::var("JBENCH_API_BASE")
                .unwrap_or_else(|_| "https://api.openai.com".to_owned()),
            api_key: std::env::var("JBENCH_API_KEY").unwrap_or_default(),
            anthropic_api_base: std::env::var("JBENCH_ANTHROPIC_API_BASE").ok(),
            anthropic_api_key: std::env::var("JBENCH_ANTHROPIC_API_KEY").ok(),
            models: [
                "gpt-5-2026-05".to_owned(),
                "google/gemini-3.1-pro".to_owned(),
                "anthropic/claude-sonnet-4-2026-05".to_owned(),
            ],
            timeout_secs: None,
            http_client: None,
        }
    }
}

/// Render the full judge prompt from commit + diff + context.
fn render_judge_prompt(
    commit: &EvalCommit,
    agent_diff: &str,
    context_files: &HashMap<String, String>,
) -> String {
    let ground_truth_diffs = commit
        .file_diffs
        .iter()
        .map(|fd| format!("### {}\n```diff\n{}\n```", fd.path, fd.diff))
        .collect::<Vec<_>>()
        .join("\n\n");

    let context_content = context_files
        .iter()
        .map(|(path, content)| format!("### {path}\n```\n{content}\n```"))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "## User Prompt (What the agent was asked to do)\n{}\n\n## Context Files (from parent commit)\n{}\n\n## Ground Truth Changes (One valid implementation)\n{}\n\n## Agent's Changes (What the agent actually did)\n```diff\n{}\n```",
        commit.prompt, context_content, ground_truth_diffs, agent_diff
    )
}

/// System prompt for the judge agent (mirrors the TS `judgeAgentBase.systemPrompt`).
fn judge_system_prompt() -> &'static str {
    r#"You are an expert software engineer evaluating AI-generated code changes with empathy for the task given.

## Your Role

You will receive:
1. The user prompt that the coding agent was given
2. Context files from the codebase
3. The ground truth changes (expected outcome)
4. The agent's actual changes

## Evaluation Philosophy

**Judge based on what the agent was asked to do, not on perfection.**

- If the prompt is vague or high-level (e.g., "add authentication"), be lenient and accept any reasonable implementation that achieves the goal
- If the prompt is specific and detailed, expect the implementation to match those details more closely
- Focus on whether the agent understood and addressed the user's intent
- Consider that there are often multiple valid ways to implement the same feature

## Evaluation Criteria

- **Completion** (0-10): How well did the agent address what was asked in the prompt? Consider the specificity of the prompt.
- **Code Quality** (0-10): How well-structured and maintainable is the code?
- **Overall** (0-10): Combined assessment of whether the agent successfully completed the task as requested

## Ground Truth

The ground truth shows ONE valid implementation, but it's not the only correct answer. The agent's implementation should be judged on:
- Does it achieve the same functional outcome?
- Is it a reasonable approach given the prompt?
- Does it maintain code quality?

Provide detailed analysis, strengths, weaknesses, and numerical scores."#
}

#[derive(Serialize)]
struct JudgeRequest<'a> {
    model: &'a str,
    input: &'a str,
    tools: &'a [serde_json::Value],
    #[serde(skip_serializing_if = "Option::is_none")]
    output_schema: Option<&'a serde_json::Value>,
}

#[derive(Deserialize)]
struct JudgeResponse {
    output: Option<serde_json::Value>,
    #[serde(default)]
    choices: Vec<serde_json::Value>,
}

/// Invoke a single judge model with a fully-rendered prompt.
///
/// `anthropic_api_base` / `anthropic_api_key` are only consulted when
/// the model routes through `JudgeProviderKind::Anthropic`; OpenAI-bound
/// requests always use the primary `api_base` / `api_key`.
///
/// Design source: `/tmp/codebuff/evals/buffbench/judge.ts` (`runSingleJudge`).
pub async fn run_single_judge(
    model: &str,
    prompt: &str,
    api_base: &str,
    api_key: &str,
    anthropic_api_base: Option<&str>,
    anthropic_api_key: Option<&str>,
    http_client: &Client,
) -> Result<Scorecard> {
    let kind = JudgeProviderKind::for_model(model);
    let system = judge_system_prompt();

    if kind == JudgeProviderKind::OpenAI {
        run_openai_judge(model, prompt, system, api_base, api_key, http_client).await
    } else {
        // Fall back to the primary host/key only if no Anthropic-specific
        // overrides were configured. The caller is expected to set both
        // overrides when targeting `api.anthropic.com` directly.
        let base = anthropic_api_base.unwrap_or(api_base);
        let key = anthropic_api_key.unwrap_or(api_key);
        run_anthropic_judge(model, prompt, system, base, key, http_client).await
    }
}

async fn run_openai_judge(
    model: &str,
    prompt: &str,
    system: &str,
    api_base: &str,
    api_key: &str,
    http_client: &Client,
) -> Result<Scorecard> {
    let output_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "analysis": { "type": "string", "description": "Detailed analysis comparing agent changes to ground truth" },
            "strengths": { "type": "array", "items": { "type": "string" }, "description": "Key strengths of the implementation" },
            "weaknesses": { "type": "array", "items": { "type": "string" }, "description": "Key weaknesses or issues found" },
            "completionScore": { "type": "number", "minimum": 0, "maximum": 10, "description": "How completely the prompt was addressed" },
            "codeQualityScore": { "type": "number", "minimum": 0, "maximum": 10, "description": "Code structure and maintainability" },
            "overallScore": { "type": "number", "minimum": 0, "maximum": 10, "description": "Combined assessment" }
        },
        "required": ["analysis", "strengths", "weaknesses", "completionScore", "codeQualityScore", "overallScore"]
    });

    let request_body = serde_json::json!({
        "model": model,
        "input": [
            { "role": "system", "content": system },
            { "role": "user", "content": prompt }
        ],
        "tools": [
            {
                "type": "function",
                "name": "set_output",
                "description": "Submit the evaluation result",
                "parameters": output_schema.clone()
            }
        ],
        "tool_choice": { "type": "function", "name": "set_output" },
        "output_schema": output_schema,
    });

    let url = format!("{api_base}/v1/responses");
    let response = http_client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(Duration::from_secs(JUDGE_TIMEOUT_SECS))
        .send()
        .await
        .context("judge HTTP request failed")?;

    let status = response.status();
    let body: serde_json::Value = response
        .json()
        .await
        .context("failed to parse judge response")?;

    if !status.is_success() {
        anyhow::bail!("judge API returned {status}: {body}");
    }

    let output = body
        .get("output")
        .and_then(|o| o.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str());

    let output_value = output
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .or_else(|| body.get("output").cloned())
        .unwrap_or(serde_json::json!({
            "analysis": "No structured output received",
            "strengths": [],
            "weaknesses": ["Judge failed to return structured output"],
            "completionScore": 0,
            "codeQualityScore": 0,
            "overallScore": 0
        }));

    parse_scorecard(output_value)
}

async fn run_anthropic_judge(
    model: &str,
    prompt: &str,
    system: &str,
    api_base: &str,
    api_key: &str,
    http_client: &Client,
) -> Result<Scorecard> {
    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "user", "content": prompt }
        ],
        "system": system,
        "max_tokens": 4096,
        "thinking": {
            "type": "enabled",
            "budget_tokens": 1024
        },
    });

    // Anthropic Messages API authenticates via `x-api-key`, not
    // `Authorization: Bearer ...`. Using the wrong header returns 401
    // even with a valid key, which previously made this branch
    // permanently dead.
    let url = format!("{api_base}/v1/messages");
    let response = http_client
        .post(&url)
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&request_body)
        .timeout(Duration::from_secs(JUDGE_TIMEOUT_SECS))
        .send()
        .await
        .context("judge HTTP request failed")?;

    let body: serde_json::Value = response
        .json()
        .await
        .context("failed to parse anthropic judge response")?;

    // Anthropic returns content blocks — try to parse the final text block as JSON
    let text = body
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.last())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or_default();

    let parsed = serde_json::from_str::<serde_json::Value>(text).unwrap_or(serde_json::json!({
        "analysis": text.to_owned(),
        "strengths": [],
        "weaknesses": ["Could not parse structured output from Anthropic judge"],
        "completionScore": 0,
        "codeQualityScore": 0,
        "overallScore": 0
    }));

    parse_scorecard(parsed)
}

fn parse_scorecard(value: serde_json::Value) -> Result<Scorecard> {
    serde_json::from_value(value).context("failed to parse JudgingResult from judge output")
}

/// Judge an agent's diff against the ground truth using three models in
/// parallel and return a [`JudgingResult`] whose qualitative analysis
/// comes from the median judge and whose numeric scores are averaged
/// across all judges that returned successfully.
///
/// Design source: `/tmp/codebuff/evals/buffbench/judge.ts`
/// (`judgeCommitResult`).
pub async fn judge_with_three_models(
    commit: &EvalCommit,
    agent_diff: &str,
    context_files: &HashMap<String, String>,
    config: &JudgeConfig,
) -> Result<JudgingResult> {
    let prompt = render_judge_prompt(commit, agent_diff, context_files);
    let http: &reqwest::Client = match &config.http_client {
        Some(c) => c,
        None => shared_client(),
    };

    let timeout_duration = Duration::from_secs(config.timeout_secs.unwrap_or(JUDGE_TIMEOUT_SECS));

    // Each judge gets its own timeout so a slow model doesn't starve the others.
    let judge_futures: Vec<_> = config
        .models
        .iter()
        .map(|model| {
            let http = http.clone();
            let prompt = prompt.clone();
            async move {
                timeout(
                    timeout_duration,
                    run_single_judge(
                        model,
                        &prompt,
                        &config.api_base,
                        &config.api_key,
                        config.anthropic_api_base.as_deref(),
                        config.anthropic_api_key.as_deref(),
                        &http,
                    ),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
            }
        })
        .collect();

    let valid: Vec<Scorecard> = futures::future::join_all(judge_futures)
        .await
        .into_iter()
        .filter_map(|r| r)
        .collect();

    if valid.len() < MIN_JUDGE_SUCCESS_COUNT {
        return Ok(Scorecard {
            analysis: format!(
                "Error running judge agent — only {}/{} judges succeeded",
                valid.len(),
                3
            ),
            strengths: vec![],
            weaknesses: vec![format!("Only {}/{} judges succeeded", valid.len(), 3)],
            completion_score: 0.0,
            code_quality_score: 0.0,
            overall_score: 0.0,
        });
    }

    // Median analysis — sort by overall_score and pick the middle
    let mut sorted = valid.clone();
    sorted.sort_by(|a, b| {
        a.overall_score
            .partial_cmp(&b.overall_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let median_idx = sorted.len() / 2;
    let median = &sorted[median_idx];

    let avg_completion = valid.iter().map(|r| r.completion_score).sum::<f64>() / valid.len() as f64;
    let avg_quality = valid.iter().map(|r| r.code_quality_score).sum::<f64>() / valid.len() as f64;
    let avg_overall = valid.iter().map(|r| r.overall_score).sum::<f64>() / valid.len() as f64;

    Ok(Scorecard {
        analysis: median.analysis.clone(),
        strengths: median.strengths.clone(),
        weaknesses: median.weaknesses.clone(),
        completion_score: (avg_completion * 10.0).round() / 10.0,
        code_quality_score: (avg_quality * 10.0).round() / 10.0,
        overall_score: (avg_overall * 10.0).round() / 10.0,
    })
}

static SHARED_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn shared_client() -> &'static Client {
    SHARED_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .tcp_keepalive(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .expect("reqwest client must build")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judge_provider_kind_for_model() {
        assert_eq!(
            JudgeProviderKind::for_model("gpt-5"),
            JudgeProviderKind::OpenAI
        );
        assert_eq!(
            JudgeProviderKind::for_model("claude-sonnet-4"),
            JudgeProviderKind::Anthropic
        );
        assert_eq!(
            JudgeProviderKind::for_model("anthropic/claude-opus-4"),
            JudgeProviderKind::Anthropic
        );
    }

    /// Locks the wire-format contract: the LLM judge returns camelCase
    /// (`completionScore`, etc.) per the request schema. Deserialization
    /// must accept that even though the on-disk JSON form is snake_case.
    #[test]
    fn parse_scorecard_accepts_camelcase_from_llm() {
        let camel = serde_json::json!({
            "analysis": "looks good",
            "strengths": ["clean diff"],
            "weaknesses": [],
            "completionScore": 8.5,
            "codeQualityScore": 7.0,
            "overallScore": 7.8
        });
        let parsed = parse_scorecard(camel).expect("camelCase must deserialize");
        assert_eq!(parsed.completion_score, 8.5);
        assert_eq!(parsed.code_quality_score, 7.0);
        assert_eq!(parsed.overall_score, 7.8);
    }

    /// snake_case (on-disk eval JSON) must round-trip as well.
    #[test]
    fn parse_scorecard_accepts_snake_case_from_disk() {
        let snake = serde_json::json!({
            "analysis": "",
            "strengths": [],
            "weaknesses": [],
            "completion_score": 1.0,
            "code_quality_score": 2.0,
            "overall_score": 3.0
        });
        let parsed = parse_scorecard(snake).expect("snake_case must deserialize");
        assert_eq!(parsed.completion_score, 1.0);
        assert_eq!(parsed.code_quality_score, 2.0);
        assert_eq!(parsed.overall_score, 3.0);
    }
}
