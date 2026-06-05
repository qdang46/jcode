//! CLI commands for inspecting and managing the hooks system.
//!
//! Provides subcommands for listing, enabling/disabling, testing, and
//! displaying metrics of configured hooks. Designed to be integrated
//! into the main `jcode` CLI dispatch (e.g. `jcode hooks list`).
//!
//! # Subcommands
//!
//! | Command                        | Description                                      |
//! |---------------------------------|--------------------------------------------------|
//! | `hooks list`                    | List all configured hooks across all events       |
//! | `hooks list --event <EVENT>`    | List hooks for a specific event                   |
//! | `hooks enable <EVENT> <INDEX>`  | Enable a hook handler by event and index          |
//! | `hooks disable <EVENT> <INDEX>` | Disable a hook handler by event and index         |
//! | `hooks test <EVENT>`            | Dry-run all hooks for an event                    |
//! | `hooks test <EVENT> --execute`  | Actually execute hooks for an event               |
//! | `hooks metrics`                 | Show execution metrics for all hooks              |
//! | `hooks metrics --json`          | Emit metrics as JSON                              |

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use serde::Serialize;

use crate::config::{load_hooks_config, HookEvent, HookHandlerConfig, HookSettings, HooksConfig};
use crate::dispatch::{dispatch_hooks, ClassifiedOutcome, DispatchConfig};
use crate::types::HookInput;

// ===========================================================================
// Error type
// ===========================================================================

/// Errors returned by CLI operations.
#[derive(Debug)]
pub enum CliError {
    /// The user-supplied event name did not match any known variant.
    UnknownEvent(String),
    /// No hooks are configured for the given event.
    NoHooksForEvent(String),
    /// The handler index is out of range for the event's handler list.
    IndexOutOfRange {
        index: usize,
        event: String,
        count: usize,
    },
    /// An I/O or serialization error occurred.
    Io(std::io::Error),
    /// A TOML serialization error occurred.
    TomlSer(toml::ser::Error),
    /// A JSON serialization error occurred.
    Json(serde_json::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::UnknownEvent(name) => write!(
                f,
                "Unknown event '{}'. Use `jcode hooks list` to see valid event names.",
                name
            ),
            CliError::NoHooksForEvent(name) => {
                write!(f, "No hooks configured for event '{}'.", name)
            }
            CliError::IndexOutOfRange {
                index,
                event,
                count,
            } => write!(
                f,
                "Index {} out of range for event '{}' (has {} handler(s), valid range: 0..{}).",
                index,
                event,
                count,
                count.saturating_sub(1)
            ),
            CliError::Io(e) => write!(f, "{}", e),
            CliError::TomlSer(e) => write!(f, "TOML serialization error: {}", e),
            CliError::Json(e) => write!(f, "JSON serialization error: {}", e),
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Io(e)
    }
}

impl From<toml::ser::Error> for CliError {
    fn from(e: toml::ser::Error) -> Self {
        CliError::TomlSer(e)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(e: serde_json::Error) -> Self {
        CliError::Json(e)
    }
}

// ===========================================================================
// Public API -- called from the main CLI dispatcher
// ===========================================================================

/// Entry point for all `jcode hooks` subcommands.
///
/// Call this from the main CLI's dispatch function when the user runs
/// `jcode hooks <subcommand>`.
pub async fn run_hooks_command(subcmd: HooksSubcommand) -> Result<(), CliError> {
    match subcmd {
        HooksSubcommand::List { event, json } => run_hooks_list(event, json),
        HooksSubcommand::Enable { event, index } => run_hooks_enable(&event, index),
        HooksSubcommand::Disable { event, index } => run_hooks_disable(&event, index),
        HooksSubcommand::Test {
            event,
            execute,
            json,
        } => run_hooks_test(&event, execute, json).await,
        HooksSubcommand::Metrics { json } => run_hooks_metrics(json),
    }
}

/// Subcommands for `jcode hooks`.
#[derive(Debug, Clone)]
pub enum HooksSubcommand {
    /// List all configured hooks, optionally filtered by event name.
    List {
        /// If set, only show hooks for this event.
        event: Option<String>,
        /// Emit JSON instead of human-readable output.
        json: bool,
    },
    /// Enable a specific hook handler.
    Enable {
        /// The event name (e.g. "PreToolUse").
        event: String,
        /// The 0-based index of the handler within that event's handler list.
        index: usize,
    },
    /// Disable a specific hook handler.
    Disable {
        /// The event name (e.g. "PreToolUse").
        event: String,
        /// The 0-based index of the handler within that event's handler list.
        index: usize,
    },
    /// Dry-run or actually execute hooks for a given event to verify behavior.
    Test {
        /// The event to test (e.g. "PreToolUse", "SessionStart").
        event: String,
        /// If set, actually execute the hooks instead of dry-run.
        execute: bool,
        /// Emit JSON instead of human-readable output.
        json: bool,
    },
    /// Show execution metrics and configuration summary.
    Metrics {
        /// Emit JSON instead of human-readable output.
        json: bool,
    },
}

// ===========================================================================
// hooks list
// ===========================================================================

/// List configured hooks, optionally filtered by event.
fn run_hooks_list(event_filter: Option<String>, json: bool) -> Result<(), CliError> {
    let config = load_hooks_config();

    if config.is_empty() {
        if json {
            let empty = HooksListOutput {
                settings: config.settings.clone(),
                events: Vec::new(),
                total_handlers: 0,
            };
            println!("{}", serde_json::to_string_pretty(&empty)?);
        } else {
            println!("No hooks configured.");
            println!();
            println!("Config sources (checked in order, later overrides):");
            print_config_sources();
        }
        return Ok(());
    }

    let entries = build_list_entries(&config, event_filter.as_deref());

    if json {
        let output = HooksListOutput {
            settings: config.settings.clone(),
            events: entries.clone(),
            total_handlers: entries.iter().map(|e| e.handlers.len()).sum(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_hooks_table(&config.settings, &entries);
    }

    Ok(())
}

/// Build a list of event entries from the config, optionally filtering by event name.
fn build_list_entries(config: &HooksConfig, event_filter: Option<&str>) -> Vec<HooksEventEntry> {
    let mut entries: Vec<HooksEventEntry> = Vec::new();

    // Sort event names for deterministic output.
    let mut event_names: Vec<&String> = config.events.keys().collect();
    event_names.sort();

    for event_name in event_names {
        if let Some(filter) = event_filter {
            // Normalize both sides for case-insensitive matching.
            let normalized_filter: String = filter
                .chars()
                .filter(|c| *c != '_' && *c != '-' && *c != ' ')
                .collect::<String>()
                .to_ascii_lowercase();
            let normalized_event: String = event_name
                .chars()
                .filter(|c| *c != '_' && *c != '-' && *c != ' ')
                .collect::<String>()
                .to_ascii_lowercase();
            if normalized_filter != normalized_event {
                continue;
            }
        }

        let handlers = &config.events[event_name];
        if handlers.is_empty() {
            continue;
        }

        let handler_entries: Vec<HandlerEntry> = handlers
            .iter()
            .enumerate()
            .map(|(i, h)| handler_to_entry(i, h))
            .collect();

        entries.push(HooksEventEntry {
            event: event_name.clone(),
            handler_count: handler_entries.len(),
            blocking: HookEvent::parse(event_name)
                .map(|e| e.is_blocking())
                .unwrap_or(false),
            handlers: handler_entries,
        });
    }

    entries
}

/// Convert a handler config into a serializable entry for display.
fn handler_to_entry(index: usize, handler: &HookHandlerConfig) -> HandlerEntry {
    match handler {
        HookHandlerConfig::Command(cmd) => HandlerEntry {
            index,
            handler_type: "command".to_string(),
            label: cmd.command.clone(),
            enabled: cmd.enabled,
            timeout_secs: cmd.timeout_secs,
            matcher: cmd.matcher.as_ref().map(|m| format!("{:?}", m)),
            condition: cmd.if_.clone(),
        },
        HookHandlerConfig::Http(http) => HandlerEntry {
            index,
            handler_type: "http".to_string(),
            label: format!("{} {}", http.method, http.url),
            enabled: http.enabled,
            timeout_secs: http.timeout_secs,
            matcher: http.matcher.as_ref().map(|m| format!("{:?}", m)),
            condition: http.if_.clone(),
        },
        HookHandlerConfig::Agent(agent) => HandlerEntry {
            index,
            handler_type: "agent".to_string(),
            label: agent.agent_id.clone(),
            enabled: agent.enabled,
            timeout_secs: Some(agent.timeout_secs),
            matcher: agent.matcher.as_ref().map(|m| format!("{:?}", m)),
            condition: agent.if_.clone(),
        },
        HookHandlerConfig::Plugin(plugin) => HandlerEntry {
            index,
            handler_type: "plugin".to_string(),
            label: plugin.path.clone(),
            enabled: plugin.enabled,
            timeout_secs: Some(plugin.timeout_secs),
            matcher: plugin.matcher.as_ref().map(|m| format!("{:?}", m)),
            condition: plugin.if_.clone(),
        },
    }
}

// ===========================================================================
// hooks enable / disable
// ===========================================================================

/// Enable a hook handler by event name and index.
///
/// Reads the config, modifies the enabled flag on the matching handler,
/// and writes it back to the project-level `.jcode/hooks.toml`.
fn run_hooks_enable(event_name: &str, index: usize) -> Result<(), CliError> {
    set_handler_enabled(event_name, index, true)
}

/// Disable a hook handler by event name and index.
fn run_hooks_disable(event_name: &str, index: usize) -> Result<(), CliError> {
    set_handler_enabled(event_name, index, false)
}

/// Set the `enabled` flag on a specific handler and write back to project config.
fn set_handler_enabled(event_name: &str, index: usize, enabled: bool) -> Result<(), CliError> {
    // Parse the event to validate it.
    let event = HookEvent::parse(event_name)
        .ok_or_else(|| CliError::UnknownEvent(event_name.to_string()))?;

    let config = load_hooks_config();

    let handlers = config
        .events
        .get(event.display_name())
        .ok_or_else(|| CliError::NoHooksForEvent(event.display_name().to_string()))?;

    if index >= handlers.len() {
        return Err(CliError::IndexOutOfRange {
            index,
            event: event.display_name().to_string(),
            count: handlers.len(),
        });
    }

    // Update the enabled flag in the loaded config.
    let mut updated_config = config;
    let handlers = updated_config.events.get_mut(event.display_name()).unwrap();
    set_handler_enabled_flag(&mut handlers[index], enabled);

    // Write back to the project-level config.
    let project_config_path = project_hooks_config_path()?;
    write_hooks_config(&project_config_path, &updated_config)?;

    let action = if enabled { "Enabled" } else { "Disabled" };
    println!(
        "{} handler #{} for event '{}'.",
        action,
        index,
        event.display_name()
    );
    println!("Config written to: {}", project_config_path.display());

    Ok(())
}

/// Set the `enabled` field on any handler variant.
fn set_handler_enabled_flag(handler: &mut HookHandlerConfig, enabled: bool) {
    match handler {
        HookHandlerConfig::Command(cmd) => cmd.enabled = enabled,
        HookHandlerConfig::Http(http) => http.enabled = enabled,
        HookHandlerConfig::Agent(agent) => agent.enabled = enabled,
        HookHandlerConfig::Plugin(plugin) => plugin.enabled = enabled,
    }
}

/// Get the `enabled` field from any handler variant.
fn get_handler_enabled(handler: &HookHandlerConfig) -> bool {
    match handler {
        HookHandlerConfig::Command(cmd) => cmd.enabled,
        HookHandlerConfig::Http(http) => http.enabled,
        HookHandlerConfig::Agent(agent) => agent.enabled,
        HookHandlerConfig::Plugin(plugin) => plugin.enabled,
    }
}

// ===========================================================================
// hooks test
// ===========================================================================

/// Test hooks for a given event.
///
/// In dry-run mode (default), resolves matching handlers and reports which
/// would fire without actually executing them. With `--execute`, runs the
/// handlers for real using the dispatch engine.
async fn run_hooks_test(event_name: &str, execute: bool, json: bool) -> Result<(), CliError> {
    let event = HookEvent::parse(event_name)
        .ok_or_else(|| CliError::UnknownEvent(event_name.to_string()))?;

    let config = load_hooks_config();

    if config.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&HooksTestOutput {
                    event: event.display_name().to_string(),
                    mode: if execute { "execute" } else { "dry-run" }.to_string(),
                    handlers_resolved: 0,
                    handlers_enabled: 0,
                    results: Vec::new(),
                    stats: None,
                })?
            );
        } else {
            println!("No hooks configured. Nothing to test.");
        }
        return Ok(());
    }

    let handlers = config
        .events
        .get(event.display_name())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);

    let enabled_handlers: Vec<&HookHandlerConfig> =
        handlers.iter().filter(|h| get_handler_enabled(h)).collect();

    if enabled_handlers.is_empty() {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&HooksTestOutput {
                    event: event.display_name().to_string(),
                    mode: if execute { "execute" } else { "dry-run" }.to_string(),
                    handlers_resolved: handlers.len(),
                    handlers_enabled: 0,
                    results: Vec::new(),
                    stats: None,
                })?
            );
        } else {
            println!(
                "Event '{}' has {} handler(s), but none are enabled.",
                event.display_name(),
                handlers.len()
            );
        }
        return Ok(());
    }

    // Build a synthetic HookInput for the test.
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/tmp".to_string());
    let input = HookInput {
        session_id: "hooks-test-session".to_string(),
        cwd,
        hook_event_name: event.display_name().to_string(),
        ..Default::default()
    };

    let dispatch_config = DispatchConfig {
        dry_run: !execute,
        ..DispatchConfig::from_settings(&config.settings)
    };

    println!(
        "Testing {} enabled handler(s) for event '{}' (mode: {})...",
        enabled_handlers.len(),
        event.display_name(),
        if execute { "execute" } else { "dry-run" }
    );
    println!();

    let stats = dispatch_hooks(&event, &input, &enabled_handlers, &dispatch_config).await;

    if json {
        let results: Vec<HooksTestResultEntry> = stats
            .results
            .iter()
            .map(|r| HooksTestResultEntry {
                handler: r.handler_label.clone(),
                outcome: format!("{:?}", r.outcome),
                duration_ms: r.duration.as_millis() as u64,
            })
            .collect();

        let output = HooksTestOutput {
            event: event.display_name().to_string(),
            mode: if execute { "execute" } else { "dry-run" }.to_string(),
            handlers_resolved: handlers.len(),
            handlers_enabled: enabled_handlers.len(),
            results,
            stats: Some(HooksTestStatsSummary {
                total_dispatched: stats.total_dispatched,
                completed: stats.completed,
                failed: stats.failed,
                allowed: stats.allowed,
                denied: stats.denied,
                asked: stats.asked,
                total_duration_ms: stats.total_duration.as_millis() as u64,
            }),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for result in &stats.results {
            let status = match &result.outcome {
                ClassifiedOutcome::Allow => "\x1b[32mALLOW\x1b[0m",
                ClassifiedOutcome::Ask { .. } => "\x1b[33mASK\x1b[0m",
                ClassifiedOutcome::Deny { .. } => "\x1b[31mDENY\x1b[0m",
                ClassifiedOutcome::Failed { .. } => "\x1b[31mFAILED\x1b[0m",
            };
            println!(
                "  [{:>7}] {} ({}ms)",
                status,
                result.handler_label,
                result.duration.as_millis()
            );
            if let ClassifiedOutcome::Ask { reason } = &result.outcome {
                if !reason.is_empty() {
                    println!("           reason: {}", reason);
                }
            }
            if let ClassifiedOutcome::Deny { reason } = &result.outcome {
                if !reason.is_empty() {
                    println!("           reason: {}", reason);
                }
            }
            if let ClassifiedOutcome::Failed { error } = &result.outcome {
                println!("           error: {}", error);
            }
        }
        println!();
        println!(
            "Summary: {} dispatched, {} completed, {} failed ({}ms total)",
            stats.total_dispatched,
            stats.completed,
            stats.failed,
            stats.total_duration.as_millis()
        );
        if !execute {
            println!();
            println!("Note: dry-run mode -- no hooks were actually executed.");
            println!("      Use --execute to run hooks for real.");
        }
    }

    Ok(())
}

// ===========================================================================
// hooks metrics
// ===========================================================================

/// Show hooks configuration summary and (future) execution metrics.
fn run_hooks_metrics(json: bool) -> Result<(), CliError> {
    let config = load_hooks_config();

    let total_handlers: usize = config.events.values().map(|v| v.len()).sum();
    let enabled_handlers: usize = config
        .events
        .values()
        .flat_map(|v| v.iter())
        .filter(|h| get_handler_enabled(h))
        .count();
    let disabled_handlers = total_handlers - enabled_handlers;

    let handler_type_counts = count_handler_types(&config);

    let mut event_summaries: Vec<EventMetricsSummary> = Vec::new();
    let mut event_names: Vec<&String> = config.events.keys().collect();
    event_names.sort();

    for event_name in &event_names {
        let handlers = &config.events[*event_name];
        if handlers.is_empty() {
            continue;
        }
        let blocking = HookEvent::parse(event_name)
            .map(|e| e.is_blocking())
            .unwrap_or(false);
        let enabled_count = handlers.iter().filter(|h| get_handler_enabled(h)).count();
        event_summaries.push(EventMetricsSummary {
            event: (*event_name).clone(),
            total_handlers: handlers.len(),
            enabled_handlers: enabled_count,
            blocking,
        });
    }

    if json {
        let output = HooksMetricsOutput {
            settings: config.settings.clone(),
            total_events: event_summaries.len(),
            total_handlers,
            enabled_handlers,
            disabled_handlers,
            handler_type_counts: handler_type_counts.clone(),
            events: event_summaries,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Hooks Configuration Summary");
        println!("===========================");
        println!();
        println!("Settings:");
        println!("  Default timeout:    {}s", config.settings.timeout_secs);
        println!("  Max concurrency:    {}", config.settings.max_concurrency);
        println!("  Dry-run mode:       {}", config.settings.dry_run);
        println!("  Fail-closed:        {}", config.settings.fail_closed);
        println!();
        println!("Total events with hooks: {}", event_summaries.len());
        println!(
            "Total handlers:          {} ({} enabled, {} disabled)",
            total_handlers, enabled_handlers, disabled_handlers
        );
        println!();

        if !handler_type_counts.is_empty() {
            println!("Handler types:");
            let mut types: Vec<(&String, &usize)> = handler_type_counts.iter().collect();
            types.sort_by_key(|(k, _)| k.as_str());
            for (htype, count) in &types {
                println!("  {:10} {}", htype, count);
            }
            println!();
        }

        if !event_summaries.is_empty() {
            println!("Per-event breakdown:");
            println!(
                "  {:<30} {:>8} {:>8} {:>8}",
                "EVENT", "HANDLERS", "ENABLED", "BLOCKING"
            );
            println!("  {:-<30} {:-<8} {:-<8} {:-<8}", "", "", "", "");
            for entry in &event_summaries {
                println!(
                    "  {:<30} {:>8} {:>8} {:>8}",
                    entry.event,
                    entry.total_handlers,
                    entry.enabled_handlers,
                    if entry.blocking { "yes" } else { "no" }
                );
            }
        }

        if config.is_empty() {
            println!();
            println!("No hooks configured.");
            print_config_sources();
        }
    }

    Ok(())
}

/// Count handlers by type across all events.
fn count_handler_types(config: &HooksConfig) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for handler in config.events.values().flat_map(|v| v.iter()) {
        let key = match handler {
            HookHandlerConfig::Command(_) => "command",
            HookHandlerConfig::Http(_) => "http",
            HookHandlerConfig::Agent(_) => "agent",
            HookHandlerConfig::Plugin(_) => "plugin",
        };
        *counts.entry(key.to_string()).or_insert(0) += 1;
    }
    counts
}

// ===========================================================================
// Config file I/O
// ===========================================================================

/// Path to the project-level hooks config file: `<cwd>/.jcode/hooks.toml`.
fn project_hooks_config_path() -> Result<PathBuf, CliError> {
    let cwd = std::env::current_dir()?;
    Ok(cwd.join(".jcode").join("hooks.toml"))
}

/// Serialize a [`HooksConfig`] to TOML and write it to the given path.
///
/// Creates parent directories if they do not exist.
fn write_hooks_config(path: &PathBuf, config: &HooksConfig) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let toml_string = toml::to_string_pretty(config)?;
    std::fs::write(path, &toml_string)?;

    Ok(())
}

/// Print the list of config source paths (for help output when no config exists).
fn print_config_sources() {
    if let Some(home) = dirs::home_dir() {
        let user_path = home.join(".jcode").join("hooks.toml");
        println!("  User:    {}", user_path.display());
    }
    if let Ok(cwd) = std::env::current_dir() {
        let project_path = cwd.join(".jcode").join("hooks.toml");
        println!("  Project: {}", project_path.display());
    }
    println!("  Env:     $JCODE_HOOKS_CONFIG");
}

// ===========================================================================
// Human-readable output helpers
// ===========================================================================

/// Print a human-readable table of hooks grouped by event.
fn print_hooks_table(settings: &HookSettings, entries: &[HooksEventEntry]) {
    println!("Hooks Configuration");
    println!("===================");
    println!(
        "  timeout: {}s | concurrency: {} | dry_run: {} | fail_closed: {}",
        settings.timeout_secs, settings.max_concurrency, settings.dry_run, settings.fail_closed,
    );
    println!();

    let total: usize = entries.iter().map(|e| e.handlers.len()).sum();
    println!(
        "{} event(s) with {} total handler(s):",
        entries.len(),
        total
    );
    println!();

    for entry in entries {
        let blocking_tag = if entry.blocking { " [blocking]" } else { "" };
        println!(
            "{} ({} handler(s)){}",
            entry.event, entry.handler_count, blocking_tag
        );
        for h in &entry.handlers {
            let status = if h.enabled {
                "\x1b[32mON\x1b[0m"
            } else {
                "\x1b[31mOFF\x1b[0m"
            };
            let timeout_str = h
                .timeout_secs
                .map(|t| format!("{}s", t))
                .unwrap_or_else(|| "default".to_string());
            let matcher_str = h
                .matcher
                .as_deref()
                .map(|m| format!(" match={}", m))
                .unwrap_or_default();
            let condition_str = h
                .condition
                .as_deref()
                .map(|c| format!(" if={}", c))
                .unwrap_or_default();

            println!(
                "  [{}] #{} {:<8} {} (timeout={}{}{})",
                status, h.index, h.handler_type, h.label, timeout_str, matcher_str, condition_str,
            );
        }
        println!();
    }
}

// ===========================================================================
// JSON output types
// ===========================================================================

#[derive(Debug, Clone, Serialize)]
struct HooksListOutput {
    settings: HookSettings,
    events: Vec<HooksEventEntry>,
    total_handlers: usize,
}

#[derive(Debug, Clone, Serialize)]
struct HooksEventEntry {
    event: String,
    handler_count: usize,
    blocking: bool,
    handlers: Vec<HandlerEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct HandlerEntry {
    index: usize,
    #[serde(rename = "type")]
    handler_type: String,
    label: String,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matcher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    condition: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct HooksTestOutput {
    event: String,
    mode: String,
    handlers_resolved: usize,
    handlers_enabled: usize,
    results: Vec<HooksTestResultEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<HooksTestStatsSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct HooksTestResultEntry {
    handler: String,
    outcome: String,
    duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct HooksTestStatsSummary {
    total_dispatched: u64,
    completed: u64,
    failed: u64,
    allowed: u64,
    denied: u64,
    asked: u64,
    total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
struct HooksMetricsOutput {
    settings: HookSettings,
    total_events: usize,
    total_handlers: usize,
    enabled_handlers: usize,
    disabled_handlers: usize,
    handler_type_counts: HashMap<String, usize>,
    events: Vec<EventMetricsSummary>,
}

#[derive(Debug, Clone, Serialize)]
struct EventMetricsSummary {
    event: String,
    total_handlers: usize,
    enabled_handlers: usize,
    blocking: bool,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommandHandlerConfig, HttpHandlerConfig};

    fn sample_config() -> HooksConfig {
        let mut config = HooksConfig::default();
        config.settings.timeout_secs = 15;
        config.settings.max_concurrency = 5;

        config
            .events
            .entry("PreToolUse".to_string())
            .or_default()
            .push(HookHandlerConfig::Command(CommandHandlerConfig {
                command: "check.sh".to_string(),
                enabled: true,
                timeout_secs: Some(5),
                ..Default::default()
            }));
        config
            .events
            .entry("PreToolUse".to_string())
            .or_default()
            .push(HookHandlerConfig::Command(CommandHandlerConfig {
                command: "lint.sh".to_string(),
                enabled: false,
                ..Default::default()
            }));
        config
            .events
            .entry("SessionEnd".to_string())
            .or_default()
            .push(HookHandlerConfig::Http(HttpHandlerConfig {
                url: "http://localhost:9090/hook".to_string(),
                enabled: true,
                ..Default::default()
            }));

        config
    }

    #[test]
    fn build_list_entries_all() {
        let config = sample_config();
        let entries = build_list_entries(&config, None);
        assert_eq!(entries.len(), 2);
        let event_names: Vec<&str> = entries.iter().map(|e| e.event.as_str()).collect();
        assert!(event_names.contains(&"PreToolUse"));
        assert!(event_names.contains(&"SessionEnd"));
    }

    #[test]
    fn build_list_entries_filtered() {
        let config = sample_config();
        let entries = build_list_entries(&config, Some("pretooluse"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event, "PreToolUse");
        assert_eq!(entries[0].handlers.len(), 2);
    }

    #[test]
    fn build_list_entries_filter_case_insensitive() {
        let config = sample_config();
        let entries = build_list_entries(&config, Some("pre-tool-use"));
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn build_list_entries_filter_no_match() {
        let config = sample_config();
        let entries = build_list_entries(&config, Some("NonExistent"));
        assert!(entries.is_empty());
    }

    #[test]
    fn handler_to_entry_command() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            command: "test.sh".to_string(),
            enabled: true,
            timeout_secs: Some(10),
            ..Default::default()
        });
        let entry = handler_to_entry(3, &handler);
        assert_eq!(entry.index, 3);
        assert_eq!(entry.handler_type, "command");
        assert_eq!(entry.label, "test.sh");
        assert!(entry.enabled);
        assert_eq!(entry.timeout_secs, Some(10));
    }

    #[test]
    fn handler_to_entry_http() {
        let handler = HookHandlerConfig::Http(HttpHandlerConfig {
            url: "http://example.com".to_string(),
            method: "PUT".to_string(),
            enabled: false,
            ..Default::default()
        });
        let entry = handler_to_entry(0, &handler);
        assert_eq!(entry.handler_type, "http");
        assert_eq!(entry.label, "PUT http://example.com");
        assert!(!entry.enabled);
    }

    #[test]
    fn set_and_get_handler_enabled() {
        let mut handler = HookHandlerConfig::Command(CommandHandlerConfig {
            enabled: true,
            ..Default::default()
        });
        assert!(get_handler_enabled(&handler));
        set_handler_enabled_flag(&mut handler, false);
        assert!(!get_handler_enabled(&handler));
        set_handler_enabled_flag(&mut handler, true);
        assert!(get_handler_enabled(&handler));
    }

    #[test]
    fn count_handler_types_mixed() {
        let config = sample_config();
        let counts = count_handler_types(&config);
        assert_eq!(counts.get("command"), Some(&2));
        assert_eq!(counts.get("http"), Some(&1));
        assert_eq!(counts.get("agent"), None);
    }

    #[test]
    fn empty_config_is_handled() {
        let config = HooksConfig::default();
        let entries = build_list_entries(&config, None);
        assert!(entries.is_empty());
    }

    #[test]
    fn event_entry_blocking_flag() {
        let config = sample_config();
        let entries = build_list_entries(&config, None);
        let pre_tool = entries.iter().find(|e| e.event == "PreToolUse").unwrap();
        assert!(pre_tool.blocking, "PreToolUse should be blocking");

        let session_end = entries.iter().find(|e| e.event == "SessionEnd").unwrap();
        assert!(!session_end.blocking, "SessionEnd should not be blocking");
    }

    #[test]
    fn cli_error_display() {
        let err = CliError::UnknownEvent("FooBar".to_string());
        assert!(format!("{}", err).contains("FooBar"));

        let err = CliError::IndexOutOfRange {
            index: 5,
            event: "PreToolUse".to_string(),
            count: 3,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("5"));
        assert!(msg.contains("PreToolUse"));
        assert!(msg.contains("3"));
    }

    #[test]
    fn cli_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: CliError = io_err.into();
        assert!(format!("{}", err).contains("file missing"));
    }
}
