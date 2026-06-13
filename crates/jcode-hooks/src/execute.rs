//! Hook v2 execution engine.
//!
//! This module provides the execution layer for the four handler types defined
//! in the hook configuration:
//!
//! - **Command** -- spawns a shell process, feeds `HookInput` as JSON on stdin,
//!   reads `HookOutput` from stdout, and interprets the exit code.
//! - **HTTP** -- sends `HookInput` as a JSON POST (or other method) to a URL
//!   and expects a `HookOutput` JSON response body.
//! - **Agent** -- placeholder for dispatching to an inline jcode sub-agent.
//! - **Plugin** -- runs an external executable with the same stdin/stdout
//!   protocol as command hooks.
//!
//! # Exit-code protocol (command & plugin hooks)
//!
//! | Exit code | Meaning                      |
//! |-----------|------------------------------|
//! | 0         | Continue (success)           |
//! | 1         | Failure (non-blocking error) |
//! | 2         | Block / deny the operation   |
//!
//! Any other exit code is treated as a failure.
//!
//! # Environment variable expansion
//!
//! Values in handler config (commands, URLs, header values, plugin args) may
//! contain `${VAR}` placeholders that are expanded at execution time from the
//! current process environment.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use regex::Regex;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::{
    AgentHandlerConfig, CommandHandlerConfig, HookHandlerConfig, HttpHandlerConfig,
    PluginHandlerConfig,
};
use crate::types::{HookInput, HookOutput, HookResult};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during hook execution.
///
/// These are distinct from [`HookResult::Failed`] which represents a hook
/// that ran but returned a failure signal.  `ExecuteError` represents an
/// infrastructure-level problem that prevented the hook from running at all
/// (or from producing a valid result).
#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    /// The hook command could not be spawned (not found, permission denied, etc.).
    #[error("failed to spawn hook command: {0}")]
    SpawnFailed(String),

    /// The hook process was killed by a signal.
    #[error("hook process killed by signal: {signal}")]
    ProcessKilled { signal: String },

    /// I/O error communicating with the hook process (stdin write, stdout read).
    #[error("I/O error communicating with hook process: {0}")]
    IoError(String),

    /// The hook's stdout was not valid JSON or did not match the `HookOutput` schema.
    #[error("hook returned invalid JSON on stdout: {0}")]
    InvalidOutput(String),

    /// An HTTP-level error occurred (network failure, non-2xx status, etc.).
    #[error("HTTP hook error: {0}")]
    HttpError(String),

    /// The hook timed out (caller wraps with tokio::time::timeout, but this
    /// variant exists for the HTTP path which handles timeouts internally).
    #[error("hook timed out after {0}s")]
    Timeout(u64),

    /// The agent handler is not yet implemented.
    #[error("agent handler not yet implemented")]
    AgentNotImplemented,

    /// Generic catch-all for unexpected failures.
    #[error("unexpected error: {0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// execute_hook  --  top-level dispatcher
// ---------------------------------------------------------------------------

/// Execute a single hook handler and return the [`HookResult`].
///
/// Dispatches to the type-specific executor based on the handler variant.
/// The `timeout_secs` parameter is the effective timeout (per-handler override
/// or global default).
///
/// This function does **not** apply its own timeout wrapper -- the caller
/// (typically the dispatch engine) is expected to wrap the call in
/// `tokio::time::timeout`.
pub async fn execute_hook(
    handler: &HookHandlerConfig,
    input: &HookInput,
    timeout_secs: u64,
) -> Result<HookResult, ExecuteError> {
    match handler {
        HookHandlerConfig::Command(cmd) => execute_command_hook(cmd, input, timeout_secs).await,
        HookHandlerConfig::Http(http) => execute_http_hook(http, input, timeout_secs).await,
        HookHandlerConfig::Agent(agent) => execute_agent_hook(agent, input).await,
        HookHandlerConfig::Plugin(plugin) => execute_plugin_hook(plugin, input, timeout_secs).await,
    }
}

// ---------------------------------------------------------------------------
// execute_single_hook  --  entry point used by the dispatch engine
// ---------------------------------------------------------------------------

/// Execute a single hook handler, returning the [`HookResult`].
///
/// This is the primary entry point used by the dispatch engine. It validates
/// that the handler is enabled before executing, and delegates to
/// [`execute_hook`].
///
/// Disabled handlers are treated as a no-op `Continue` result.
pub async fn execute_single_hook(
    handler: &HookHandlerConfig,
    input: &HookInput,
    timeout_secs: u64,
) -> Result<HookResult, ExecuteError> {
    // Check if the handler is enabled.
    if !is_handler_enabled(handler) {
        return Ok(HookResult::Continue(HookOutput::continue_()));
    }

    execute_hook(handler, input, timeout_secs).await
}

/// Return `true` if the handler's `enabled` field is `true`.
fn is_handler_enabled(handler: &HookHandlerConfig) -> bool {
    match handler {
        HookHandlerConfig::Command(cmd) => cmd.enabled,
        HookHandlerConfig::Http(http) => http.enabled,
        HookHandlerConfig::Agent(agent) => agent.enabled,
        HookHandlerConfig::Plugin(plugin) => plugin.enabled,
    }
}

// ---------------------------------------------------------------------------
// execute_command_hook
// ---------------------------------------------------------------------------

/// Execute a command-type hook handler.
///
/// # Protocol
///
/// 1. The `HookInput` is serialized as JSON and piped to the command's stdin.
/// 2. The command's stdout is captured and deserialized as `HookOutput`.
/// 3. The exit code determines the outcome:
///    - **0**: `HookResult::Continue` (with the parsed output)
///    - **1**: `HookResult::Failed` (the hook reported a failure)
///    - **2**: `HookResult::Blocked` (the hook wants to block the operation)
///    - **other**: `HookResult::Failed` (unexpected exit code)
///
/// If the command produces no stdout (empty), a default `HookOutput::continue_()`
/// is assumed.
///
/// # Environment
///
/// The child process inherits the current process environment, plus:
/// - Any variables from the handler's `env` field (after `${VAR}` expansion).
/// - `JCODE_HOOK_EVENT` set to the event name.
/// - `JCODE_HOOK_SESSION_ID` set to the session id.
/// - `JCODE_HOOK_CWD` set to the working directory.
///
/// # Working directory
///
/// If the handler config specifies a `cwd`, it is used as the child process
/// working directory.  Otherwise, the `cwd` from the `HookInput` is used.
pub async fn execute_command_hook(
    config: &CommandHandlerConfig,
    input: &HookInput,
    _timeout_secs: u64,
) -> Result<HookResult, ExecuteError> {
    let expanded_command = expand_env_var(&config.command);

    // Determine the working directory: handler override > input cwd > current dir.
    let working_dir = config
        .cwd
        .as_deref()
        .map(expand_env_var)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if input.cwd.is_empty() {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "/tmp".to_string())
            } else {
                input.cwd.clone()
            }
        });

    // Build environment variables.
    let env_vars = build_command_env(&config.env, input);

    // Spawn the child process via sh -c so that shell syntax works.
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&expanded_command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(&working_dir)
        .envs(&env_vars)
        .spawn()
        .map_err(|e| ExecuteError::SpawnFailed(format!("{}: {}", expanded_command, e)))?;

    // Write HookInput JSON to stdin.
    if let Some(mut stdin) = child.stdin.take() {
        let json = serde_json::to_vec(input)
            .map_err(|e| ExecuteError::IoError(format!("serialize input: {}", e)))?;
        stdin
            .write_all(&json)
            .await
            .map_err(|e| ExecuteError::IoError(format!("write stdin: {}", e)))?;
        // Close stdin so the child knows input is complete.
        drop(stdin);
    }

    // Wait for the child to exit and collect output.
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ExecuteError::IoError(format!("wait for child: {}", e)))?;

    let exit_code = output.status.code().unwrap_or_else(|| {
        // Process was killed by a signal on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(sig) = output.status.signal() {
                eprintln!(
                    "Hook command '{}' killed by signal {}",
                    expanded_command, sig
                );
            }
        }
        1 // Treat signal-killed as failure.
    });

    // Parse stdout as HookOutput (may be empty).
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let hook_output = if stdout_str.trim().is_empty() {
        HookOutput::continue_()
    } else {
        serde_json::from_str::<HookOutput>(stdout_str.trim()).unwrap_or_else(|e| {
            eprintln!(
                "Hook command '{}' produced invalid JSON on stdout: {} (raw: {:?})",
                expanded_command,
                e,
                stdout_str.trim()
            );
            // Treat invalid JSON as a simple continue (best-effort).
            HookOutput::continue_()
        })
    };

    // Log stderr if non-empty (for debugging).
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    if !stderr_str.trim().is_empty() {
        eprintln!(
            "Hook command '{}' stderr: {}",
            expanded_command,
            stderr_str.trim()
        );
    }

    // Interpret exit code per protocol.
    interpret_exit_code(exit_code, hook_output, &expanded_command)
}

/// Build the full environment for a command hook child process.
///
/// Starts with the current process environment, overlays the handler's `env`
/// (with `${VAR}` expansion), and adds the standard `JCODE_HOOK_*` vars.
///
/// # Backward-compat env vars (v1→v2 migration)
///
/// In addition to the three v2-standard vars, we set several env vars that
/// v1 hooks relied on, so existing scripts continue to work without changes:
///
/// - `JCODE_HOOKS_DISABLED=1` — recursion guard (v1 also set this).
/// - `JCODE_HOOK_TOOL_NAME` — name of the tool being executed (if applicable).
/// - `JCODE_HOOK_TOOL_INPUT` — JSON tool input (truncated to 16 KB).
fn build_command_env(
    handler_env: &HashMap<String, String>,
    input: &HookInput,
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();

    // Handler-specific env (expanded).
    for (key, value) in handler_env {
        env.insert(key.clone(), expand_env_var(value));
    }

    // Recursion guard — prevents hook commands from re-entering the hook
    // system (v1 compat). A hook that spawns jcode will see this env var
    // and skip hook dispatch.
    env.insert("JCODE_HOOKS_DISABLED".to_string(), "1".to_string());

    // Standard hook env vars.
    env.insert(
        "JCODE_HOOK_EVENT".to_string(),
        input.hook_event_name.clone(),
    );
    env.insert(
        "JCODE_HOOK_SESSION_ID".to_string(),
        input.session_id.clone(),
    );
    env.insert("JCODE_HOOK_CWD".to_string(), input.cwd.clone());

    // Backward-compat env vars — v1 hooks used these for decision-making.
    if let Some(tool_name) = &input.tool_name {
        env.insert("JCODE_HOOK_TOOL_NAME".to_string(), tool_name.clone());
    }
    if let Some(tool_input) = &input.tool_input {
        // Truncate to 16 KB to match v1's TOOL_INPUT_ENV_LIMIT.
        let json_str = serde_json::to_string(tool_input).unwrap_or_default();
        const TOOL_INPUT_LIMIT: usize = 16 * 1024;
        let truncated: String = json_str.chars().take(TOOL_INPUT_LIMIT).collect();
        env.insert("JCODE_HOOK_TOOL_INPUT".to_string(), truncated);
    }

    env
}

/// Interpret a process exit code and `HookOutput` into a [`HookResult`].
///
/// Exit code mapping:
/// - 0 => Continue (use the provided output)
/// - 1 => Failed
/// - 2 => Blocked
/// - other => Failed
fn interpret_exit_code(
    exit_code: i32,
    output: HookOutput,
    command_label: &str,
) -> Result<HookResult, ExecuteError> {
    match exit_code {
        0 => Ok(HookResult::Continue(output)),
        1 => {
            let reason = output
                .stop_reason
                .clone()
                .or_else(|| output.reason.clone())
                .unwrap_or_else(|| format!("hook command '{}' exited with code 1", command_label));
            Ok(HookResult::Failed { error: reason })
        }
        2 => {
            let reason = output
                .stop_reason
                .clone()
                .or_else(|| output.reason.clone())
                .unwrap_or_else(|| {
                    format!("hook command '{}' blocked the operation", command_label)
                });
            Ok(HookResult::Blocked { reason, output })
        }
        other => {
            let reason = format!(
                "hook command '{}' exited with unexpected code {}",
                command_label, other
            );
            Ok(HookResult::Failed { error: reason })
        }
    }
}

// ---------------------------------------------------------------------------
// execute_http_hook
// ---------------------------------------------------------------------------

/// Execute an HTTP-type hook handler.
///
/// # Protocol
///
/// 1. The `HookInput` is serialized as JSON and sent as the request body
///    (unless the handler config specifies a static `body` override).
/// 2. The response body is deserialized as `HookOutput`.
/// 3. A 2xx status code with a valid `HookOutput` is treated as success.
/// 4. Non-2xx status codes are treated as failures.
/// 5. If the response body sets `continue_: false`, the result is `Blocked`.
///
/// # Headers
///
/// The handler's `headers` values support `${VAR}` environment variable
/// expansion.  A default `Content-Type: application/json` header is set
/// unless overridden.
///
/// # Timeout
///
/// The HTTP client timeout is set to `timeout_secs`.  If the request times
/// out, an `ExecuteError::Timeout` is returned.
pub async fn execute_http_hook(
    config: &HttpHandlerConfig,
    input: &HookInput,
    timeout_secs: u64,
) -> Result<HookResult, ExecuteError> {
    let url = expand_env_var(&config.url);
    let method = config.method.to_uppercase();

    // Build the HTTP client with timeout.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| ExecuteError::Other(format!("build HTTP client: {}", e)))?;

    // Build the request based on method.
    let mut request = match method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        other => {
            return Err(ExecuteError::Other(format!(
                "unsupported HTTP method: {}",
                other
            )));
        }
    };

    // Set headers (with env expansion).
    let mut has_content_type = false;
    for (key, value) in &config.headers {
        let expanded_value = expand_env_var(value);
        if key.to_lowercase() == "content-type" {
            has_content_type = true;
        }
        request = request.header(key.as_str(), expanded_value.as_str());
    }

    // Default Content-Type if not explicitly set.
    if !has_content_type {
        request = request.header("Content-Type", "application/json");
    }

    // Set body: static body override or serialized HookInput.
    let body_json = match &config.body {
        Some(static_body) => serde_json::to_vec(static_body)
            .map_err(|e| ExecuteError::Other(format!("serialize static body: {}", e)))?,
        None => serde_json::to_vec(input)
            .map_err(|e| ExecuteError::Other(format!("serialize hook input: {}", e)))?,
    };
    request = request.body(body_json);

    // Execute the request.
    let response = request.send().await.map_err(|e| {
        if e.is_timeout() {
            ExecuteError::Timeout(timeout_secs)
        } else {
            ExecuteError::HttpError(format!("request to {}: {}", url, e))
        }
    })?;

    let status = response.status();

    // Read response body.
    let body_bytes = response
        .bytes()
        .await
        .map_err(|e| ExecuteError::HttpError(format!("read response from {}: {}", url, e)))?;

    let body_str = String::from_utf8_lossy(&body_bytes);

    // Non-2xx => failure.
    if !status.is_success() {
        return Ok(HookResult::Failed {
            error: format!(
                "HTTP hook returned status {} from {}: {}",
                status.as_u16(),
                url,
                body_str.chars().take(200).collect::<String>()
            ),
        });
    }

    // Parse response as HookOutput.
    let hook_output: HookOutput = if body_str.trim().is_empty() {
        HookOutput::continue_()
    } else {
        serde_json::from_str(body_str.trim()).unwrap_or_else(|e| {
            eprintln!(
                "HTTP hook at '{}' returned invalid JSON: {} (raw: {:?})",
                url,
                e,
                body_str.trim()
            );
            HookOutput::continue_()
        })
    };

    // Interpret the output.
    if hook_output.continue_ {
        Ok(HookResult::Continue(hook_output))
    } else {
        let reason = hook_output
            .stop_reason
            .clone()
            .or_else(|| hook_output.reason.clone())
            .unwrap_or_else(|| format!("HTTP hook at '{}' blocked the operation", url));
        Ok(HookResult::Blocked {
            reason,
            output: hook_output,
        })
    }
}

// ---------------------------------------------------------------------------
// execute_agent_hook  --  placeholder
// ---------------------------------------------------------------------------

/// Execute an agent-type hook handler.
///
/// **Not yet implemented.** This is a placeholder for future support of
/// inline jcode sub-agent dispatch.  Currently returns
/// [`ExecuteError::AgentNotImplemented`].
///
/// When implemented, this function will:
/// 1. Resolve the agent by `agent_id` from jcode's agent registry.
/// 2. Construct a sub-agent invocation with the `HookInput` as context.
/// 3. Optionally override the system prompt if `config.system_prompt` is set.
/// 4. Wait for completion (if `config.wait_for_completion` is `true`).
/// 5. Parse the agent's response as `HookOutput`.
pub async fn execute_agent_hook(
    config: &AgentHandlerConfig,
    _input: &HookInput,
) -> Result<HookResult, ExecuteError> {
    eprintln!(
        "Agent hook handler '{}' is not yet implemented; skipping",
        config.agent_id
    );
    Err(ExecuteError::AgentNotImplemented)
}

// ---------------------------------------------------------------------------
// execute_plugin_hook
// ---------------------------------------------------------------------------

/// Execute a plugin-type hook handler.
///
/// Plugins are external executables that follow the same stdin/stdout protocol
/// as command hooks:
///
/// 1. `HookInput` JSON is piped to the plugin's stdin.
/// 2. `HookOutput` JSON is read from the plugin's stdout.
/// 3. The exit code is interpreted per the standard protocol (0/1/2).
///
/// Unlike command hooks, plugins are specified as a direct executable path
/// (not via `sh -c`), and receive CLI arguments from `config.args`.
///
/// # Environment
///
/// The plugin inherits the current process environment plus:
/// - `JCODE_HOOK_EVENT`
/// - `JCODE_HOOK_SESSION_ID`
/// - `JCODE_HOOK_CWD`
///
/// # Arguments
///
/// Each argument in `config.args` supports `${VAR}` expansion.
pub async fn execute_plugin_hook(
    config: &PluginHandlerConfig,
    input: &HookInput,
    _timeout_secs: u64,
) -> Result<HookResult, ExecuteError> {
    let plugin_path = expand_env_var(&config.path);

    // Expand args.
    let expanded_args: Vec<String> = config.args.iter().map(|a| expand_env_var(a)).collect();

    // Build environment.
    let mut env_vars: HashMap<String, String> = std::env::vars().collect();
    env_vars.insert(
        "JCODE_HOOK_EVENT".to_string(),
        input.hook_event_name.clone(),
    );
    env_vars.insert(
        "JCODE_HOOK_SESSION_ID".to_string(),
        input.session_id.clone(),
    );
    env_vars.insert("JCODE_HOOK_CWD".to_string(), input.cwd.clone());

    // Spawn the plugin process.
    let mut child = Command::new(&plugin_path)
        .args(&expanded_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(&input.cwd)
        .envs(&env_vars)
        .spawn()
        .map_err(|e| ExecuteError::SpawnFailed(format!("plugin '{}': {}", plugin_path, e)))?;

    // Write HookInput JSON to stdin.
    if let Some(mut stdin) = child.stdin.take() {
        let json = serde_json::to_vec(input)
            .map_err(|e| ExecuteError::IoError(format!("serialize input: {}", e)))?;
        stdin
            .write_all(&json)
            .await
            .map_err(|e| ExecuteError::IoError(format!("write stdin to plugin: {}", e)))?;
        drop(stdin);
    }

    // Wait for the plugin to exit.
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ExecuteError::IoError(format!("wait for plugin '{}': {}", plugin_path, e)))?;

    let exit_code = output.status.code().unwrap_or_else(|| {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(sig) = output.status.signal() {
                eprintln!("Plugin '{}' killed by signal {}", plugin_path, sig);
            }
        }
        1
    });

    // Parse stdout.
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let hook_output = if stdout_str.trim().is_empty() {
        HookOutput::continue_()
    } else {
        serde_json::from_str::<HookOutput>(stdout_str.trim()).unwrap_or_else(|e| {
            eprintln!(
                "Plugin '{}' produced invalid JSON on stdout: {} (raw: {:?})",
                plugin_path,
                e,
                stdout_str.trim()
            );
            HookOutput::continue_()
        })
    };

    // Log stderr.
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    if !stderr_str.trim().is_empty() {
        eprintln!("Plugin '{}' stderr: {}", plugin_path, stderr_str.trim());
    }

    interpret_exit_code(exit_code, hook_output, &format!("plugin:{}", plugin_path))
}

// ---------------------------------------------------------------------------
// expand_env_var
// ---------------------------------------------------------------------------

/// Expand `${VAR}` placeholders in a string with values from the current
/// process environment.
///
/// # Syntax
///
/// - `${VAR}` -- replaced with the value of environment variable `VAR`.
///   If `VAR` is not set, the placeholder is left as-is (literal `${VAR}`).
/// - `${VAR:-default}` -- replaced with the value of `VAR`, or `default`
///   if `VAR` is not set or is empty.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(expand_env_var("hello"), "hello");
/// // If HOME=/home/user:
/// assert_eq!(expand_env_var("${HOME}/bin"), "/home/user/bin");
/// assert_eq!(expand_env_var("${MISSING:-fallback}"), "fallback");
/// ```
///
/// # Safety
///
/// This function does **not** perform shell command substitution or any
/// form of code execution.  Only environment variable values are substituted.
pub fn expand_env_var(input: &str) -> String {
    // Fast path: no dollar sign means nothing to expand.
    if !input.contains('$') {
        return input.to_string();
    }

    let re =
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::(-)([^}]*))?\}").expect("valid env var regex");

    let mut result = String::with_capacity(input.len());
    let mut last_end = 0;

    for caps in re.captures_iter(input) {
        let full_match = caps.get(0).unwrap();
        let var_name = &caps[1];
        let has_default = caps.get(2).is_some();
        let default_value = caps.get(3).map(|m| m.as_str()).unwrap_or("");

        // Append text before this match.
        result.push_str(&input[last_end..full_match.start()]);

        // Look up the variable.
        match std::env::var(var_name) {
            Ok(val) if !val.is_empty() => {
                result.push_str(&val);
            }
            _ if has_default => {
                result.push_str(default_value);
            }
            _ => {
                // Variable not set and no default: keep the original placeholder.
                result.push_str(full_match.as_str());
            }
        }

        last_end = full_match.end();
    }

    // Append any remaining text after the last match.
    result.push_str(&input[last_end..]);

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommandHandlerConfig, HttpHandlerConfig, PluginHandlerConfig};
    use crate::types::HookInputBuilder;

    // -- expand_env_var --------------------------------------------------------

    #[test]
    fn expand_no_dollar() {
        assert_eq!(expand_env_var("hello world"), "hello world");
    }

    #[test]
    fn expand_empty_string() {
        assert_eq!(expand_env_var(""), "");
    }

    #[test]
    fn expand_existing_var() {
        // PATH is always set.
        let result = expand_env_var("${PATH}");
        assert!(!result.contains("${PATH}"));
        assert!(!result.is_empty());
    }

    #[test]
    fn expand_missing_var_no_default() {
        // A variable that is extremely unlikely to exist.
        let result = expand_env_var("${JCODE_HOOKS_TEST_VAR_987654321}");
        assert_eq!(result, "${JCODE_HOOKS_TEST_VAR_987654321}");
    }

    #[test]
    fn expand_missing_var_with_default() {
        let result = expand_env_var("${JCODE_HOOKS_TEST_MISSING:-fallback_value}");
        assert_eq!(result, "fallback_value");
    }

    #[test]
    fn expand_existing_var_with_default() {
        // PATH is set; the default should be ignored.
        let result = expand_env_var("${PATH:-/usr/bin}");
        assert_ne!(result, "/usr/bin");
        assert!(!result.is_empty());
    }

    #[test]
    fn expand_mixed_text() {
        std::env::set_var("_JCODE_TEST_EXPAND", "REPLACED");
        let result = expand_env_var("before_${_JCODE_TEST_EXPAND}_after");
        assert_eq!(result, "before_REPLACED_after");
        std::env::remove_var("_JCODE_TEST_EXPAND");
    }

    #[test]
    fn expand_multiple_vars() {
        std::env::set_var("_JCODE_TEST_A", "AAA");
        std::env::set_var("_JCODE_TEST_B", "BBB");
        let result = expand_env_var("${_JCODE_TEST_A}/${_JCODE_TEST_B}");
        assert_eq!(result, "AAA/BBB");
        std::env::remove_var("_JCODE_TEST_A");
        std::env::remove_var("_JCODE_TEST_B");
    }

    #[test]
    fn expand_dollar_brace_in_default() {
        let result = expand_env_var("${_JCODE_TEST_UNDEF:-${ALSO_UNDEF}}");
        // When the var is not set, the default is "${ALSO_UNDEF}" (literal).
        assert_eq!(result, "${ALSO_UNDEF}");
    }

    #[test]
    fn expand_single_dollar_no_brace() {
        // A bare $ without braces is not expanded.
        assert_eq!(expand_env_var("price is $5"), "price is $5");
    }

    #[test]
    fn expand_default_empty() {
        let result = expand_env_var("${_JCODE_TEST_UNDEF:-}");
        assert_eq!(result, "");
    }

    // -- interpret_exit_code ---------------------------------------------------

    #[test]
    fn exit_code_0_continue() {
        let output = HookOutput::continue_();
        let result = interpret_exit_code(0, output, "test").unwrap();
        assert!(matches!(result, HookResult::Continue(_)));
    }

    #[test]
    fn exit_code_1_fail() {
        let output = HookOutput::continue_();
        let result = interpret_exit_code(1, output, "test_cmd").unwrap();
        assert!(matches!(result, HookResult::Failed { .. }));
        if let HookResult::Failed { error } = result {
            assert!(error.contains("test_cmd"));
        }
    }

    #[test]
    fn exit_code_2_block() {
        let output = HookOutput::block("denied");
        let result = interpret_exit_code(2, output, "test_cmd").unwrap();
        assert!(matches!(result, HookResult::Blocked { .. }));
        if let HookResult::Blocked { reason, .. } = result {
            assert_eq!(reason, "denied");
        }
    }

    #[test]
    fn exit_code_2_block_with_output_reason() {
        let output = HookOutput {
            continue_: false,
            suppress_output: None,
            stop_reason: Some("custom reason".to_string()),
            decision: Some("deny".to_string()),
            reason: None,
            system_message: None,
            hook_specific_output: None,
        };
        let result = interpret_exit_code(2, output, "test_cmd").unwrap();
        if let HookResult::Blocked { reason, .. } = result {
            assert_eq!(reason, "custom reason");
        }
    }

    #[test]
    fn exit_code_other_fail() {
        let output = HookOutput::continue_();
        let result = interpret_exit_code(127, output, "missing_cmd").unwrap();
        assert!(matches!(result, HookResult::Failed { .. }));
        if let HookResult::Failed { error } = result {
            assert!(error.contains("127"));
        }
    }

    // -- build_command_env -----------------------------------------------------

    #[test]
    fn build_env_includes_standard_vars() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("PreToolUse")
            .build();
        let handler_env = HashMap::new();
        let env = build_command_env(&handler_env, &input);
        assert_eq!(env.get("JCODE_HOOK_EVENT").unwrap(), "PreToolUse");
        assert_eq!(env.get("JCODE_HOOK_SESSION_ID").unwrap(), "ses_1");
        assert_eq!(env.get("JCODE_HOOK_CWD").unwrap(), "/project");
    }

    #[test]
    fn build_env_merges_handler_env() {
        let input = HookInputBuilder::new()
            .session("ses_1", "/project")
            .event("PreToolUse")
            .build();
        let mut handler_env = HashMap::new();
        handler_env.insert("MY_VAR".to_string(), "my_value".to_string());
        let env = build_command_env(&handler_env, &input);
        assert_eq!(env.get("MY_VAR").unwrap(), "my_value");
    }

    // -- is_handler_enabled ----------------------------------------------------

    #[test]
    fn enabled_command() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            enabled: true,
            command: "test".to_string(),
            ..Default::default()
        });
        assert!(is_handler_enabled(&handler));
    }

    #[test]
    fn disabled_command() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            enabled: false,
            command: "test".to_string(),
            ..Default::default()
        });
        assert!(!is_handler_enabled(&handler));
    }

    #[test]
    fn enabled_http() {
        let handler = HookHandlerConfig::Http(HttpHandlerConfig {
            enabled: true,
            url: "http://localhost".to_string(),
            ..Default::default()
        });
        assert!(is_handler_enabled(&handler));
    }

    #[test]
    fn disabled_plugin() {
        let handler = HookHandlerConfig::Plugin(PluginHandlerConfig {
            enabled: false,
            path: "/usr/bin/plugin".to_string(),
            ..Default::default()
        });
        assert!(!is_handler_enabled(&handler));
    }

    // -- execute_single_hook (disabled handler) --------------------------------

    #[tokio::test]
    async fn disabled_handler_returns_continue() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            enabled: false,
            command: "echo should_not_run".to_string(),
            ..Default::default()
        });
        let input = HookInput::default();
        let result = execute_single_hook(&handler, &input, 5).await.unwrap();
        assert!(matches!(result, HookResult::Continue(_)));
    }

    // -- execute_command_hook (integration, requires `echo`) -------------------

    #[tokio::test]
    async fn command_hook_echo_continue() {
        let config = CommandHandlerConfig {
            enabled: true,
            command: "cat".to_string(), // cat reads stdin and echoes it
            ..Default::default()
        };
        let input = HookInputBuilder::new()
            .session("ses_test", "/tmp")
            .event("PreToolUse")
            .build();
        let result = execute_command_hook(&config, &input, 5).await.unwrap();
        // cat with stdin JSON will echo the JSON; since it's valid HookInput
        // but NOT valid HookOutput (it has different fields), it will fall back
        // to continue_(). Exit code 0 => Continue.
        assert!(matches!(result, HookResult::Continue(_)));
    }

    #[tokio::test]
    async fn command_hook_exit_2_blocks() {
        let config = CommandHandlerConfig {
            enabled: true,
            command:
                "echo '{\"continue_\": false, \"stop_reason\": \"blocked by test\"}' && exit 2"
                    .to_string(),
            ..Default::default()
        };
        let input = HookInput::default();
        let result = execute_command_hook(&config, &input, 5).await.unwrap();
        assert!(matches!(result, HookResult::Blocked { .. }));
        if let HookResult::Blocked { reason, .. } = result {
            assert_eq!(reason, "blocked by test");
        }
    }

    #[tokio::test]
    async fn command_hook_exit_1_fails() {
        let config = CommandHandlerConfig {
            enabled: true,
            command: "exit 1".to_string(),
            ..Default::default()
        };
        let input = HookInput::default();
        let result = execute_command_hook(&config, &input, 5).await.unwrap();
        assert!(matches!(result, HookResult::Failed { .. }));
    }

    // -- execute_command_hook with env vars ------------------------------------

    #[tokio::test]
    async fn command_hook_receives_env_vars() {
        let config = CommandHandlerConfig {
            enabled: true,
            // The command checks that JCODE_HOOK_EVENT is set.
            command: "test \"$JCODE_HOOK_EVENT\" = \"SessionStart\"".to_string(),
            ..Default::default()
        };
        let input = HookInputBuilder::new()
            .session("ses_env", "/tmp")
            .event("SessionStart")
            .build();
        let result = execute_command_hook(&config, &input, 5).await.unwrap();
        assert!(matches!(result, HookResult::Continue(_)));
    }

    // -- execute_agent_hook (placeholder) --------------------------------------

    #[tokio::test]
    async fn agent_hook_returns_not_implemented() {
        let config = AgentHandlerConfig {
            agent_id: "test_agent".to_string(),
            ..Default::default()
        };
        let input = HookInput::default();
        let result = execute_agent_hook(&config, &input).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ExecuteError::AgentNotImplemented
        ));
    }

    // -- execute_hook dispatches correctly ------------------------------------

    #[tokio::test]
    async fn execute_hook_dispatches_command() {
        let handler = HookHandlerConfig::Command(CommandHandlerConfig {
            enabled: true,
            command: "exit 0".to_string(),
            ..Default::default()
        });
        let input = HookInput::default();
        let result = execute_hook(&handler, &input, 5).await.unwrap();
        assert!(matches!(result, HookResult::Continue(_)));
    }

    #[tokio::test]
    async fn execute_hook_dispatches_agent() {
        let handler = HookHandlerConfig::Agent(AgentHandlerConfig {
            agent_id: "test".to_string(),
            ..Default::default()
        });
        let input = HookInput::default();
        let result = execute_hook(&handler, &input, 5).await;
        assert!(result.is_err());
    }

    // -- ExecuteError display --------------------------------------------------

    #[test]
    fn execute_error_display() {
        let err = ExecuteError::SpawnFailed("not found".to_string());
        assert!(format!("{}", err).contains("not found"));

        let err = ExecuteError::Timeout(30);
        assert!(format!("{}", err).contains("30"));

        let err = ExecuteError::AgentNotImplemented;
        assert!(format!("{}", err).contains("not yet implemented"));
    }

    // -- HookOutput JSON round-trip through command ---------------------------

    #[tokio::test]
    async fn command_hook_output_json_roundtrip() {
        // A command that outputs a valid HookOutput JSON.
        let output_json = r#"{"continue_": true, "decision": "allow"}"#;
        let config = CommandHandlerConfig {
            enabled: true,
            command: format!("echo '{}'", output_json),
            ..Default::default()
        };
        let input = HookInput::default();
        let result = execute_command_hook(&config, &input, 5).await.unwrap();
        if let HookResult::Continue(output) = result {
            assert!(output.continue_);
            assert_eq!(output.decision.as_deref(), Some("allow"));
        } else {
            panic!("expected Continue");
        }
    }
}
