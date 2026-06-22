use super::*;

impl App {
    /// Build split system prompt for better caching
    pub(super) fn build_system_prompt_split(
        &mut self,
        memory_prompt: Option<&str>,
    ) -> crate::prompt::SplitSystemPrompt {
        // Ambient mode: use the full override prompt directly
        if let Some(ref prompt) = self.ambient_system_prompt {
            return crate::prompt::SplitSystemPrompt {
                static_part: prompt.clone(),
                dynamic_part: String::new(),
            };
        }

        let skills = self.current_skills_snapshot();
        let skill_prompt = self
            .active_skill
            .as_ref()
            .and_then(|name| skills.get(name).map(|s| s.get_prompt().to_string()));
        let available_skills: Vec<crate::prompt::SkillInfo> = skills
            .list()
            .iter()
            .map(|s| crate::prompt::SkillInfo {
                name: s.name.clone(),
                description: s.description.clone(),
            })
            .collect();
        // Run the same keyword pipeline as the agent runtime so TUI users see
        // workflow prompts and have their mode state persisted. (See issue #391.)
        let keyword_prompt = {
            let latest_input = self
                .session
                .messages
                .iter()
                .rev()
                .find(|m| {
                    use crate::message::Role;
                    matches!(m.role, Role::User)
                })
                .and_then(|m| {
                    m.content.iter().find_map(|b| match b {
                        crate::message::ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                })
                .unwrap_or("");
            let last_assistant = self
                .session
                .messages
                .iter()
                .rev()
                .find(|m| {
                    use crate::message::Role;
                    matches!(m.role, Role::Assistant)
                })
                .and_then(|m| {
                    m.content.iter().find_map(|b| match b {
                        crate::message::ContentBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                });
            let result = jcode_keywords::process_turn(
                latest_input,
                last_assistant,
                self.session
                    .working_dir
                    .as_deref()
                    .map(std::path::Path::new),
                &self.session.id,
            );
            for conflict in &result.conflicts {
                crate::logging::warn(&jcode_keywords::conflict::format_warning(conflict));
            }

            // Show status notice for activated modes (so user sees something happen)
            if result.keyword_prompt.is_some() {
                let mode_state = jcode_keywords::state::load_state(
                    self.session
                        .working_dir
                        .as_deref()
                        .map(std::path::Path::new),
                );
                if let Some(active) = mode_state.active_modes.first() {
                    self.set_status_notice(format!("🧠 {} mode activated", active.workflow));
                }
            }

            // Dispatch deferred spawns as status messages
            // The keyword prompt (injected into system prompt) already tells
            // the model to spawn subagents. These deferred spawns are
            // informational — the model reads the prompt and uses the
            // subagent tool directly.
            for spawn in &result.deferred_spawns {
                let msg = format!(
                    "🧩 Keyword: {} requested a subagent spawn (deferred)",
                    spawn.kind
                );
                crate::logging::info(&msg);
            }

            result.keyword_prompt
        };

        // Inject priority-tier notes into the system prompt so they survive compaction.
        let working_dir = self
            .session
            .working_dir
            .as_deref()
            .map(std::path::Path::new);
        let notepad_prompt =
            crate::notepad::Notepad::new(working_dir, &crate::config::config().notepad)
                .and_then(|n| n.priority_prompt_block());

        let (mut split, context_info) = crate::prompt::build_system_prompt_split(
            skill_prompt.as_deref(),
            &available_skills,
            self.session.is_canary,
            memory_prompt,
            working_dir,
            keyword_prompt,
            notepad_prompt.as_deref(),
        );
        self.append_current_turn_system_reminder(&mut split);
        self.context_info = context_info;
        split
    }

    pub(in crate::tui::app) fn show_injected_memory_context(
        &mut self,
        prompt: &str,
        display_prompt: Option<&str>,
        count: usize,
        age_ms: u64,
        memory_ids: Vec<String>,
    ) {
        let count = count.max(1);
        let plural = if count == 1 { "memory" } else { "memories" };
        let display_prompt = if let Some(display_prompt) = display_prompt {
            display_prompt.to_string()
        } else if prompt.trim().is_empty() {
            "# Memory\n\n## Notes\n1. (empty injection payload)".to_string()
        } else {
            prompt.to_string()
        };
        if !self.should_inject_memory_context(prompt) {
            return;
        }
        crate::memory::record_injected_prompt(prompt, count, age_ms);
        let summary = if count == 1 {
            "🧠 auto-recalled 1 memory".to_string()
        } else {
            format!("🧠 auto-recalled {} memories", count)
        };
        // Record to session for replay visualization
        self.session.record_memory_injection(
            summary.clone(),
            display_prompt.clone(),
            count as u32,
            age_ms,
            memory_ids,
        );
        if let Err(err) = self.session.save() {
            crate::logging::warn(&format!(
                "Failed to persist memory injection for session {}: {}",
                self.session.id, err
            ));
        }
        self.push_display_message(DisplayMessage::memory(summary, display_prompt));
        let notice = if let Some(experimental_notice) =
            self.note_experimental_feature_use("memory_injection")
        {
            format!(
                "🧠 {} {} injected · ⚠ {}",
                count, plural, experimental_notice
            )
        } else {
            format!("🧠 {} {} injected", count, plural)
        };
        self.set_status_notice(notice);
    }

    /// Get memory prompt using async non-blocking approach
    /// Takes any pending memory from background check and sends context to memory agent for next turn
    pub(in crate::tui::app) fn build_memory_prompt_nonblocking(
        &self,
        messages: &[Message],
    ) -> Option<crate::memory::PendingMemory> {
        if self.is_remote || !self.memory_enabled {
            return None;
        }

        // Take pending memory if available (computed in background during last turn)
        let pending = if crate::message::ends_with_fresh_user_turn(messages) {
            crate::memory::take_pending_memory(&self.session.id)
        } else {
            None
        };

        // Send context to memory agent for the NEXT turn (doesn't block current send)
        let shared_messages: std::sync::Arc<[crate::message::Message]> = messages.to_vec().into();
        crate::memory_agent::update_context_sync_with_dir(
            &self.session.id,
            shared_messages,
            self.session.working_dir.clone(),
        );

        // Return pending memory from previous turn
        pending
    }

    /// Extract and store memories from the session transcript at end of session
    pub(super) async fn extract_session_memories(&self) {
        // Skip if remote mode or not enough messages
        let provider_messages = self.materialized_provider_messages();
        if self.is_remote || !self.memory_enabled || provider_messages.len() < 4 {
            return;
        }

        crate::logging::info(&format!(
            "Extracting memories from {} messages",
            provider_messages.len()
        ));

        // Build transcript from messages
        let mut transcript = String::new();
        for msg in &provider_messages {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
            };
            transcript.push_str(&format!("**{}:**\n", role));
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text, .. } => {
                        transcript.push_str(text);
                        transcript.push('\n');
                    }
                    ContentBlock::ToolUse { name, .. } => {
                        transcript.push_str(&format!("[Used tool: {}]\n", name));
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        // Truncate long results
                        let preview = if content.len() > 200 {
                            format!("{}...", crate::util::truncate_str(content, 200))
                        } else {
                            content.clone()
                        };
                        transcript.push_str(&format!("[Result: {}]\n", preview));
                    }
                    ContentBlock::Reasoning { .. }
                    | ContentBlock::ReasoningTrace { .. }
                    | ContentBlock::AnthropicThinking { .. }
                    | ContentBlock::OpenAIReasoning { .. } => {}
                    ContentBlock::Image { .. } => {
                        transcript.push_str("[Image]\n");
                    }
                    ContentBlock::OpenAICompaction { .. } => {
                        transcript.push_str("[OpenAI native compaction]\n");
                    }
                }
            }
            transcript.push('\n');
        }

        if !crate::memory::memory_llm_judge_available() {
            crate::logging::info("Memory extraction skipped: LLM judge unavailable");
            return;
        }

        // Extract memories using sidecar (with existing context for dedup)
        let manager = self
            .session
            .working_dir
            .as_deref()
            .map(|dir| {
                crate::memory::MemoryManager::new()
                    .with_project_dir(dir)
                    .with_skills(self.active_skill.is_none())
            })
            .unwrap_or_else(|| {
                crate::memory::MemoryManager::new().with_skills(self.active_skill.is_none())
            });
        let existing: Vec<String> = manager
            .list_all()
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.active)
            .map(|e| e.content)
            .collect();
        let sidecar = crate::sidecar::Sidecar::new();
        match sidecar
            .extract_memories_with_existing(&transcript, &existing)
            .await
        {
            Ok(extracted) if !extracted.is_empty() => {
                let manager = self
                    .session
                    .working_dir
                    .as_deref()
                    .map(|dir| crate::memory::MemoryManager::new().with_project_dir(dir))
                    .unwrap_or_default();
                let mut stored_count = 0;

                for memory in extracted {
                    let category = crate::memory::MemoryCategory::from_extracted(&memory.category);

                    // Map trust string to enum
                    let trust = match memory.trust.as_str() {
                        "high" => crate::memory::TrustLevel::High,
                        "low" => crate::memory::TrustLevel::Low,
                        _ => crate::memory::TrustLevel::Medium,
                    };

                    // Create memory entry
                    let entry = crate::memory::MemoryEntry::new(category, memory.content)
                        .with_id(format!("auto_{}", chrono::Utc::now().timestamp_millis()))
                        .with_source(self.session.id.clone())
                        .with_trust(trust);

                    // Store memory
                    if manager.remember_project(entry).is_ok() {
                        stored_count += 1;
                    }
                }

                if stored_count > 0 {
                    crate::logging::info(&format!(
                        "Extracted {} memories from session",
                        stored_count
                    ));
                }
            }
            Ok(_) => {
                // No memories extracted, that's fine
            }
            Err(e) => {
                crate::logging::info(&format!("Memory extraction skipped: {}", e));
            }
        }
    }
}
