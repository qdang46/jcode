//! Best-of-N editing orchestrator runner.
//!
//! Spawns N parallel candidate subagents, collects their proposed content
//! from the shared `ProposedContentStore`, runs the deterministic selector,
//! and applies the winner's diffs to real files on disk.
//!
//! The pure orchestrator lives in `jcode-best-of-n`; this module is the
//! glue that bridges the orchestrator to jcode's agent runtime (which
//! lives in `jcode-app-core` and depends on this crate's types).
//!
//! Per-candidate subagents are spawned via `Agent::new_with_session` with
//! a restricted tool surface — they can read and propose, but cannot
//! apply edits directly. The `propose_edit` / `propose_write` tools
//! require `best_of_n_run_id` / `best_of_n_candidate_id` on their
//! `ToolContext` (see `Agent::set_best_of_n_context`), which the
//! orchestrator sets before launching each candidate.

use crate::agent::Agent;
use crate::bus::{Bus, BusEvent, FileOp, FileTouch, SidePanelUpdated};
use crate::provider::Provider;
use crate::session::Session;
use crate::tool::{
    BestOfNOrchestratorHandle, Registry, Tool, ToolContext, ToolOutput, clear_best_of_n_handle,
    set_best_of_n_handle,
};
use anyhow::{Context as _, Result};
use async_trait::async_trait;
use jcode_best_of_n::store::OriginalFileEntry;
use jcode_best_of_n::{BestOfNResult, CandidateDiff, CandidateStatus};
use jcode_hooks::{
    DispatchConfig, HookContext, HookEvent, HookInputBuilder, HookRegistry, load_hooks_config,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

/// Tools that a best-of-N candidate must never have access to.
///
/// These would let the candidate apply edits directly to disk, bypassing
/// the propose/store/select/apply pipeline. They are removed from the
/// subagent's allowed tool set so that the only way to record a change
/// is through `propose_edit` / `propose_write`.
const FORBIDDEN_TOOLS: &[&str] = &[
    "edit",
    "write",
    "multiedit",
    "patch",
    "apply_patch",
    "hashline_edit",
    "subagent",
    "batch",
    "best_of_n",
    "selfdev",
    "debug_socket",
];

/// Ambient-only tools excluded from candidate subagents. The candidates
/// are not ambient sessions and have no business sending channel
/// messages or requesting elevated permissions.
const AMBIENT_TOOLS: &[&str] = &[
    "end_ambient_cycle",
    "schedule_ambient",
    "request_permission",
    "send_message",
];

/// Top-level tool that agents call to trigger a best-of-N edit.
///
/// Schema: `{ prompt, file_paths[], show_mode? }`
///
/// - Auto mode: runs N candidates in parallel, selects the winner
///   via deterministic scoring, and applies it to disk.
/// - Show mode: returns the full `BestOfNResult` as JSON output so a
///   UI picker can let the user choose.
/// - Off mode: returns an error immediately (shouldn't be called).
pub struct BestOfNTool {
    provider: Arc<dyn Provider>,
    registry: Registry,
}

impl BestOfNTool {
    pub fn new(provider: Arc<dyn Provider>, registry: Registry) -> Self {
        Self { provider, registry }
    }
}

#[async_trait]
impl Tool for BestOfNTool {
    fn name(&self) -> &str {
        "best_of_n"
    }

    fn description(&self) -> &str {
        "Run N parallel edits with different strategies, select the best result, and apply it. \
         Use for high-quality edits where multiple approaches should be compared."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["prompt", "file_paths"],
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The edit task to perform in N parallel attempts."
                },
                "file_paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "File paths to read as context for the candidate agents."
                },
                "show_mode": {
                    "type": "boolean",
                    "description": "Override config to force Show mode (return candidates for user selection)."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let config = crate::config::config().best_of_n.clone();
        let mode = config.mode;

        // Off mode: refuse with a clear error.
        if matches!(mode, jcode_best_of_n::BestOfNMode::Off) {
            return Err(anyhow::anyhow!(
                "Best-of-N editing is disabled (mode=off). Enable it in config.toml under [best_of_n]."
            ));
        }

        let params: BestOfNInput = serde_json::from_value(input)?;

        // Read original file contents for diff computation.
        let mut original_files = Vec::new();
        for fp in &params.file_paths {
            let path = ctx.resolve_path(std::path::Path::new(fp));
            let content = if path.exists() {
                tokio::fs::read_to_string(&path).await?
            } else {
                String::new()
            };
            original_files.push(OriginalFileEntry {
                file_path: fp.clone(),
                content,
            });
        }

        // Build the runner and run.
        let runner = BestOfNRunner::new(config.clone());
        let result = runner
            .run(
                params.prompt,
                original_files,
                self.provider.clone(),
                self.registry.clone(),
                ctx.session_id.clone(),
            )
            .await?;

        // Decide whether to show or auto-apply based on mode / override.
        let show_mode = params
            .show_mode
            .unwrap_or(matches!(mode, jcode_best_of_n::BestOfNMode::Show));

        let file_paths = params.file_paths.clone();

        if show_mode {
            let output = format_show_result(&result);
            // Write a side panel page so the user can scroll through candidates.
            let page_id = format!("best-of-n-{}", result.run_id);
            let sidepanel_result = crate::side_panel::write_markdown_page(
                &ctx.session_id,
                &page_id,
                Some("Best-of-N Candidates"),
                &output.output,
                true,
            );
            if let Ok(snapshot) = sidepanel_result {
                Bus::global().publish(BusEvent::SidePanelUpdated(SidePanelUpdated {
                    session_id: ctx.session_id.clone(),
                    snapshot,
                }));
            }
            // Clean up per-run state from store and registry after rendering.
            runner.cleanup(&result.run_id, &self.registry).await;
            return Ok(output);
        } else {
            // Auto mode: apply the winner.
            runner
                .apply_winner(&result, &file_paths, &self.registry, &ctx)
                .await
        }
    }
}

/// Input parameters for the `best_of_n` tool.
#[derive(Deserialize)]
struct BestOfNInput {
    prompt: String,
    file_paths: Vec<String>,
    #[serde(default)]
    show_mode: Option<bool>,
}

/// Runner that drives a single best-of-N cycle end-to-end.
///
/// The runner is cheap to construct and is intended to be created by the
/// `best_of_n` tool entry point, used to spawn candidates, and dropped
/// after the result is applied. The underlying `BestOfNOrchestrator` is
/// shared via the registry (and the static handle) so propose tools can
/// see it without it being threaded through every layer.
pub struct BestOfNRunner {
    pub orchestrator: jcode_best_of_n::BestOfNOrchestrator,
}

impl BestOfNRunner {
    /// Create a runner that owns its own orchestrator + store.
    pub fn new(config: jcode_best_of_n::BestOfNConfig) -> Self {
        Self {
            orchestrator: jcode_best_of_n::BestOfNOrchestrator::new(config),
        }
    }

    /// Use an externally provided orchestrator (lets the caller share
    /// a store with other code paths if needed).
    pub fn with_orchestrator(orchestrator: jcode_best_of_n::BestOfNOrchestrator) -> Self {
        Self { orchestrator }
    }

    /// Run the full best-of-N cycle: spawn N parallel subagents, collect
    /// their proposals, run the selector, return the result.
    ///
    /// `parent_session_id` is used as the parent for the candidate
    /// sub-sessions so they show up under the parent in the session
    /// hierarchy. `original_files` are the on-disk contents the
    /// candidates are expected to modify; the orchestrator uses them to
    /// compute unified diffs after the run.
    pub async fn run(
        &self,
        prompt: String,
        original_files: Vec<OriginalFileEntry>,
        parent_provider: Arc<dyn Provider>,
        parent_registry: Registry,
        parent_session_id: String,
    ) -> Result<BestOfNResult> {
        let strategies = self.orchestrator.generate_strategies();
        let run_id = jcode_best_of_n::RunId::new();

        if strategies.is_empty() {
            anyhow::bail!("BestOfNRunner: no strategies generated from config");
        }

        let store_arc: Arc<jcode_best_of_n::ProposedContentStore> =
            Arc::new(self.orchestrator.store.clone());

        // Install the global handle so propose_* tools can find the
        // store. The handle lives on a StdRwLock and is updated each
        // run, so subsequent runs see fresh context.
        let static_handle = BestOfNOrchestratorHandle {
            run_id: run_id.0.clone(),
            candidate_id: String::new(),
            config: self.orchestrator.config.clone(),
            store: store_arc.clone(),
        };
        set_best_of_n_handle(static_handle);
        // Drop guard auto-clears the static handle even on panic,
        // preventing a stale handle from leaking across turns.
        let _handle_guard = StaticHandleGuard;

        // Publish onto the shared registry so downstream clones
        // (subagent's registry) see the per-run handle. The candidate
        // subagents will overwrite this with their own per-candidate
        // handle; we keep the run-level entry here for the parent.
        {
            let mut guard = parent_registry.best_of_n.write().await;
            *guard = Some(BestOfNOrchestratorHandle {
                run_id: run_id.0.clone(),
                candidate_id: String::new(),
                config: self.orchestrator.config.clone(),
                store: store_arc.clone(),
            });
        }

        let candidate_diffs = match self
            .run_candidates(
                &run_id,
                &strategies,
                prompt,
                parent_provider,
                parent_registry.clone(),
                parent_session_id,
                store_arc.clone(),
                &original_files,
            )
            .await
        {
            Ok(diffs) => diffs,
            Err(err) => {
                self.cleanup(&run_id, &parent_registry).await;
                return Err(err);
            }
        };

        let selection = self.orchestrator.select_winner(&candidate_diffs);
        let result = self
            .orchestrator
            .build_result(run_id.clone(), candidate_diffs, &selection);

        Ok(result)
    }

    /// Spawn the candidate subagents in parallel and collect their diffs.
    #[allow(clippy::too_many_arguments)]
    async fn run_candidates(
        &self,
        run_id: &jcode_best_of_n::RunId,
        strategies: &[jcode_best_of_n::CandidateStrategy],
        prompt: String,
        parent_provider: Arc<dyn Provider>,
        parent_registry: Registry,
        parent_session_id: String,
        store: Arc<jcode_best_of_n::ProposedContentStore>,
        original_files: &[jcode_best_of_n::store::OriginalFileEntry],
    ) -> Result<Vec<CandidateDiff>> {
        let mut tasks = Vec::with_capacity(strategies.len());
        // Hoist: build the allowed-tool set once, clone per candidate.
        let allowed_tool_set = build_allowed_tool_set(&parent_registry).await;

        for (index, strategy) in strategies.iter().enumerate() {
            let candidate_id = jcode_best_of_n::CandidateId::new(index);
            let candidate_id_str = candidate_id.0.clone();
            let run_id_str = run_id.0.clone();
            let strategy = strategy.clone();
            let prompt = prompt.clone();
            let provider = parent_provider.fork();
            let registry = parent_registry.clone();
            let parent_session_id = parent_session_id.clone();
            let config = self.orchestrator.config.clone();
            let store_for_task = store.clone();

            if let Err(e) = provider.set_temperature(strategy.temperature as f32) {
                crate::logging::warn(&format!(
                    "[best_of_n] set_temperature(t={:.2}) failed for candidate {}: {e}",
                    strategy.temperature, index
                ));
            }
            #[allow(clippy::collapsible_if)]
            if let Some(ref model) = strategy.model {
                if let Err(e) = provider.set_model(model) {
                    crate::logging::warn(&format!(
                        "[best_of_n] set_model({}) failed for candidate {}: {e}",
                        model, index
                    ));
                }
            }

            let allowed = allowed_tool_set.clone();

            let session_title = match strategy.model {
                Some(ref m) => format!(
                    "best-of-N candidate {} (t={:.2}, model={})",
                    index, strategy.temperature, m
                ),
                None => format!(
                    "best-of-N candidate {} (t={:.2})",
                    index, strategy.temperature
                ),
            };
            let mut session = Session::create(Some(parent_session_id.clone()), Some(session_title));
            session.model = Some(provider.model());
            if let Err(err) = session.save() {
                crate::logging::warn(&format!(
                    "[best_of_n] failed to save candidate session: {err}"
                ));
            }

            let mut agent =
                Agent::new_with_session(provider.clone(), registry.clone(), session, Some(allowed));
            agent.set_best_of_n_context(run_id_str.clone(), candidate_id_str.clone());

            {
                let handle = BestOfNOrchestratorHandle {
                    run_id: run_id_str.clone(),
                    candidate_id: candidate_id_str.clone(),
                    config: config.clone(),
                    store: store_for_task.clone(),
                };
                let mut guard = registry.best_of_n.write().await;
                *guard = Some(handle);
            }

            tasks.push(tokio::spawn(async move {
                let result = agent.run_once_capture(&prompt).await;
                (candidate_id, strategy, result, store_for_task, run_id_str)
            }));
        }

        // Wait for all candidates and build their diffs in order.
        let mut diffs = Vec::with_capacity(tasks.len());
        for handle in tasks {
            let (candidate_id, strategy, outcome, candidate_store, _run_id_str) = handle
                .await
                .map_err(|e| anyhow::anyhow!("candidate task join error: {e}"))?;
            let diff = match outcome {
                Ok(_text) => {
                    let file_diffs =
                        candidate_store.build_diffs(run_id, &candidate_id.0, original_files);
                    if file_diffs.is_empty() {
                        CandidateDiff {
                            candidate_id,
                            strategy,
                            status: CandidateStatus::NoChanges,
                            file_diffs,
                            total_tokens: None,
                            error: None,
                        }
                    } else {
                        CandidateDiff {
                            candidate_id,
                            strategy,
                            status: CandidateStatus::Success,
                            file_diffs,
                            total_tokens: None,
                            error: None,
                        }
                    }
                }
                Err(err) => CandidateDiff {
                    candidate_id,
                    strategy,
                    status: CandidateStatus::Failed,
                    file_diffs: Vec::new(),
                    total_tokens: None,
                    error: Some(err.to_string()),
                },
            };
            diffs.push(diff);
        }

        Ok(diffs)
    }

    /// Clear the per-run state from the store and the registry.
    /// The static best-of-n handle is cleared by `StaticHandleGuard`'s Drop
    /// when `run()` returns, so we don't duplicate it here.
    async fn cleanup(&self, run_id: &jcode_best_of_n::RunId, registry: &Registry) {
        self.orchestrator.store.clear_run(run_id);
        let mut guard = registry.best_of_n.write().await;
        *guard = None;
    }

    /// Apply the winner's diffs to real files on disk.
    ///
    /// `ctx` is used to resolve relative file paths against the same
    /// working directory the candidate saw. The winner's diffs are
    /// written in order; a failure on one file leaves previously
    /// written files applied and returns the error.
    ///
    /// `allowed_paths` restricts which files may be written: any diff
    /// whose `file_path` is not in this set is skipped with a warning.
    /// This prevents a misbehaving candidate from editing files outside
    /// the scope the user (or parent agent) explicitly requested.
    pub async fn apply_winner(
        &self,
        result: &BestOfNResult,
        allowed_paths: &[String],
        registry: &Registry,
        ctx: &ToolContext,
    ) -> Result<ToolOutput> {
        let Some(winner) = &result.winner else {
            anyhow::bail!("BestOfNRunner::apply_winner: no winner in result");
        };

        if winner.file_diffs.is_empty() {
            return Ok(ToolOutput::new(
                "No file changes to apply — winner produced no diffs.",
            ));
        }

        let mut summary_lines: Vec<String> = Vec::with_capacity(winner.file_diffs.len() + 2);
        summary_lines.push(format!(
            "Applying winner '{}' (strategy: {}):",
            winner.candidate_id, winner.strategy.label
        ));

        for diff in &winner.file_diffs {
            // allowlist: skip any path not in the original file_paths.
            // Paths are resolved and canonicalized before comparison to handle
            // case-insensitive filesystems and path-format differences (./ vs
            // absolute, symlinks, case variation on macOS).
            let is_allowed = allowed_paths.iter().any(|p| {
                let allowed = ctx.resolve_path(std::path::Path::new(p));
                let candidate = ctx.resolve_path(std::path::Path::new(&diff.file_path));
                paths_match(&allowed, &candidate)
            });
            if !is_allowed {
                let skip_msg = format!("  ! skip '{}' (not in allowed paths)", diff.file_path);
                crate::logging::warn(&format!(
                    "[best_of_n] apply_winner skipping out-of-scope path: {}",
                    diff.file_path
                ));
                summary_lines.push(skip_msg);
                continue;
            }

            let path = ctx.resolve_path(std::path::Path::new(&diff.file_path));
            let path_display = path.display().to_string();
            let path_buf = path.to_path_buf();

            if diff.is_new_file {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .with_context(|| format!("create_dir_all {}", parent.display()))?;
                }
                tokio::fs::write(&path, diff.new_content.as_bytes())
                    .await
                    .with_context(|| format!("write new file {}", path_display))?;
                summary_lines.push(format!(
                    "  + create {} ({} bytes)",
                    path_display,
                    diff.new_content.len()
                ));
            } else {
                tokio::fs::write(&path, diff.new_content.as_bytes())
                    .await
                    .with_context(|| format!("write {}", path_display))?;
                summary_lines.push(format!(
                    "  ~ update {} ({} bytes)",
                    path_display,
                    diff.new_content.len()
                ));
            }

            // Bug #1: Publish FileTouch bus event for swarm coordination.
            let op = if diff.is_new_file {
                FileOp::Write
            } else {
                FileOp::Edit
            };
            let detail = build_file_touch_preview(&diff.unified_diff);
            Bus::global().publish(BusEvent::FileTouch(FileTouch {
                session_id: ctx.session_id.clone(),
                path: path_buf,
                op,
                intent: None,
                summary: Some(format!(
                    "best-of-n winner candidate '{}'",
                    winner.candidate_id
                )),
                detail,
            }));

            // Bug #1: Fire FileChanged hook (fire-and-forget, observational).
            let session_id = ctx.session_id.clone();
            let cwd = ctx
                .working_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let file_path_str = path_display.clone();
            let hook_diff = diff.unified_diff.clone();
            let hook_change_type = if diff.is_new_file {
                "created"
            } else {
                "modified"
            };
            tokio::spawn(async move {
                let hook_config = load_hooks_config();
                let hook_registry = HookRegistry::from_config(hook_config.clone());
                let dispatch_config = DispatchConfig::from_settings(&hook_config.settings);
                let mut hook_ctx = HookContext::new(&session_id, "", &cwd, "FileChanged");
                hook_ctx.file_path = Some(file_path_str.clone());
                let handlers = hook_registry.get_matching(&HookEvent::FileChanged, &hook_ctx);
                if !handlers.is_empty() {
                    let mut hook_input = HookInputBuilder::new()
                        .session(&session_id, &cwd)
                        .event("FileChanged")
                        .build();
                    hook_input.file_path = Some(file_path_str);
                    hook_input.change_type = Some(hook_change_type.to_string());
                    hook_input.diff = Some(hook_diff);
                    let _ = jcode_hooks::dispatch_hooks(
                        &HookEvent::FileChanged,
                        &hook_input,
                        &handlers,
                        &dispatch_config,
                    )
                    .await;
                }
            });
        }

        // Drop the run from the store once the winner is applied.
        self.orchestrator.store.clear_run(&result.run_id);
        let mut guard = registry.best_of_n.write().await;
        *guard = None;

        Ok(ToolOutput::new(summary_lines.join("\n"))
            .with_title(format!("Applied winner {}", winner.candidate_id)))
    }
}

/// Build the allowed-tool set for a candidate subagent.
///
/// Starts from the registry's full tool name list, removes tools in
/// `FORBIDDEN_TOOLS` and `AMBIENT_TOOLS`, and ensures the propose tools
/// are present.
pub(crate) async fn build_allowed_tool_set(registry: &Registry) -> HashSet<String> {
    let mut allowed: HashSet<String> = registry.tool_names().await.into_iter().collect();
    for blocked in FORBIDDEN_TOOLS {
        allowed.remove(*blocked);
    }
    for ambient in AMBIENT_TOOLS {
        allowed.remove(*ambient);
    }
    for required in ["propose_edit", "propose_write", "propose_hashline"] {
        allowed.insert(required.to_string());
    }
    allowed
}

/// Format a `BestOfNResult` as a human-readable markdown display for
/// Show mode. Shows each candidate with strategy, status, model, and
/// unified diff, followed by the automatic selection recommendation.
const FILE_TOUCH_PREVIEW_MAX_LINES: usize = 6;
const FILE_TOUCH_PREVIEW_MAX_BYTES: usize = 240;

/// Build a compact preview string from a unified diff for bus events.
/// Mirrors the same helper in edit.rs / write.rs / apply_patch.rs.
pub(crate) fn build_file_touch_preview(diff: &str) -> Option<String> {
    let trimmed = diff.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut lines = trimmed.lines();
    let mut preview = lines
        .by_ref()
        .take(FILE_TOUCH_PREVIEW_MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let mut truncated = lines.next().is_some();

    if preview.len() > FILE_TOUCH_PREVIEW_MAX_BYTES {
        preview = crate::util::truncate_str(&preview, FILE_TOUCH_PREVIEW_MAX_BYTES)
            .trim_end()
            .to_string();
        truncated = true;
    }

    if truncated {
        preview.push_str("\n…");
    }

    Some(preview)
}

pub(crate) fn format_show_result(result: &BestOfNResult) -> ToolOutput {
    use std::fmt::Write;
    let mut md = String::new();

    let _ = writeln!(md, "# Best-of-N Candidates\n");
    let _ = writeln!(
        md,
        "**Run**: `{}` | **Candidates**: {} | **Winner**: Candidate #{} ({})\n",
        result.run_id,
        result.candidates.len(),
        result.winner_index.map_or(-1, |i| i as i32),
        result.selection_reason.as_deref().unwrap_or("no selection"),
    );

    for (i, candidate) in result.candidates.iter().enumerate() {
        let is_winner = result.winner_index == Some(i);
        let marker = if is_winner { "★ " } else { "  " };
        let _ = writeln!(md, "---\n");
        let _ = writeln!(
            md,
            "{}## Candidate {} — {}",
            marker, i, candidate.strategy.label
        );

        let model_str = match &candidate.strategy.model {
            Some(m) => format!("model={}", m),
            None => String::new(),
        };
        let _ = writeln!(
            md,
            "- **Status**: `{:?}` | **T={:.2}** {}",
            candidate.status, candidate.strategy.temperature, model_str,
        );

        if let Some(ref err) = candidate.error {
            let _ = writeln!(md, "- **Error**: `{}`", err);
        }

        let _ = writeln!(
            md,
            "- **Files**: {} changed, {} ops total, {} tokens",
            candidate.changed_file_count(),
            candidate.total_ops(),
            candidate
                .total_tokens
                .map_or("?".to_string(), |t| t.to_string()),
        );

        if is_winner && !candidate.file_diffs.is_empty() {
            let _ = writeln!(md, "\n### Applied changes (winner)\n");
            for diff in &candidate.file_diffs {
                let action = if diff.is_new_file { "CREATE" } else { "EDIT" };
                let _ = writeln!(md, "**{}** `{}`", action, diff.file_path);
                if !diff.unified_diff.is_empty() {
                    let _ = writeln!(md, "```diff\n{}\n```", diff.unified_diff);
                }
            }
        } else if !candidate.file_diffs.is_empty() {
            let _ = writeln!(
                md,
                "\n<details>\n<summary>Diffs ({}, {} files)</summary>\n\n",
                candidate.strategy.label,
                candidate.changed_file_count()
            );
            for diff in &candidate.file_diffs {
                let action = if diff.is_new_file { "CREATE" } else { "EDIT" };
                let _ = writeln!(md, "**{}** `{}`\n", action, diff.file_path);
                if !diff.unified_diff.is_empty() {
                    let _ = writeln!(md, "```diff\n{}\n```\n", diff.unified_diff);
                }
            }
            let _ = writeln!(md, "</details>\n");
        }
    }

    let _ = writeln!(md, "---\n");
    let _ = writeln!(
        md,
        "**Recommendation**: {}",
        result.selection_reason.as_deref().unwrap_or("no winner")
    );

    ToolOutput::new(md).with_title("Best-of-N candidates (show mode)")
}

/// Drop guard that clears the global BEST_OF_N_HANDLE on scope exit,
/// even during a panic. Prevents stale handle leakage across turns.
struct StaticHandleGuard;

impl Drop for StaticHandleGuard {
    fn drop(&mut self) {
        clear_best_of_n_handle();
    }
}

/// Compare two paths for equality, using canonicalization to handle
/// case-insensitive filesystems, symlinks, and path-format differences.
fn paths_match(a: &std::path::Path, b: &std::path::Path) -> bool {
    let a_canon = std::fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let b_canon = std::fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    a_canon == b_canon
}
