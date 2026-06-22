use super::*;

impl Agent {
    /// Run a single turn with the given user message
    pub async fn run_once(&mut self, user_message: &str) -> Result<()> {
        self.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: user_message.to_string(),
                cache_control: None,
            }],
        );
        self.session.save()?;
        if trace_enabled() {
            eprintln!("[trace] session_id {}", self.session.id);
        }
        let _ = self.run_turn(true).await?;
        Ok(())
    }

    pub async fn run_once_capture(&mut self, user_message: &str) -> Result<String> {
        self.add_message(
            Role::User,
            vec![ContentBlock::Text {
                text: user_message.to_string(),
                cache_control: None,
            }],
        );
        self.session.save()?;
        if trace_enabled() {
            eprintln!("[trace] session_id {}", self.session.id);
        }
        self.run_turn(false).await
    }

    /// Run one conversation turn with streaming events via mpsc channel (per-client)
    pub async fn run_once_streaming_mpsc(
        &mut self,
        user_message: &str,
        images: Vec<(String, String)>,
        system_reminder: Option<String>,
        event_tx: mpsc::UnboundedSender<ServerEvent>,
    ) -> Result<()> {
        // Inject any pending notifications before the user message
        let alerts = self.take_alerts();
        if !alerts.is_empty() {
            let alert_text = format!(
                "[NOTIFICATION]\nYou received {} notification(s) from other agents working in this codebase:\n\n{}\n\nUse the communicate tool (actions: list, read, message/broadcast, dm, channel, share) to coordinate with other agents.",
                alerts.len(),
                alerts.join("\n\n---\n\n")
            );
            self.add_message(
                Role::User,
                vec![ContentBlock::Text {
                    text: alert_text,
                    cache_control: None,
                }],
            );
        }

        self.current_turn_system_reminder =
            system_reminder.filter(|value| !value.trim().is_empty());

        let mut blocks: Vec<ContentBlock> = images
            .into_iter()
            .map(|(media_type, data)| ContentBlock::Image { media_type, data })
            .collect();
        blocks.push(ContentBlock::Text {
            text: user_message.to_string(),
            cache_control: None,
        });

        if blocks.len() > 1 {
            crate::logging::info(&format!(
                "Agent received message with {} image(s)",
                blocks.len() - 1
            ));
        }

        // UserPromptSubmit hook — BLOCKING: can deny the prompt before it enters the conversation
        {
            let session_id = self.session.id.clone();
            let cwd = self.session.working_dir.clone().unwrap_or_default();
            let hook_ctx = HookContext::new(&session_id, "", &cwd, "UserPromptSubmit");
            let handlers = self
                .hook_registry
                .get_matching(&HookEvent::UserPromptSubmit, &hook_ctx);
            if !handlers.is_empty() {
                let hook_input = HookInputBuilder::new()
                    .session(&session_id, &cwd)
                    .event("UserPromptSubmit")
                    .prompt(user_message)
                    .build();
                let stats = jcode_hooks::dispatch_hooks(
                    &HookEvent::UserPromptSubmit,
                    &hook_input,
                    &handlers,
                    &self.dispatch_config,
                )
                .await;
                if stats.any_denied() {
                    let deny_reason = stats
                        .results
                        .iter()
                        .find(|r| matches!(r.outcome, jcode_hooks::ClassifiedOutcome::Deny { .. }))
                        .map(|r| match &r.outcome {
                            jcode_hooks::ClassifiedOutcome::Deny { reason } => reason.clone(),
                            _ => String::new(),
                        })
                        .unwrap_or_else(|| "blocked by hook".to_string());
                    return Err(anyhow::anyhow!("Prompt blocked by hook: {}", deny_reason));
                }
            }
        }

        // UserPromptExpansion hook (fire-and-forget, diagnostic)
        {
            let session_id = self.session.id.clone();
            let cwd = self.session.working_dir.clone().unwrap_or_default();
            let hook_ctx = HookContext::new(&session_id, "", &cwd, "UserPromptExpansion");
            let handlers = self
                .hook_registry
                .get_matching(&HookEvent::UserPromptExpansion, &hook_ctx);
            if !handlers.is_empty() {
                let hook_input = HookInputBuilder::new()
                    .session(&session_id, &cwd)
                    .event("UserPromptExpansion")
                    .prompt(user_message)
                    .build();
                let event = HookEvent::UserPromptExpansion;
                let handlers: Vec<jcode_hooks::HookHandlerConfig> =
                    handlers.into_iter().cloned().collect();
                let dispatch_config = self.dispatch_config.clone();
                tokio::spawn(async move {
                    let refs: Vec<&jcode_hooks::HookHandlerConfig> = handlers.iter().collect();
                    let _ =
                        jcode_hooks::dispatch_hooks(&event, &hook_input, &refs, &dispatch_config)
                            .await;
                });
            }
        }

        self.add_message(Role::User, blocks);
        crate::telemetry::record_turn();
        self.session.save()?;
        let turn_started_at = Instant::now();
        let start_message_index = self.message_count();
        let result = self.run_turn_streaming_mpsc(event_tx).await;
        self.current_turn_system_reminder = None;
        self.fire_turn_end_hook(&result, turn_started_at, start_message_index);
        result
    }

    /// Fire the `TurnEnd` observer hook with turn outcome metadata.
    /// Fire-and-forget via tokio::spawn — the agent does not wait for hooks.
    fn fire_turn_end_hook(
        &self,
        result: &Result<()>,
        started_at: Instant,
        start_message_index: usize,
    ) {
        let session_id = self.session.id.clone();
        let cwd = self.working_dir().unwrap_or_default().to_string();
        let ctx = HookContext::for_turn_end(session_id.clone(), cwd.clone());
        let handlers: Vec<jcode_hooks::HookHandlerConfig> = self
            .hook_registry
            .get_matching(&HookEvent::TurnEnd, &ctx)
            .into_iter()
            .cloned()
            .collect();
        if handlers.is_empty() {
            return;
        }

        let duration_ms = started_at.elapsed().as_millis() as u64;
        let model = self.provider_model();
        let dispatch_config = self.dispatch_config.clone();

        let mut input = HookInputBuilder::new()
            .session(&session_id, &cwd)
            .event("TurnEnd")
            .duration(duration_ms)
            .build();
        input.model = Some(model);

        // Add last assistant text snippet
        if let Some(text) = self.latest_assistant_text_after(start_message_index) {
            const LAST_TEXT_LIMIT: usize = 4000;
            input.prompt_text = Some(text.chars().take(LAST_TEXT_LIMIT).collect());
        }

        // Add error info on failure
        if let Err(error) = result {
            const ERROR_LIMIT: usize = 1000;
            let message: String = error.to_string().chars().take(ERROR_LIMIT).collect();
            input.stop_reason = Some(message);
        }

        let event = HookEvent::TurnEnd;
        tokio::spawn(async move {
            let refs: Vec<&jcode_hooks::HookHandlerConfig> = handlers.iter().collect();
            jcode_hooks::dispatch_hooks(&event, &input, &refs, &dispatch_config).await;
        });
    }

    /// Clear conversation history
    pub fn clear(&mut self) {
        let preserve_canary = self.session.is_canary;
        let preserve_testing_build = self.session.testing_build.clone();
        let preserve_debug = self.session.is_debug;
        let preserve_working_dir = self.session.working_dir.clone();

        self.session.mark_closed();
        self.persist_session_best_effort("pre-clear session close state");

        let mut new_session = Session::create(None, None);
        new_session.mark_active();
        new_session.model = Some(self.provider.model());
        new_session.provider_key =
            crate::session::derive_session_provider_key(self.provider.name());
        new_session.is_canary = preserve_canary;
        new_session.testing_build = preserve_testing_build;
        new_session.is_debug = preserve_debug;
        new_session.working_dir = preserve_working_dir;
        new_session.ensure_initial_session_context_message();

        self.session = new_session;
        self.reset_runtime_state_for_session_change();
        self.provider_session_id = None;
        self.seed_compaction_from_session();
    }

    /// Clear provider session so the next turn sends full context.
    pub fn reset_provider_session(&mut self) {
        self.provider_session_id = None;
        self.session.provider_session_id = None;
        self.persist_session_best_effort("provider session reset");
    }

    /// Rewind the conversation to a 1-based visible conversation message index.
    ///
    /// Provider-side resumable sessions are reset so the next request sends the
    /// truncated context from scratch instead of continuing from a stale upstream
    /// conversation.
    pub fn rewind_to_message(&mut self, message_index: usize) -> Result<usize, String> {
        let message_count = self.session.visible_conversation_message_count();
        let Some(stored_len) = self
            .session
            .stored_len_for_visible_conversation_message(message_index)
        else {
            return Err(format!(
                "Invalid message number: {}. Valid range: 1-{}",
                message_index, message_count
            ));
        };

        let removed = message_count - message_index;
        self.rewind_undo_snapshot = Some(RewindUndoSnapshot {
            messages: self.session.messages.clone(),
            provider_session_id: self.provider_session_id.clone(),
            session_provider_session_id: self.session.provider_session_id.clone(),
            visible_message_count: message_count,
        });
        self.session.truncate_messages(stored_len);
        self.session.updated_at = chrono::Utc::now();
        self.provider_session_id = None;
        self.session.provider_session_id = None;
        self.cache_tracker.reset();
        self.locked_tools = None;
        self.reset_tool_output_tracking();
        self.persist_session_best_effort("conversation rewind");
        Ok(removed)
    }

    pub fn undo_rewind(&mut self) -> Result<usize, String> {
        let Some(snapshot) = self.rewind_undo_snapshot.take() else {
            return Err("No rewind to undo.".to_string());
        };

        let current_count = self.session.visible_conversation_message_count();
        let restored = snapshot.visible_message_count.saturating_sub(current_count);
        self.session.replace_messages(snapshot.messages);
        self.provider_session_id = snapshot.provider_session_id;
        self.session.provider_session_id = snapshot.session_provider_session_id;
        self.session.updated_at = chrono::Utc::now();
        self.cache_tracker.reset();
        self.locked_tools = None;
        self.reset_tool_output_tracking();
        self.persist_session_best_effort("conversation rewind undo");
        Ok(restored)
    }

    /// Unlock the tool list so the next API request picks up any new tools.
    /// Called after MCP reload or when the user explicitly wants new tools.
    pub fn unlock_tools(&mut self) {
        if self.locked_tools.is_some() {
            logging::info("Tool list unlocked — next request will pick up current tools");
            self.locked_tools = None;
            self.cache_tracker.reset();
        }
        // Allow the late-MCP-registration recheck to fire once for the next
        // snapshot (e.g. after an explicit `mcp` reload).
        self.mcp_late_register_resolved = false;
    }

    /// Unlock tools if a tool execution may have changed the registry
    /// (e.g., mcp connect/disconnect/reload)
    pub(super) fn unlock_tools_if_needed(&mut self, tool_name: &str) {
        if tool_name == "mcp" {
            self.unlock_tools();
        }
    }

    pub fn is_canary(&self) -> bool {
        self.session.is_canary
    }

    pub fn is_debug(&self) -> bool {
        self.session.is_debug
    }

    pub fn set_canary(&mut self, build_hash: &str) {
        self.session.set_canary(build_hash);
        if let Err(err) = self.session.save() {
            logging::error(&format!("Failed to persist canary session state: {}", err));
        }
    }

    /// Mark this session as a debug/test session
    /// Set a custom system prompt override (used by ambient mode).
    /// When set, this replaces the normal system prompt entirely.
    pub fn set_system_prompt(&mut self, prompt: &str) {
        self.system_prompt_override = Some(prompt.to_string());
    }

    pub fn set_debug(&mut self, is_debug: bool) {
        self.session.set_debug(is_debug);
        if let Err(err) = self.session.save() {
            logging::error(&format!("Failed to persist debug session state: {}", err));
        }
    }

    /// Enable or disable memory features for this session.
    pub fn set_memory_enabled(&mut self, enabled: bool) {
        self.memory_enabled = enabled;
        if !enabled {
            crate::memory::clear_pending_memory(&self.session.id);
        }
    }

    /// Mark this session as an inline swarm worker. When enabled, the streaming
    /// loop publishes a throttled output tail to the global bus so a
    /// coordinator can render a live inline gallery viewport for it.
    pub fn set_inline_output_tap(&mut self, enabled: bool) {
        self.inline_output_tap = enabled;
    }

    /// Whether this session streams an inline output tail to the bus.
    pub(crate) fn inline_output_tap(&self) -> bool {
        self.inline_output_tap
    }

    /// Check whether memory features are enabled for this session.
    pub fn memory_enabled(&self) -> bool {
        self.memory_enabled
    }

    /// Set the stdin request channel for interactive stdin forwarding
    pub fn set_stdin_request_tx(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::tool::StdinInputRequest>,
    ) {
        self.stdin_request_tx = Some(tx);
    }

    pub(super) async fn tool_definitions(&mut self) -> Vec<ToolDefinition> {
        if self.session.is_canary {
            self.registry.register_selfdev_tools().await;
        }

        // Return locked tools if available (prevents cache invalidation from
        // tools arriving asynchronously after the first API request).
        //
        // Exception: MCP servers connect on a background task and register
        // `mcp__*` tools seconds after the session starts — typically *after*
        // the first turn has already locked the snapshot. We deliberately do
        // NOT block the first turn on MCP connection: servers can be slow or
        // hang, and we want the user to be able to talk to the agent the moment
        // the session spawns. The price is that the first locked snapshot is
        // missing MCP tools, and the only other unlock path fires when the model
        // calls the `mcp` management tool — which it cannot do without first
        // seeing MCP tools (#206).
        //
        // So, exactly once per locked snapshot, if MCP tools have since appeared
        // in the registry, we rebuild. This is a single intentional provider
        // prompt-cache miss (the turn MCP tools first appear). The
        // `mcp_late_register_resolved` flag makes this a one-shot check so we do
        // not rescan the registry on every subsequent turn.
        if let Some(ref locked) = self.locked_tools {
            if self.mcp_late_register_resolved {
                return locked.clone();
            }
            if self.registry_has_new_mcp_tools(locked).await {
                logging::info(
                    "MCP tools registered after first turn locked the tool snapshot — \
                     rebuilding once to expose them. This is one intentional prompt-cache \
                     miss; we accept it so the agent is reachable immediately at spawn \
                     instead of blocking on MCP connection (#206).",
                );
                // Latch the one-shot guard and drop the stale snapshot directly.
                // We intentionally do NOT call `unlock_tools()` here, because that
                // re-arms the guard (it is the explicit-reload path) and would let
                // the recheck fire again on every later turn.
                self.mcp_late_register_resolved = true;
                self.locked_tools = None;
                self.cache_tracker.reset();
            } else {
                // No MCP tools have appeared. They may still be connecting, so
                // leave the guard unset and re-check on the next turn. Once they
                // appear (or never do, after the registry settles) we stop.
                return locked.clone();
            }
        }

        let tools = self.build_filtered_tool_definitions().await;

        // Lock the tool list to prevent cache invalidation when more tools
        // arrive asynchronously mid-session.
        logging::info(&format!(
            "Locking tool list at {} tools for cache stability",
            tools.len()
        ));
        self.locked_tools = Some(tools.clone());
        tools
    }

    /// Build the agent's tool definitions from the registry, applying the
    /// session's `allowed_tools`, `disabled_tools`, and self-dev filters.
    async fn build_filtered_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut tools = self.registry.definitions(self.allowed_tools.as_ref()).await;
        if !self.disabled_tools.is_empty() {
            tools.retain(|tool| !self.disabled_tools.contains(&tool.name));
        }
        Self::apply_selfdev_tool_surface(&mut tools, self.session.is_canary);
        tools
    }

    /// Tailor the `selfdev` tool definition to the session mode.
    ///
    /// The registry stores a single shared `selfdev` tool with a default
    /// (non-self-dev) schema. Self-dev sessions get the full build/test/reload
    /// surface; every other session keeps the lightweight on-ramp surface
    /// (`enter`, `setup`, `reload`, `status`, `find-config`). The tool stays
    /// available in all sessions so the agent can always enter self-dev mode.
    fn apply_selfdev_tool_surface(tools: &mut [ToolDefinition], is_canary: bool) {
        for tool in tools.iter_mut() {
            if tool.name == "selfdev" {
                tool.description =
                    crate::tool::selfdev::SelfDevTool::description_for(is_canary).to_string();
                tool.input_schema = crate::tool::selfdev::SelfDevTool::schema_for(is_canary);
            }
        }
    }

    /// Returns true if the registry contains `mcp__*` tools (subject to the
    /// session's `allowed_tools` filter) that are not present in the currently
    /// locked snapshot. Used to detect the async MCP-registration race (#206).
    async fn registry_has_new_mcp_tools(&self, locked: &[ToolDefinition]) -> bool {
        let registry_names = self.registry.tool_names().await;
        let allowed = self.allowed_tools.as_ref();
        registry_names.iter().any(|name| {
            name.starts_with("mcp__")
                && allowed.map(|set| set.contains(name)).unwrap_or(true)
                && !self.disabled_tools.contains(name)
                && !locked.iter().any(|t| &t.name == name)
        })
    }

    pub async fn tool_names(&self) -> Vec<String> {
        self.tool_definitions_for_debug()
            .await
            .into_iter()
            .map(|tool| tool.name)
            .collect()
    }

    /// Get full tool definitions for debug introspection (bypasses lock)
    pub async fn tool_definitions_for_debug(&self) -> Vec<crate::message::ToolDefinition> {
        if self.session.is_canary {
            self.registry.register_selfdev_tools().await;
        }
        let mut tools = self.registry.definitions(self.allowed_tools.as_ref()).await;
        if !self.disabled_tools.is_empty() {
            tools.retain(|tool| !self.disabled_tools.contains(&tool.name));
        }
        Self::apply_selfdev_tool_surface(&mut tools, self.session.is_canary);
        tools
    }

    pub async fn execute_tool(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<crate::tool::ToolOutput> {
        self.validate_tool_allowed(name, Some(&input)).await?;

        let call_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| format!("debug-{}", d.as_millis()))
            .unwrap_or_else(|_| "debug".to_string());
        let ctx = ToolContext {
            session_id: self.session.id.clone(),
            message_id: self.session.id.clone(),
            tool_call_id: call_id,
            working_dir: self.working_dir().map(PathBuf::from),
            stdin_request_tx: self.stdin_request_tx.clone(),
            graceful_shutdown_signal: Some(self.graceful_shutdown.clone()),
            execution_mode: ToolExecutionMode::Direct,
            best_of_n_run_id: self.best_of_n_run_id.clone(),
            best_of_n_candidate_id: self.best_of_n_candidate_id.clone(),
        };
        self.registry.execute(name, input, ctx).await
    }

    pub fn add_manual_tool_use(
        &mut self,
        tool_call_id: String,
        tool_name: String,
        input: serde_json::Value,
    ) -> Result<String> {
        let message_id = self.add_message(
            Role::Assistant,
            vec![ContentBlock::ToolUse {
                id: tool_call_id,
                name: tool_name,
                input,
                thought_signature: None,
            }],
        );
        self.session.save()?;
        Ok(message_id)
    }

    pub fn add_manual_tool_result(
        &mut self,
        tool_call_id: String,
        output: crate::tool::ToolOutput,
        duration_ms: u64,
    ) -> Result<()> {
        let blocks = tool_output_to_content_blocks(tool_call_id, output);
        self.add_message_with_duration(Role::User, blocks, Some(duration_ms));
        self.session.save()?;
        Ok(())
    }

    pub fn add_manual_tool_error(
        &mut self,
        tool_call_id: String,
        error: String,
        duration_ms: u64,
    ) -> Result<()> {
        self.add_message_with_duration(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: tool_call_id,
                content: error,
                is_error: Some(true),
            }],
            Some(duration_ms),
        );
        self.session.save()?;
        Ok(())
    }

    pub(super) async fn validate_tool_allowed(
        &self,
        name: &str,
        input: Option<&serde_json::Value>,
    ) -> Result<()> {
        if let Some(allowed) = self.allowed_tools.as_ref()
            && !allowed.contains(name)
        {
            return Err(anyhow::anyhow!("Tool '{}' is not allowed", name));
        }
        if self.disabled_tools.contains(name) {
            return Err(anyhow::anyhow!("Tool '{}' is disabled", name));
        }
        // Delegate to dcg_bridge for permission mode evaluation.
        // Uses classify_for_session so that per-session overrides (e.g. a
        // subagent spawned with an explicit mode) are honored; falls back to
        // the global mode when no per-session override is set.
        // - Allow → proceed
        // - Deny → block with error
        // - Prompt → block with a permission-required error and surface a
        //   TUI dialog via the bus
        {
            use crate::dcg_bridge::{self, BridgeDecision};
            match dcg_bridge::classify_for_session(name, &self.session.id) {
                BridgeDecision::Deny {
                    reason,
                    alternatives,
                    ..
                } => {
                    let msg = if alternatives.is_empty() {
                        format!(
                            "Tool '{}' blocked: {}. Current mode: {:?}",
                            name,
                            reason,
                            crate::dcg_bridge::current_mode()
                        )
                    } else {
                        format!(
                            "Tool '{}' blocked: {}. Alternatives: {}. Current mode: {:?}",
                            name,
                            reason,
                            alternatives.join(", "),
                            crate::dcg_bridge::current_mode()
                        )
                    };
                    crate::logging::info(&format!(
                        "[permission] Denied tool '{}': {} (mode {:?})",
                        name,
                        reason,
                        crate::dcg_bridge::current_mode()
                    ));
                    return Err(anyhow::anyhow!(msg));
                }
                BridgeDecision::Prompt {
                    reason,
                    allow_once_code,
                    alternatives,
                } => {
                    // Publish bus event so TUI can show a permission dialog
                    let tool_input =
                        input.and_then(|v| if v.is_null() { None } else { Some(v.clone()) });
                    crate::bus::Bus::global().publish(crate::bus::BusEvent::PermissionRequested(
                        crate::bus::PermissionRequested {
                            session_id: self.session.id.clone(),
                            tool_name: name.to_string(),
                            reason: reason.clone(),
                            allow_once_code: allow_once_code.clone(),
                            alternatives: alternatives.clone(),
                            tool_input,
                        },
                    ));

                    // Await user response — tool execution PAUSES here (Claude Code behavior)
                    match crate::dcg_bridge::await_permission_response().await {
                        Ok(true) => {
                            crate::dcg_bridge::approve_session_action(&self.session.id, name);
                            crate::logging::info(&format!(
                                "[permission] Approved '{}' for session {}",
                                name, self.session.id
                            ));
                            return Ok(());
                        }
                        Ok(false) => {
                            let msg = format!(
                                "Tool '{}' denied by user. Current mode: {:?}",
                                name,
                                crate::dcg_bridge::current_mode()
                            );
                            crate::logging::info(&format!(
                                "[permission] Denied '{}' by user for session {}",
                                name, self.session.id
                            ));
                            return Err(anyhow::anyhow!(msg));
                        }
                        Err(e) => {
                            let msg = format!(
                                "Tool '{}' permission cancelled: {}. Current mode: {:?}",
                                name,
                                e,
                                crate::dcg_bridge::current_mode()
                            );
                            return Err(anyhow::anyhow!(msg));
                        }
                    }
                }
                BridgeDecision::Allow => {}
            }
        }
        Ok(())
    }

    /// Restore a session by ID (loads from disk)
    pub fn restore_session(&mut self, session_id: &str) -> Result<SessionStatus> {
        let restore_start = Instant::now();
        let load_start = Instant::now();
        let session = Session::load(session_id)?;
        let load_ms = load_start.elapsed().as_millis();
        logging::info(&format!(
            "Restoring session '{}' with {} messages, provider_session_id: {:?}, status: {}",
            session_id,
            session.messages.len(),
            session.provider_session_id,
            session.status.display()
        ));
        let previous_status = session.status.clone();

        let assign_start = Instant::now();
        let previous_session_id = self.session.id.clone();
        // Restore provider_session_id for Claude CLI session resume
        self.provider_session_id = session.provider_session_id.clone();
        self.session = session;
        crate::tool::clear_session_tool_policy(&previous_session_id);
        crate::tool::set_session_tool_policy(
            &self.session.id,
            self.allowed_tools.clone(),
            self.disabled_tools.clone(),
        );
        let assign_ms = assign_start.elapsed().as_millis();

        let reset_start = Instant::now();
        self.reset_runtime_state_for_session_change();
        let restored_soft_interrupts = self.restore_persisted_soft_interrupts();
        let reset_ms = reset_start.elapsed().as_millis();

        let model_start = Instant::now();
        if let Some(model) = self.session.model.clone() {
            let model_request =
                crate::provider::MultiProvider::model_switch_request_for_session_route(
                    &model,
                    self.session.provider_key.as_deref(),
                    self.session.route_api_method.as_deref(),
                );
            if let Err(e) =
                crate::provider::set_model_with_auth_refresh(self.provider.as_ref(), &model_request)
            {
                logging::error(&format!(
                    "Failed to restore session model '{}' via '{}': {}",
                    model, model_request, e
                ));
            }
        } else {
            self.session.model = Some(self.provider.model());
        }
        self.restore_reasoning_effort_from_session();
        let model_ms = model_start.elapsed().as_millis();

        let mark_active_start = Instant::now();
        self.session.mark_active();
        let mark_active_ms = mark_active_start.elapsed().as_millis();
        self.sync_memory_dedup_state_from_session();

        logging::info(&format!(
            "restore_session: loaded session {} with {} messages, calling seed_compaction",
            session_id,
            self.session.messages.len()
        ));
        let compaction_start = Instant::now();
        self.seed_compaction_from_session();
        let compaction_ms = compaction_start.elapsed().as_millis();

        let env_snapshot_start = Instant::now();
        self.log_env_snapshot("resume");
        let env_snapshot_ms = env_snapshot_start.elapsed().as_millis();
        // Dispatch SessionStart hook on resume (fire-and-forget, observational only)
        {
            let registry = self.hook_registry.clone();
            let config = self.dispatch_config.clone();
            let sid = self.session.id.clone();
            let cwd = self.session.working_dir.clone().unwrap_or_default();
            let hook_input = HookInputBuilder::new()
                .session(&sid, &cwd)
                .event("SessionStart")
                .build();
            let ctx = HookContext::for_session_start(sid, cwd);
            let event = HookEvent::SessionStart;
            tokio::spawn(async move {
                let handlers = registry.get_matching(&event, &ctx);
                if !handlers.is_empty() {
                    jcode_hooks::dispatch_hooks(&event, &hook_input, &handlers, &config).await;
                }
            });
        }

        let save_start = Instant::now();
        if let Err(err) = self.session.save() {
            logging::error(&format!(
                "Failed to persist resumed session state for {}: {}",
                session_id, err
            ));
        }
        let save_ms = save_start.elapsed().as_millis();

        logging::info(&format!(
            "[TIMING] restore_session: session={}, messages={}, restored_soft_interrupts={}, load={}ms, assign={}ms, reset={}ms, model={}ms, mark_active={}ms, compaction={}ms, env_snapshot={}ms, save={}ms, total={}ms",
            session_id,
            self.session.messages.len(),
            restored_soft_interrupts,
            load_ms,
            assign_ms,
            reset_ms,
            model_ms,
            mark_active_ms,
            compaction_ms,
            env_snapshot_ms,
            save_ms,
            restore_start.elapsed().as_millis(),
        ));
        logging::info(&format!(
            "Session restored: {} messages in session",
            self.session.messages.len()
        ));

        // Dispatch SessionUpdated hooks — session state changed to "active" via restore
        {
            let registry = self.hook_registry.clone();
            let config = self.dispatch_config.clone();
            let session_id = self.session.id.clone();
            let cwd = self.session.working_dir.clone().unwrap_or_default();
            let prev = previous_status.display().to_string();
            let hook_input = HookInputBuilder::new()
                .session(&session_id, &cwd)
                .event("SessionUpdated")
                .session_state(&prev, "active", "session_restored")
                .build();
            let ctx = HookContext::for_session_updated(session_id, cwd);
            let event = HookEvent::SessionUpdated;
            tokio::spawn(async move {
                let handlers = registry.get_matching(&event, &ctx);
                if !handlers.is_empty() {
                    jcode_hooks::dispatch_hooks(&event, &hook_input, &handlers, &config).await;
                }
            });
        }

        Ok(previous_status)
    }

    /// Get conversation history for sync
    pub fn get_history(&self) -> Vec<HistoryMessage> {
        crate::session::render_messages(&self.session)
            .into_iter()
            .map(|msg| HistoryMessage {
                role: msg.role,
                content: msg.content,
                tool_calls: if msg.tool_calls.is_empty() {
                    None
                } else {
                    Some(msg.tool_calls)
                },
                tool_data: msg.tool_data,
            })
            .collect()
    }

    pub fn get_history_and_rendered_images(
        &self,
    ) -> (Vec<HistoryMessage>, Vec<crate::session::RenderedImage>) {
        let (messages, images) = crate::session::render_messages_and_images(&self.session);
        let history = messages
            .into_iter()
            .map(|msg| HistoryMessage {
                role: msg.role,
                content: msg.content,
                tool_calls: if msg.tool_calls.is_empty() {
                    None
                } else {
                    Some(msg.tool_calls)
                },
                tool_data: msg.tool_data,
            })
            .collect();
        (history, images)
    }

    pub fn get_history_and_rendered_images_with_compacted_history(
        &self,
        compacted_history_visible: usize,
    ) -> (
        Vec<HistoryMessage>,
        Vec<crate::session::RenderedImage>,
        Option<crate::session::RenderedCompactedHistoryInfo>,
    ) {
        let (messages, images, compacted_info) =
            crate::session::render_messages_and_images_with_compacted_history(
                &self.session,
                compacted_history_visible,
            );
        let history = messages
            .into_iter()
            .map(|msg| HistoryMessage {
                role: msg.role,
                content: msg.content,
                tool_calls: if msg.tool_calls.is_empty() {
                    None
                } else {
                    Some(msg.tool_calls)
                },
                tool_data: msg.tool_data,
            })
            .collect();
        (history, images, compacted_info)
    }

    pub fn get_tool_call_summaries(&self, limit: usize) -> Vec<crate::protocol::ToolCallSummary> {
        crate::session::summarize_tool_calls(&self.session, limit)
    }

    /// Start an interactive REPL
    pub async fn repl(&mut self) -> Result<()> {
        println!("J-Code - Coding Agent");
        println!("Type your message, or 'quit' to exit.");

        // Show available skills
        let skills = self.current_skills_snapshot();
        let skill_list = skills.list();
        if !skill_list.is_empty() {
            println!(
                "Available skills: {}",
                skill_list
                    .iter()
                    .map(|s| format!("/{}", s.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();

        loop {
            print!("> ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            let input = input.trim();
            if input.is_empty() {
                continue;
            }

            if input == "quit" || input == "exit" {
                break;
            }

            if input == "clear" {
                self.clear();
                println!("Conversation cleared.");
                continue;
            }

            // Check for skill invocation
            if let Some(skill_name) = SkillRegistry::parse_invocation(input) {
                if let Some(skill) = skills.get(skill_name) {
                    println!("Activating skill: {}", skill.name);
                    println!("{}\n", skill.description);
                    self.active_skill = Some(skill_name.to_string());
                    continue;
                } else {
                    println!("Unknown skill: /{}", skill_name);
                    println!(
                        "Available: {}",
                        skills
                            .list()
                            .iter()
                            .map(|s| format!("/{}", s.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    continue;
                }
            }

            if let Err(e) = self.run_once(input).await {
                eprintln!("\nError: {}\n", e);
            }

            println!();
        }

        // Extract memories from session before exiting
        self.extract_session_memories().await;

        Ok(())
    }

    /// Extract memories from the session transcript
    /// Returns the number of memories extracted, or 0 if none/skipped
    pub async fn extract_session_memories(&self) -> usize {
        if !self.memory_enabled {
            return 0;
        }

        // Need at least 4 messages for meaningful extraction
        if self.session.messages.len() < 4 {
            return 0;
        }

        logging::info(&format!(
            "Extracting memories from {} messages",
            self.session.messages.len()
        ));

        // Build transcript
        let mut transcript = String::new();
        for msg in &self.session.messages {
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
            logging::info("Memory extraction skipped: LLM judge unavailable");
            return 0;
        }

        // Extract using sidecar
        let sidecar = crate::sidecar::Sidecar::new();
        match sidecar.extract_memories(&transcript).await {
            Ok(extracted) if !extracted.is_empty() => {
                let manager = self
                    .session
                    .working_dir
                    .as_deref()
                    .map(|dir| crate::memory::MemoryManager::new().with_project_dir(dir))
                    .unwrap_or_default();
                let mut stored_count = 0;

                for memory in &extracted {
                    let category = crate::memory::MemoryCategory::from_extracted(&memory.category);

                    let trust = match memory.trust.as_str() {
                        "high" => crate::memory::TrustLevel::High,
                        "low" => crate::memory::TrustLevel::Low,
                        _ => crate::memory::TrustLevel::Medium,
                    };

                    let entry = crate::memory::MemoryEntry::new(category, &memory.content)
                        .with_source(&self.session.id)
                        .with_trust(trust);

                    // Use the MemoryProvider trait method so this works with
                    // any backend (legacy MemoryManager or MempalaceAdapter).
                    use jcode_memory_types::{MemoryProvider, MemoryScope};
                    if manager.remember(entry, MemoryScope::Project).await.is_ok() {
                        stored_count += 1;
                    }
                }

                if stored_count > 0 {
                    logging::info(&format!("Extracted {} memories from session", stored_count));
                }
                stored_count
            }
            Ok(_) => 0,
            Err(e) => {
                logging::info(&format!("Memory extraction skipped: {}", e));
                0
            }
        }
    }
}
