use super::Agent;
use crate::logging;
use crate::message::{Message, ToolDefinition};

impl Agent {
    pub(super) fn log_prompt_prefix_accounting(
        &self,
        split: &crate::prompt::SplitSystemPrompt,
        tools: &[ToolDefinition],
    ) {
        let system_tokens = split.estimated_tokens();
        let tool_tokens = ToolDefinition::aggregate_prompt_token_estimate(tools);
        let prefix_tokens = system_tokens + tool_tokens;
        logging::info(&format!(
            "Prompt prefix estimate: total={} tokens (system={} tools={})",
            prefix_tokens, system_tokens, tool_tokens
        ));
    }

    pub(super) fn build_memory_prompt_nonblocking_shared(
        &self,
        messages: std::sync::Arc<[Message]>,
        _memory_event_tx: Option<crate::memory::MemoryEventSink>,
    ) -> Option<crate::memory::PendingMemory> {
        if !self.memory_enabled {
            return None;
        }

        let session_id = &self.session.id;

        let pending = if crate::message::ends_with_fresh_user_turn(&messages) {
            crate::memory::take_pending_memory(session_id)
        } else {
            None
        };

        // Issue #358: when mempalace backend is configured, bypass the
        // native MemoryAgent and run the mempalace per-turn pipeline instead.
        #[cfg(feature = "mempalace-backend")]
        {
            if is_mempalace_backend() {
                let sid = session_id.to_string();
                let working_dir = self.session.working_dir.clone();
                tokio::spawn(async move {
                    mempalace_per_turn_pipeline(&sid, messages, working_dir).await;
                });
                return pending;
            }
        }

        // Use the persistent memory-agent pipeline as the single source of truth.
        // Running both this and the legacy MemoryManager background retrieval path
        // can prepare overlapping pending prompts for the same turn, which makes
        // memory injection feel overly aggressive.
        crate::memory_agent::update_context_sync_with_dir(
            session_id,
            messages,
            self.session.working_dir.clone(),
        );

        pending
    }

    fn append_current_turn_system_reminder(&self, split: &mut crate::prompt::SplitSystemPrompt) {
        let Some(reminder) = self
            .current_turn_system_reminder
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        else {
            return;
        };

        if !split.dynamic_part.is_empty() {
            split.dynamic_part.push_str("\n\n");
        }
        split.dynamic_part.push_str("# System Reminder\n\n");
        split.dynamic_part.push_str(reminder);
    }

    /// Build split system prompt for better caching
    /// Returns static (cacheable) and dynamic (not cached) parts separately
    pub(super) fn build_system_prompt_split(
        &self,
        memory_prompt: Option<&str>,
    ) -> crate::prompt::SplitSystemPrompt {
        if let Some(ref override_prompt) = self.system_prompt_override {
            return crate::prompt::SplitSystemPrompt {
                static_part: override_prompt.clone(),
                dynamic_part: String::new(),
            };
        }

        let skills = self.current_skills_snapshot();
        let skill_prompt = self
            .active_skill
            .as_ref()
            .and_then(|name| skills.get(name).map(|skill| skill.get_prompt().to_string()));

        let available_skills: Vec<crate::prompt::SkillInfo> = self
            .current_skills_snapshot()
            .list()
            .iter()
            .map(|skill| crate::prompt::SkillInfo {
                name: skill.name.clone(),
                description: skill.description.clone(),
            })
            .collect();

        let working_dir = self
            .session
            .working_dir
            .as_ref()
            .map(std::path::PathBuf::from);

        // Detect keywords, update mode state, execute workflows, build prompt.
        // We skip the whole pipeline on empty input so an empty/system-only turn
        // does not burn a turn of any active mode's budget.
        let keyword_prompt = {
            let latest_input = self
                .session
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, crate::message::Role::User))
                .and_then(|m| {
                    m.content.iter().find_map(|b| match b {
                        crate::message::ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                })
                .unwrap_or("");
            if latest_input.is_empty() {
                None
            } else {
                let detections = jcode_keywords::detect_keywords(latest_input);
                let mut mode_state =
                    jcode_keywords::state::update_modes(&detections, working_dir.as_deref());

                // Surface any mode conflicts (TDD + ultrawork, etc.) to logs.
                let active_kinds: Vec<jcode_keywords::registry::WorkflowKind> =
                    mode_state.active_modes.iter().map(|m| m.workflow).collect();
                for conflict in jcode_keywords::conflict::check_conflicts(&active_kinds) {
                    crate::logging::warn(&jcode_keywords::conflict::format_warning(&conflict));
                }

                // Process PREVIOUS turn's LLM response (phase transitions, completion)
                // This runs at the START of the current turn, using last turn's response
                if let Some(last_assistant) = self
                    .session
                    .messages
                    .iter()
                    .rev()
                    .find(|m| matches!(m.role, crate::message::Role::Assistant))
                    .and_then(|m| {
                        m.content.iter().find_map(|b| match b {
                            crate::message::ContentBlock::Text { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                    })
                {
                    let response_actions =
                        jcode_keywords::process_turn_response(&mode_state, last_assistant);
                    if !response_actions.is_empty() {
                        let _ = jcode_keywords::apply_actions(&mut mode_state, &response_actions);
                    }
                }

                // Classify task size so heavy workflows can suppress themselves for
                // trivial requests (e.g. a one-line "$ultrawork fix typo").
                let task_size = jcode_keywords::task_size::classify(latest_input);

                // Execute active workflows for THIS turn
                let actions = jcode_keywords::execute_active_workflows(
                    &mode_state,
                    latest_input,
                    working_dir.as_deref(),
                    &self.session.id,
                    task_size,
                );
                if !actions.is_empty() {
                    let (summaries, deferred) =
                        jcode_keywords::apply_actions(&mut mode_state, &actions);
                    for s in &summaries {
                        crate::logging::info(&format!("Keyword workflow: {}", s));
                    }
                    if !deferred.is_empty() {
                        crate::logging::warn(&format!(
                            "Keyword workflow: {} spawn action(s) deferred — they will not run until SubagentTool is wired from the agent runtime. (See issue #391 follow-up.)",
                            deferred.len()
                        ));
                    }
                }

                // Persist metadata to disk
                jcode_keywords::state::save_state(&mode_state, working_dir.as_deref());

                // Build workflow prompt
                let prompt = jcode_keywords::build_workflow_prompt(&mode_state);
                if prompt.is_empty() {
                    None
                } else {
                    Some(prompt)
                }
            }
        };

        let (mut split, _context_info) = crate::prompt::build_system_prompt_split(
            skill_prompt.as_deref(),
            &available_skills,
            self.session.is_canary,
            memory_prompt,
            working_dir.as_deref(),
            keyword_prompt,
        );

        self.append_current_turn_system_reminder(&mut split);

        split
    }

    /// Non-blocking memory prompt - takes pending result and spawns check for next turn
    #[allow(dead_code)]
    pub(super) fn build_memory_prompt_nonblocking(
        &self,
        messages: &[Message],
        _memory_event_tx: Option<crate::memory::MemoryEventSink>,
    ) -> Option<crate::memory::PendingMemory> {
        self.build_memory_prompt_nonblocking_shared(messages.to_vec().into(), _memory_event_tx)
    }
}

/// Wrap a step prompt body in `<system_reminder>...</system_reminder>` tags.
///
/// Step prompts are emitted by the harness (not typed by the user), but they
/// arrive in the conversation transcript at the same position a user message
/// would. Without disambiguation, the LLM tends to treat them as a fresh user
/// turn — re-greeting, re-asking, or otherwise breaking flow.
///
/// Wrapping the body in `<system_reminder>` tags signals "this is harness
/// scaffolding, not the user speaking" and lets the model continue its
/// existing turn cleanly. Returns an empty string when `prompt` is empty so
/// callers don't end up emitting an empty tag pair.
///
/// This helper is intentionally not yet wired into step-prompt emission;
/// integration will land alongside the Phase 1 `AgentDefinition.step_prompt`
/// changes.
#[allow(dead_code)]
pub fn wrap_as_system_reminder(prompt: &str) -> String {
    if prompt.is_empty() {
        String::new()
    } else {
        format!("<system_reminder>{}</system_reminder>", prompt)
    }
}

#[cfg(test)]
mod wrap_as_system_reminder_tests {
    use super::wrap_as_system_reminder;

    #[test]
    fn wrap_as_system_reminder_empty_input_returns_empty() {
        assert_eq!(wrap_as_system_reminder(""), "");
    }

    #[test]
    fn wrap_as_system_reminder_non_empty_input_wrapped_correctly() {
        let body = "remaining steps: 3";
        assert_eq!(
            wrap_as_system_reminder(body),
            "<system_reminder>remaining steps: 3</system_reminder>"
        );
    }
}

// ---- Issue #358: mempalace per-turn pipeline --------------------------

/// Check if the mempalace backend is configured via environment or config.
#[cfg(feature = "mempalace-backend")]
fn is_mempalace_backend() -> bool {
    // Check env var first (fast path)
    if let Ok(val) = std::env::var("JCODE_MEMORY_BACKEND") {
        if val.eq_ignore_ascii_case("mempalace") {
            return true;
        }
    }
    // TODO: check config file when config loading is wired
    false
}

/// Issue #358: mempalace-native per-turn pipeline.
///
/// When `memory_backend = "mempalace"`, this replaces the MemoryAgent
/// singleton's `process_context()` path. It runs:
///
/// 1. Format context from messages
/// 2. Embed context (via Palace embedder)
/// 3. Search Palace for relevant drawers
/// 4. Optionally verify via sidecar
/// 5. Surface results into PENDING_MEMORY (so `take_pending_memory()` works)
/// 6. Spawn maintenance in background
///
/// The pipeline writes into the same `PENDING_MEMORY` static that the
/// native path uses, so downstream code (TUI, prompting) is unchanged.
#[cfg(feature = "mempalace-backend")]
async fn mempalace_per_turn_pipeline(
    session_id: &str,
    messages: std::sync::Arc<[Message]>,
    working_dir: Option<String>,
) {
    use crate::memory::{self, MemoryState};
    use crate::memory_types::MemoryEventKind;

    // Format context from messages
    let context = memory::format_context_for_relevance(&messages);
    if context.is_empty() {
        return;
    }

    // Resolve palace path from working_dir or default
    let palace_path = resolve_palace_path(working_dir.as_deref());
    if palace_path.is_none() {
        logging::info(&format!(
            "[{}] mempalace pipeline: no palace path found, skipping",
            session_id
        ));
        return;
    }
    let palace_path = palace_path.unwrap();

    // Open adapter (in a real implementation this would be cached/pooled)
    let adapter = match jcode_mempalace_adapter::MempalaceAdapter::open(&palace_path).await {
        Ok(a) => a,
        Err(e) => {
            logging::info(&format!(
                "[{}] mempalace pipeline: failed to open palace: {}",
                session_id, e
            ));
            return;
        }
    };

    let palace = adapter.palace();
    use jcode_mempalace_adapter::MemoryProvider;

    // Step 1: Embed context
    memory::set_state(MemoryState::Embedding);
    memory::add_event(MemoryEventKind::EmbeddingStarted);

    let query_vec = match palace.embedder().embed(&context).await {
        Ok(v) => v,
        Err(e) => {
            logging::info(&format!(
                "[{}] mempalace pipeline: embedding failed: {}",
                session_id, e
            ));
            memory::set_state(MemoryState::Idle);
            return;
        }
    };

    // Step 2: Search Palace
    let scope = jcode_mempalace_adapter::SearchScope::new().limit(10);
    let hits = match palace.search_with_embedding(&query_vec, &scope).await {
        Ok(h) => h,
        Err(e) => {
            logging::info(&format!(
                "[{}] mempalace pipeline: search failed: {}",
                session_id, e
            ));
            memory::set_state(MemoryState::Idle);
            return;
        }
    };

    let search_latency = 0u64; // TODO: measure actual latency
    memory::add_event(MemoryEventKind::EmbeddingComplete {
        latency_ms: search_latency,
        hits: hits.len(),
    });

    if hits.is_empty() {
        memory::set_state(MemoryState::Idle);
        return;
    }

    // Step 3: Verify (optional sidecar)
    memory::set_state(MemoryState::SidecarChecking { count: hits.len() });
    memory::add_event(MemoryEventKind::SidecarStarted);

    // For now, take all hits as relevant (sidecar verification can be
    // added when the LLM sidecar feature is fully wired)
    let relevant: Vec<_> = hits.into_iter().take(5).collect();

    memory::add_event(MemoryEventKind::SidecarComplete { latency_ms: 0 });

    // Step 4: Format and surface into PENDING_MEMORY
    if !relevant.is_empty() {
        let count = relevant.len();
        let mut prompt = String::from("Relevant memories:\n");
        let mut ids = Vec::new();
        for hit in &relevant {
            prompt.push_str(&format!("- {}\n", hit.text));
            ids.push(hit.text.clone()); // Use text as surrogate ID
        }

        memory::set_pending_memory_with_ids(session_id, prompt, count, ids);
        memory::set_state(MemoryState::FoundRelevant { count });
    } else {
        memory::set_state(MemoryState::Idle);
    }

    // Step 5: Maintenance (spawned, non-blocking)
    // TODO: wire Palace::spawn_maintenance when available
}

/// Resolve the mempalace path from the working directory.
#[cfg(feature = "mempalace-backend")]
fn resolve_palace_path(working_dir: Option<&str>) -> Option<std::path::PathBuf> {
    // Check environment variable first
    if let Ok(path) = std::env::var("JCODE_MEMPALACE_PATH") {
        let p = std::path::PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }

    // Check working directory for .mempalace
    if let Some(dir) = working_dir {
        let palace = std::path::PathBuf::from(dir).join(".mempalace");
        if palace.exists() {
            return Some(palace);
        }
    }

    // Check global palace location
    if let Some(config_dir) = dirs::config_dir() {
        let global = config_dir.join("mempalace");
        if global.exists() {
            return Some(global);
        }
    }

    None
}
