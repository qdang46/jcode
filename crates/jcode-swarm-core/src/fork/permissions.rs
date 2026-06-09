//! Fork tool permission utilities.
//!
//! Maps jcode's tool system to the restricted permission model required
//! by forked agents. This bridges `ForkPermissionMode` (the fork's policy)
//! with `jcode_tool_core::ToolContext` (the execution layer).

use crate::fork::ForkPermissionMode;
use serde_json::Value;

/// Wraps a parent `ToolContext` with fork-level permission gating.
///
/// Every tool call goes through this wrapper before execution:
/// 1. Check `ForkPermissionMode::is_tool_allowed()`
/// 2. If denied, return a descriptive error string
/// 3. If allowed, delegate to the inner `ToolContext`
///
/// ## Usage in the fork query loop
/// When the fork's model makes a tool call, the execution layer calls
/// `ForkToolContext::check_tool()` instead of the normal tool dispatch.
/// This ensures the fork's restricted permissions are enforced.
pub struct ForkToolContext {
    /// The fork's permission policy.
    permission: ForkPermissionMode,
    /// Fork label for analytics.
    fork_label: String,
}

impl ForkToolContext {
    pub fn new(permission: ForkPermissionMode, fork_label: String) -> Self {
        Self {
            permission,
            fork_label,
        }
    }

    /// Check whether a tool call is allowed under the fork's permission mode.
    ///
    /// # Returns
    /// - `Ok(())` if the tool is allowed (caller should execute it).
    /// - `Err(String)` if denied (caller should return the error as a tool result).
    pub fn check_tool(&self, tool_name: &str, input: &Value) -> Result<(), String> {
        if self.permission.is_tool_allowed(tool_name, input) {
            Ok(())
        } else {
            Err(self.permission.denial_reason(tool_name))
        }
    }

    /// Get the fork label for analytics.
    pub fn fork_label(&self) -> &str {
        &self.fork_label
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_fork_tool_context_allows_read_tools() {
        let ctx = ForkToolContext::new(
            ForkPermissionMode::MemoryExtraction {
                memory_dir: PathBuf::from(".jcode/memory"),
            },
            "test".to_string(),
        );

        assert!(ctx.check_tool("read", &serde_json::json!({"file_path": "/etc/passwd"})).is_ok());
        assert!(ctx.check_tool("grep", &serde_json::json!({"pattern": "test"})).is_ok());
    }

    #[test]
    fn test_fork_tool_context_denies_write_outside_memory_dir() {
        let ctx = ForkToolContext::new(
            ForkPermissionMode::MemoryExtraction {
                memory_dir: PathBuf::from(".jcode/memory"),
            },
            "test".to_string(),
        );

        let result = ctx.check_tool("write", &serde_json::json!({"file_path": "/etc/config"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("write"));
    }

    #[test]
    fn test_fork_tool_context_allows_write_in_memory_dir() {
        let ctx = ForkToolContext::new(
            ForkPermissionMode::MemoryExtraction {
                memory_dir: PathBuf::from(".jcode/memory"),
            },
            "test".to_string(),
        );

        assert!(ctx
            .check_tool("write", &serde_json::json!({"file_path": ".jcode/memory/test.md"}))
            .is_ok());
    }

    #[test]
    fn test_fork_tool_context_denies_unknown_tools() {
        let ctx = ForkToolContext::new(
            ForkPermissionMode::MemoryExtraction {
                memory_dir: PathBuf::from(".jcode/memory"),
            },
            "test".to_string(),
        );

        assert!(ctx.check_tool("agent_spawn", &serde_json::json!({})).is_err());
    }

    #[test]
    fn test_fork_tool_context_fork_label_is_accessible() {
        let ctx = ForkToolContext::new(
            ForkPermissionMode::MemoryExtraction {
                memory_dir: PathBuf::from(".jcode/memory"),
            },
            "my_fork".to_string(),
        );

        assert_eq!(ctx.fork_label(), "my_fork");
    }
}
