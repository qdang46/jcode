use super::helpers::{
    agent_model_default_summary, agent_model_target_config_path, agent_model_target_label,
    agent_model_target_slug, load_agent_model_override, model_entry_base_name,
    model_entry_saved_spec,
};
use super::*;
use crate::tui::{
    AgentModelTarget, InlineInteractiveState, PickerAction, PickerEntry, PickerKind, PickerOption,
    RunningItem, RunningItemKind, RunningItemStatus,
};

impl App {
    /// Open the agents picker with Running + Library tabs (Claude Code style).
    /// Tab 0 (column 0) = Running: live subagents, background tasks, batch tools
    /// Tab 1 (column 1) = Library: saved agent definitions / model overrides
    pub(crate) fn open_agents_picker(&mut self) {
        let mut entries: Vec<PickerEntry> = Vec::new();

        // === Running tab entries (column 0) ===

        // Build a section header for the Running tab
        entries.push(PickerEntry {
            name: "── Running ──────────────────────".into(),
            options: vec![],
            action: PickerAction::SectionHeader,
            selected_option: 0,
            is_current: false,
            is_default: false,
            is_favorite: false,
            recommended: false,
            recommendation_rank: usize::MAX,
            usage_score: 0,
            old: false,
            created_date: None,
            effort: None,
            is_free: false,
            is_latest: false,
        });

        // Collect live subagent/background/batch items from current state
        let running_items = self.build_running_tab_entries();
        let running_count = running_items.len();
        entries.extend(running_items);

        // === Library tab entries (column 1) ===

        // Section header for Library tab
        entries.push(PickerEntry {
            name: "── Library ──────────────────────".into(),
            options: vec![],
            action: PickerAction::SectionHeader,
            selected_option: 0,
            is_current: false,
            is_default: false,
            is_favorite: false,
            recommended: false,
            recommendation_rank: usize::MAX,
            usage_score: 0,
            old: false,
            created_date: None,
            effort: None,
            is_free: false,
            is_latest: false,
        });

        // "Create new agent" entry
        entries.push(PickerEntry {
            name: "  + Create new agent".into(),
            options: vec![PickerOption {
                provider: "action".into(),
                api_method: "create".into(),
                available: true,
                detail: "Create a new subagent definition file".into(),
                estimated_reference_cost_micros: None,
                context_window: None,
                latency_ms: None,
                cost_per_million_input: None,
                cost_per_million_output: None,
                is_free: false,
                is_latest: false,
            }],
            action: PickerAction::CreateAgent,
            selected_option: 0,
            is_current: false,
            is_default: false,
            is_favorite: false,
            recommended: false,
            recommendation_rank: usize::MAX,
            usage_score: 0,
            old: false,
            created_date: None,
            effort: None,
            is_free: false,
            is_latest: false,
        });

        // "Generate agent via AI" entry
        entries.push(PickerEntry {
            name: "  + Generate via AI".into(),
            options: vec![PickerOption {
                provider: "action".into(),
                api_method: "generate".into(),
                available: true,
                detail: "Describe an agent, generate with current model".into(),
                estimated_reference_cost_micros: None,
                context_window: None,
                latency_ms: None,
                cost_per_million_input: None,
                cost_per_million_output: None,
                is_free: false,
                is_latest: false,
            }],
            action: PickerAction::GenerateAgent,
            selected_option: 0,
            is_current: false,
            is_default: false,
            is_favorite: false,
            recommended: false,
            recommendation_rank: usize::MAX,
            usage_score: 0,
            old: false,
            created_date: None,
            effort: None,
            is_free: false,
            is_latest: false,
        });

        // Load agent definitions from disk
        let mut registry = jcode_agent_runtime::AgentRegistry::new();
        let home = dirs::home_dir().map(|h| h.join(".jcode/agents"));
        let cwd = std::env::current_dir().ok().map(|d| d.join(".jcode/agents"));
        if let Some(path) = &home {
            let _ = registry.load_directory(path, jcode_agent_runtime::SourceKind::UserGlobal);
        }
        if let Some(path) = &cwd {
            let _ = registry.load_directory(path, jcode_agent_runtime::SourceKind::ProjectLocal);
        }

        // Add each loaded agent definition to Library tab
        let library_agent_count: usize;
        {
            let sorted = registry.iter_sorted();
            library_agent_count = sorted.len();
            for loaded in &sorted {
                let def = &loaded.definition;
                // Color badge for agent entry name
                let badge = def.color.as_deref()
                    .and_then(agent_color_icon)
                    .unwrap_or("  ");
                let display_name = format!("{} {}", badge, def.display_name);
                entries.push(PickerEntry {
                    name: display_name,
                    options: vec![PickerOption {
                        provider: "config".into(),
                        api_method: "edit".into(),
                        available: true,
                        detail: format!("{} tools · model: {} · color: {}",
                            def.tool_names.len(),
                            def.model_override.as_deref().unwrap_or("inherit"),
                            def.color.as_deref().unwrap_or("default"),
                        ),
                        estimated_reference_cost_micros: None,
                        context_window: None,
                        latency_ms: None,
                        cost_per_million_input: None,
                        cost_per_million_output: None,
                        is_free: false,
                        is_latest: false,
                    }],
                    action: {
                        let source_path = match &loaded.source {
                            jcode_agent_runtime::registry::AgentSource::UserGlobal { path } => path.to_string_lossy().to_string(),
                            jcode_agent_runtime::registry::AgentSource::ProjectLocal { path } => path.to_string_lossy().to_string(),
                            jcode_agent_runtime::registry::AgentSource::Builtin => String::new(),
                        };
                        PickerAction::EditAgent {
                            agent_id: def.id.clone(),
                            source_path,
                        }
                    },
                    selected_option: 0,
                    is_current: false,
                    is_default: false,
                    is_favorite: false,
                    recommended: false,
                    recommendation_rank: usize::MAX,
                    usage_score: 0,
                    old: false,
                    created_date: None,
                    effort: None,
                    is_free: false,
                    is_latest: false,
                });
        }
        }

        // Also add the 5 built-in agent model override entries
        let models = [
            AgentModelTarget::Swarm,
            AgentModelTarget::Review,
            AgentModelTarget::Judge,
            AgentModelTarget::Memory,
            AgentModelTarget::Ambient,
        ]
        .into_iter()
        .map(|target| {
            let configured = load_agent_model_override(target);
            let summary = configured
                .clone()
                .unwrap_or_else(|| agent_model_default_summary(target, self));
            PickerEntry {
                name: format!("  {} (config)", agent_model_target_label(target)),
                options: vec![PickerOption {
                    provider: summary,
                    api_method: agent_model_target_config_path(target).to_string(),
                    available: true,
                    detail: format!("/agents {}", agent_model_target_slug(target)),
                    estimated_reference_cost_micros: None,
                    context_window: None,
                    latency_ms: None,
                    cost_per_million_input: None,
                    cost_per_million_output: None,
                    is_free: false,
                    is_latest: false,
                }],
                action: PickerAction::AgentTarget(target),
                selected_option: 0,
                is_current: false,
                is_default: configured.is_some(),
                is_favorite: false,
                recommended: false,
                recommendation_rank: usize::MAX,
                usage_score: 0,
                old: false,
                created_date: None,
                effort: None,
                is_free: false,
                is_latest: false,
            }
        })
        .collect::<Vec<_>>();

        let model_override_count = models.len();
        entries.extend(models);
        // Build filtered indices: column 0 -> running entries, column 1 -> library entries
        let running_start = 1; // after "── Running ──" header
        let running_end = running_start + running_count;
        let library_header_idx = running_end; // "── Library ──" header
        let library_create_idx = library_header_idx + 1; // "+ Create new agent"
        let library_generate_idx = library_create_idx + 1; // "+ Generate via AI"
        let library_agent_start = library_generate_idx + 1; // loaded agent files
        let library_agent_end = library_agent_start + library_agent_count;
        let library_end = library_agent_end + model_override_count;

        let filtered: Vec<usize> = (running_start..running_end).collect();

        // Store metadata in filter: "running_end:library_end"
        // Secret metadata: filter = running_end:library_end for tab index reconstruction
        let meta = format!("{}:{}", running_end, library_end);

        self.inline_view_state = None;
        let mut picker = InlineInteractiveState {
            kind: PickerKind::Agents,
            filtered,
            entries,
            selected: 0,
            column: 0,
            filter: meta,
            preview: false,
        };
        Self::rebuild_agents_picker_filtered(&mut picker);
        self.inline_interactive_state = Some(picker);
        self.input.clear();
        self.cursor_pos = 0;
    }

    /// Rebuild the `filtered` list for the agents picker based on the current column (tab).
    /// The filter metadata is stored in `picker.filter` as "running_end:library_end".
    pub(super) fn rebuild_agents_picker_filtered(picker: &mut InlineInteractiveState) {
        if picker.kind != PickerKind::Agents {
            return;
        }
        // Parse metadata from filter: "running_end:library_end"
        let parts: Vec<&str> = picker.filter.split(':').collect();
        if parts.len() < 2 {
            return;
        }
        let running_end: usize = parts[0].parse().unwrap_or(0);
        let library_end: usize = parts[1].parse().unwrap_or(0);
        let running_start = 1; // after "── Running ──" section header
        let library_start = running_end + 1; // after Library section header

        picker.filtered = if picker.column == 0 {
            // Running tab: entries between running_start and running_end
            (running_start..running_end).collect()
        } else {
            // Library tab: entries between library_start and library_end
            (library_start..library_end).collect()
        };
        if picker.selected >= picker.filtered.len() && !picker.filtered.is_empty() {
            picker.selected = picker.filtered.len() - 1;
        }
    }

    pub(crate) fn open_login_picker_inline(&mut self) {
        self.open_auth_provider_picker_inline(false);
    }

    pub(crate) fn open_logout_picker_inline(&mut self) {
        self.open_auth_provider_picker_inline(true);
    }

    fn open_auth_provider_picker_inline(&mut self, logout: bool) {
        let status = crate::auth::AuthStatus::check_fast();
        let providers = crate::provider_catalog::tui_login_providers();
        let mut models = providers
            .into_iter()
            .filter(|provider| {
                !(logout
                    && matches!(
                        provider.target,
                        crate::provider_catalog::LoginProviderTarget::AutoImport
                    ))
            })
            .map(|provider| {
                let assessment = status.assessment_for_provider(provider);
                let auth_state = assessment.state;
                let state_label = match auth_state {
                    crate::auth::AuthState::Available => {
                        if matches!(
                            provider.target,
                            crate::provider_catalog::LoginProviderTarget::AutoImport
                        ) {
                            "detected"
                        } else {
                            "configured"
                        }
                    }
                    crate::auth::AuthState::Expired => "attention",
                    crate::auth::AuthState::NotConfigured => "setup",
                };
                PickerEntry { name: provider.display_name.to_string(),
                options: vec![PickerOption {
                    provider: provider.auth_kind.label().to_string(),
                    api_method: state_label.to_string(),
                    available: true,
                    detail: format!("{} · {}", assessment.method_detail, provider.menu_detail),
                    estimated_reference_cost_micros: None,
                    context_window: None,
                    latency_ms: None,
                    cost_per_million_input: None,
                    cost_per_million_output: None,
                    is_free: false,
                    is_latest: false,
                }],
                action: if logout {
                    PickerAction::Logout(provider)
                } else {
                    PickerAction::Login(provider)
                },
                selected_option: 0,
                is_current: auth_state == crate::auth::AuthState::Available,
                is_default: false,
                is_favorite: false,
                recommended: provider.recommended,
                recommendation_rank: usize::MAX,
                usage_score: 0,
                old: false,
                created_date: None,
                effort: None, is_free: false, is_latest: false, }
            })
            .collect::<Vec<_>>();

        if logout {
            models.insert(
                0,
                PickerEntry { name: "All providers".to_string(),
                options: vec![PickerOption {
                    provider: "all".to_string(),
                    api_method: "logout".to_string(),
                    available: true,
                    detail: "Log out of every provider with a saved session".to_string(),
                    estimated_reference_cost_micros: None,
                    context_window: None,
                    latency_ms: None,
                    cost_per_million_input: None,
                    cost_per_million_output: None,
                    is_free: false,
                    is_latest: false,
                }],
                action: PickerAction::LogoutAll,
                selected_option: 0,
                is_current: false,
                is_default: false,
                is_favorite: false,
                recommended: false,
                recommendation_rank: usize::MAX,
                usage_score: 0,
                old: false,
                created_date: None,
                effort: None, is_free: false, is_latest: false, },
            );
        }

        self.inline_view_state = None;
        self.inline_interactive_state = Some(InlineInteractiveState {
            kind: PickerKind::Login,
            filtered: (0..models.len()).collect(),
            entries: models,
            selected: 0,
            column: 0,
            filter: String::new(),
            preview: false,
        });
        self.input.clear();
        self.cursor_pos = 0;
    }

    pub(crate) fn open_agent_model_picker(&mut self, target: AgentModelTarget) {
        let configured = load_agent_model_override(target);
        let inherit_summary = agent_model_default_summary(target, self);
        self.open_model_picker();
        let load_started = std::time::Instant::now();
        while self.pending_model_picker_load.is_some()
            && load_started.elapsed() < std::time::Duration::from_secs(2)
        {
            if self.poll_model_picker_load() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        if let Some(ref mut picker) = self.inline_interactive_state {
            if target == AgentModelTarget::Memory {
                picker.entries.retain(|entry| {
                    matches!(
                        crate::provider::provider_for_model(&model_entry_base_name(entry)),
                        Some("openai" | "claude")
                    )
                });
            }

            for entry in &mut picker.entries {
                let matches_saved = configured.as_deref().map(|saved| {
                    let base = model_entry_base_name(entry);
                    model_entry_saved_spec(entry) == saved || base == saved
                }) == Some(true);
                entry.action = PickerAction::AgentModelChoice {
                    target,
                    clear_override: false,
                };
                entry.is_current = matches_saved;
                entry.is_default = false;
            }

            if let Some(saved) = configured.as_deref() {
                let already_present = picker.entries.iter().any(|entry| {
                    model_entry_saved_spec(entry) == saved || model_entry_base_name(entry) == saved
                });
                if !already_present {
                    picker.entries.insert(
                        0,
                        PickerEntry { name: saved.to_string(),
                        options: vec![PickerOption {
                            provider: "saved override".to_string(),
                            api_method: agent_model_target_config_path(target).to_string(),
                            available: true,
                            detail: "not in current picker catalog".to_string(),
                            estimated_reference_cost_micros: None,
                            context_window: None,
                            latency_ms: None,
                            cost_per_million_input: None,
                            cost_per_million_output: None,
                            is_free: false,
                            is_latest: false,
                        }],
                        action: PickerAction::AgentModelChoice {
                            target,
                            clear_override: false,
                        },
                        selected_option: 0,
                        is_current: true,
                        is_default: false,
                        is_favorite: false,
                        recommended: false,
                        recommendation_rank: usize::MAX,
                        usage_score: 0,
                        old: false,
                        created_date: None,
                        effort: None, is_free: false, is_latest: false, },
                    );
                }
            }

            picker.entries.insert(
                0,
                PickerEntry { name: format!("inherit ({})", inherit_summary),
                options: vec![PickerOption {
                    provider: "default".to_string(),
                    api_method: agent_model_target_config_path(target).to_string(),
                    available: true,
                    detail: "clear saved override".to_string(),
                    estimated_reference_cost_micros: None,
                    context_window: None,
                    latency_ms: None,
                    cost_per_million_input: None,
                    cost_per_million_output: None,
                    is_free: false,
                    is_latest: false,
                }],
                action: PickerAction::AgentModelChoice {
                    target,
                    clear_override: true,
                },
                selected_option: 0,
                is_current: configured.is_none(),
                is_default: false,
                is_favorite: false,
                recommended: false,
                recommendation_rank: usize::MAX,
                usage_score: 0,
                old: false,
                created_date: None,
                effort: None, is_free: false, is_latest: false, },
            );

            picker.filtered = (0..picker.entries.len()).collect();
            picker.selected = picker
                .entries
                .iter()
                .position(|entry| entry.is_current)
                .unwrap_or(0);
            picker.column = 0;
            picker.filter.clear();
        }
    }

    /// Build running tab entries from current subagent/background/batch state.
    fn build_running_tab_entries(&self) -> Vec<PickerEntry> {
        let mut entries: Vec<PickerEntry> = Vec::new();

        // 1. Subagent status
        if let Some(status) = &self.subagent_status {
            let elapsed = self.processing_started.map(|t| t.elapsed())
                .map(|d| format_elapsed_secs(d.as_secs()))
                .unwrap_or_default();
            entries.push(PickerEntry {
                name: format!("◯ subagent"),
                options: vec![PickerOption {
                    provider: "running".into(),
                    api_method: "view".into(),
                    available: true,
                    detail: status.clone(),
                    estimated_reference_cost_micros: None,
                    context_window: None,
                    latency_ms: None,
                    cost_per_million_input: None,
                    cost_per_million_output: None,
                    is_free: false,
                    is_latest: false,
                }],
                action: PickerAction::Model, // placeholder - opens detail view
                selected_option: 0,
                is_current: false,
                is_default: false,
                is_favorite: false,
                recommended: false,
                recommendation_rank: usize::MAX,
                usage_score: 0,
                old: false,
                created_date: None,
                effort: None,
                is_free: false,
                is_latest: false,
            });
        }

        // 2. Background tasks
        let bg = crate::background::global();
        let (_count, running_tasks, _progress) = bg.running_snapshot();
        for task_name in &running_tasks {
            entries.push(PickerEntry {
                name: format!("◯ {}", task_name),
                options: vec![PickerOption {
                    provider: "running".into(),
                    api_method: "cancel".into(),
                    available: true,
                    detail: format!("background task: {}", task_name),
                    estimated_reference_cost_micros: None,
                    context_window: None,
                    latency_ms: None,
                    cost_per_million_input: None,
                    cost_per_million_output: None,
                    is_free: false,
                    is_latest: false,
                }],
                action: PickerAction::Model,
                selected_option: 0,
                is_current: false,
                is_default: false,
                is_favorite: false,
                recommended: false,
                recommendation_rank: usize::MAX,
                usage_score: 0,
                old: false,
                created_date: None,
                effort: None,
                is_free: false,
                is_latest: false,
            });
        }

        // 3. Batch subcalls
        if let Some(bp) = &self.batch_progress {
            for sub in &bp.running {
                entries.push(PickerEntry {
                    name: format!("◯ {}", sub.name),
                    options: vec![PickerOption {
                        provider: "running".into(),
                        api_method: "view".into(),
                        available: true,
                        detail: format!("batch: {}/{} done", bp.completed, bp.total),
                        estimated_reference_cost_micros: None,
                        context_window: None,
                        latency_ms: None,
                        cost_per_million_input: None,
                        cost_per_million_output: None,
                        is_free: false,
                        is_latest: false,
                    }],
                    action: PickerAction::Model,
                    selected_option: 0,
                    is_current: false,
                    is_default: false,
                    is_favorite: false,
                    recommended: false,
                    recommendation_rank: usize::MAX,
                    usage_score: 0,
                    old: false,
                    created_date: None,
                    effort: None,
                    is_free: false,
                    is_latest: false,
                });
            }
        }

        // 4. Remote swarm members
        for member in &self.remote_swarm_members {
            let icon = match member.status.as_str() {
                "running" | "processing" => "◯",
                "completed" | "done" | "ok" => "✓",
                "failed" | "error" => "✗",
                "stopped" | "cancelled" => "■",
                _ => "○",
            };
            entries.push(PickerEntry {
                name: format!("{} {}", icon, member.friendly_name.as_deref().unwrap_or("agent")),
                options: vec![PickerOption {
                    provider: member.status.clone(),
                    api_method: "view".into(),
                    available: true,
                    detail: member.detail.clone().unwrap_or_default(),
                    estimated_reference_cost_micros: None,
                    context_window: None,
                    latency_ms: None,
                    cost_per_million_input: None,
                    cost_per_million_output: None,
                    is_free: false,
                    is_latest: false,
                }],
                action: PickerAction::Model,
                selected_option: 0,
                is_current: false,
                is_default: false,
                is_favorite: false,
                recommended: false,
                recommendation_rank: usize::MAX,
                usage_score: 0,
                old: false,
                created_date: None,
                effort: None,
                is_free: false,
                is_latest: false,
            });
        }

        entries
    }
}

fn format_elapsed_secs(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

impl App {
    pub(super) fn run_agent_creation_flow(&mut self, template: &str) -> anyhow::Result<String> {
        use std::io::Write;
        let mut tmp = tempfile::Builder::new()
            .prefix("jcode-agent-")
            .suffix(".toml")
            .tempfile()?;
        tmp.write_all(template.as_bytes())?;
        let path = tmp.path().to_path_buf();

        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg("exec ${VISUAL:-${EDITOR:-vi}} \"$@\"")
            .arg("jcode-editor")
            .arg(&path)
            .status()?;
        anyhow::ensure!(status.success(), "Editor exited with non-zero status");

        let content = std::fs::read_to_string(&path)?;
        let def = jcode_agent_runtime::AgentRegistry::load_file(&path)?;

        if let Some(home) = dirs::home_dir() {
            let agents_dir = home.join(".jcode/agents");
            std::fs::create_dir_all(&agents_dir)?;
            let dest = agents_dir.join(format!("{}.toml", def.id));
            std::fs::write(&dest, &content)?;
            Ok(format!("Created agent: {} → {}", def.display_name, dest.display()))
        } else {
            anyhow::bail!("No home directory found");
        }
    }
}

/// Map agent color name to a colored circle character for the agent list.
fn agent_color_icon(color: &str) -> Option<&'static str> {
    match color {
        "red" => Some("\x1b[31m●\x1b[0m"),
        "blue" => Some("\x1b[34m●\x1b[0m"),
        "green" => Some("\x1b[32m●\x1b[0m"),
        "yellow" => Some("\x1b[33m●\x1b[0m"),
        "purple" => Some("\x1b[35m●\x1b[0m"),
        "orange" => Some("\x1b[38;5;214m●\x1b[0m"),
        "pink" => Some("\x1b[38;5;206m●\x1b[0m"),
        "cyan" => Some("\x1b[36m●\x1b[0m"),
        _ => None,
    }
}