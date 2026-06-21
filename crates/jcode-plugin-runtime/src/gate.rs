//! Approval gate -- single chokepoint for every tool call.
//!
//! The `ApprovalGate` layers user overrides, the capability chain check, and
//! the permission mode to produce a [`GateDecision`] for each tool invocation.
//! This is the plugin-equivalent of what `dcg_core::Mode` does for shell commands.

use jcode_agent_runtime::PermissionMode;
use jcode_plugin_core::{CapabilityAction, CapabilityChainV2, ToolTier};
use std::collections::HashMap;
use tracing;

/// Hard override that a user or policy can attach to a specific tool name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalOverride {
    Allow,
    Deny,
    Prompt,
}

/// The result of running a tool through [`ApprovalGate::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    /// Tool call is unconditionally allowed.
    Allow,
    /// Tool call is denied. `layer` identifies which check rejected it.
    Deny { reason: String, layer: String },
    /// Tool call requires interactive human approval before it can proceed.
    NeedsApproval { prompt: ApprovalPrompt },
}

/// Structured prompt describing why a tool call needs human approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPrompt {
    pub tool_name: String,
    pub tier: ToolTier,
    pub reason: String,
    pub mode: PermissionMode,
}

/// Single chokepoint that checks every tool invocation.
///
/// Evaluation order:
///   1. User override (if present, wins unconditionally)
///   2. Capability chain (5-layer allow/deny/prompt from `CapabilityChainV2`)
///   3. Permission mode (which tiers auto-approve in the current mode)
pub struct ApprovalGate {
    chain: CapabilityChainV2,
    mode: PermissionMode,
    user_overrides: HashMap<String, ApprovalOverride>,
}

impl ApprovalGate {
    pub fn new(
        chain: CapabilityChainV2,
        mode: PermissionMode,
        user_overrides: HashMap<String, ApprovalOverride>,
    ) -> Self {
        Self {
            chain,
            mode,
            user_overrides,
        }
    }

    /// Run the full evaluation pipeline for a single tool call.
    pub fn check(
        &self,
        tool_name: &str,
        tier: ToolTier,
        _args: &serde_json::Value,
    ) -> GateDecision {
        // 1. User override wins unconditionally
        if let Some(ov) = self.user_overrides.get(tool_name) {
            return match ov {
                ApprovalOverride::Allow => GateDecision::Allow,
                ApprovalOverride::Deny => {
                    let reason = format!("user policy denies '{}'", tool_name);
                    let layer = "user_override".to_string();
                    tracing::warn!(
                        "plugin gate denied tool '{tool_name}': {reason} (layer {layer})"
                    );
                    GateDecision::Deny { reason, layer }
                }
                ApprovalOverride::Prompt => GateDecision::NeedsApproval {
                    prompt: self.prompt_for(tool_name, tier),
                },
            };
        }

        // 2. Capability chain check
        let resource = format!("tool:{}", tool_name);
        let action = match tier {
            ToolTier::Read => CapabilityAction::Read,
            ToolTier::Write => CapabilityAction::Write,
            ToolTier::Exec => CapabilityAction::Execute,
        };
        match self.chain.check(&resource, &action) {
            jcode_plugin_core::AccessDecisionV2::Allow { .. } => {
                // Mode interaction: does this tier auto-approve in the current mode?
                if !self.auto_approves(tier) {
                    return GateDecision::NeedsApproval {
                        prompt: self.prompt_for(tool_name, tier),
                    };
                }
                GateDecision::Allow
            }
            jcode_plugin_core::AccessDecisionV2::Deny { reason, layer } => {
                let layer_str = format!("layer_{}", layer);
                tracing::warn!(
                    "plugin gate denied tool '{tool_name}': {reason} (layer {layer_str})"
                );
                GateDecision::Deny {
                    reason,
                    layer: layer_str,
                }
            }
            jcode_plugin_core::AccessDecisionV2::NeedsApproval {
                reason: _,
                layer: _,
            } => {
                if self.auto_approves(tier) {
                    GateDecision::Allow
                } else {
                    GateDecision::NeedsApproval {
                        prompt: self.prompt_for(tool_name, tier),
                    }
                }
            }
        }
    }

    /// Map a tier + the current permission mode to whether the tier is
    /// auto-approved (no prompt needed).
    fn auto_approves(&self, tier: ToolTier) -> bool {
        match (self.mode, tier) {
            (PermissionMode::Plan, _) => false,
            (PermissionMode::AcceptEdits, ToolTier::Read) => true,
            (PermissionMode::AcceptEdits, ToolTier::Write) => true,
            (PermissionMode::AcceptEdits, ToolTier::Exec) => false,
            (PermissionMode::BypassPermissions, _) => true,
            (PermissionMode::DontAsk, ToolTier::Read) => true,
            (PermissionMode::DontAsk, ToolTier::Write) => false,
            (PermissionMode::DontAsk, ToolTier::Exec) => false,
            // Default mode: read auto-approves, write and exec prompt
            (PermissionMode::Default, ToolTier::Read) => true,
            (PermissionMode::Default, ToolTier::Write) => false,
            (PermissionMode::Default, ToolTier::Exec) => false,
            // Auto mode (LLM classifier) — fall to prompt by default
            (PermissionMode::Auto, _) => false,
        }
    }

    fn prompt_for(&self, tool_name: &str, tier: ToolTier) -> ApprovalPrompt {
        ApprovalPrompt {
            tool_name: tool_name.into(),
            tier,
            reason: format!("{:?} tier tool requires approval", tier),
            mode: self.mode,
        }
    }

    /// Replace the permission mode at runtime (e.g. after a mode cycle).
    pub fn set_mode(&mut self, mode: PermissionMode) {
        self.mode = mode;
    }

    /// Borrow the current permission mode.
    pub fn mode(&self) -> PermissionMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_agent_runtime::PermissionMode;
    use jcode_plugin_core::{CapabilityChainV2, PolicyMode};

    // -----------------------------------------------------------------
    // 1. BypassPermissions — everything allowed
    // -----------------------------------------------------------------
    #[test]
    fn bypass_allows_all_tiers() {
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Permissive,
                ..Default::default()
            },
            PermissionMode::BypassPermissions,
            HashMap::new(),
        );
        for (name, tier) in [
            ("read", ToolTier::Read),
            ("write", ToolTier::Write),
            ("exec", ToolTier::Exec),
        ] {
            let d = gate.check(name, tier, &serde_json::json!({}));
            assert_eq!(d, GateDecision::Allow, "bypass should allow {tier:?}");
        }
    }

    // -----------------------------------------------------------------
    // 2. User override beats everything
    // -----------------------------------------------------------------
    #[test]
    fn user_override_deny_wins_over_bypass() {
        let mut overrides = HashMap::new();
        overrides.insert("bash".into(), ApprovalOverride::Deny);
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Permissive,
                ..Default::default()
            },
            PermissionMode::BypassPermissions,
            overrides,
        );
        match gate.check("bash", ToolTier::Exec, &serde_json::json!({})) {
            GateDecision::Deny { layer, .. } => {
                assert_eq!(layer, "user_override")
            }
            other => panic!("expected Deny(user_override), got {other:?}"),
        }
    }

    #[test]
    fn user_override_allow_wins_over_strict_capability() {
        let mut overrides = HashMap::new();
        overrides.insert("rm".into(), ApprovalOverride::Allow);
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Strict,
                ..Default::default()
            },
            PermissionMode::Plan,
            overrides,
        );
        assert_eq!(
            gate.check("rm", ToolTier::Exec, &serde_json::json!({})),
            GateDecision::Allow
        );
    }

    // -----------------------------------------------------------------
    // 3. Plan mode — everything needs approval
    // -----------------------------------------------------------------
    #[test]
    fn plan_mode_prompts_all_tiers() {
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Prompt,
                ..Default::default()
            },
            PermissionMode::Plan,
            HashMap::new(),
        );
        for (name, tier) in [
            ("read", ToolTier::Read),
            ("write", ToolTier::Write),
            ("exec", ToolTier::Exec),
        ] {
            let d = gate.check(name, tier, &serde_json::json!({}));
            assert!(
                matches!(d, GateDecision::NeedsApproval { .. }),
                "plan mode should prompt {tier:?}, got {d:?}"
            );
        }
    }

    // -----------------------------------------------------------------
    // 4. AcceptEdits — Exec prompts, Read/Write allowed
    // -----------------------------------------------------------------
    #[test]
    fn accept_edits_prompts_exec_only() {
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Prompt,
                ..Default::default()
            },
            PermissionMode::AcceptEdits,
            HashMap::new(),
        );
        assert_eq!(
            gate.check("read-file", ToolTier::Read, &serde_json::json!({})),
            GateDecision::Allow
        );
        assert_eq!(
            gate.check("write-file", ToolTier::Write, &serde_json::json!({})),
            GateDecision::Allow
        );
        assert!(
            matches!(
                gate.check("bash", ToolTier::Exec, &serde_json::json!({})),
                GateDecision::NeedsApproval { .. }
            ),
            "AcceptEdits should prompt Exec"
        );
    }

    // -----------------------------------------------------------------
    // 5. Default mode — Read allowed, Write/Exec prompt
    // -----------------------------------------------------------------
    #[test]
    fn default_mode_allows_read_prompts_write_exec() {
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Prompt,
                ..Default::default()
            },
            PermissionMode::Default,
            HashMap::new(),
        );
        assert_eq!(
            gate.check("grep", ToolTier::Read, &serde_json::json!({})),
            GateDecision::Allow
        );
        assert!(
            matches!(
                gate.check("edit", ToolTier::Write, &serde_json::json!({})),
                GateDecision::NeedsApproval { .. }
            ),
            "Default mode should prompt Write"
        );
        assert!(
            matches!(
                gate.check("bash", ToolTier::Exec, &serde_json::json!({})),
                GateDecision::NeedsApproval { .. }
            ),
            "Default mode should prompt Exec"
        );
    }

    // -----------------------------------------------------------------
    // 6. Capability chain deny propagates
    // -----------------------------------------------------------------
    #[test]
    fn capability_chain_deny_returns_deny() {
        let chain = CapabilityChainV2 {
            mode: PolicyMode::Strict,
            ..Default::default()
        };
        let gate = ApprovalGate::new(chain, PermissionMode::BypassPermissions, HashMap::new());
        // Strict mode with no allow lists → layer 5 deny
        match gate.check("anything", ToolTier::Read, &serde_json::json!({})) {
            GateDecision::Deny { layer, .. } => {
                assert!(
                    layer.starts_with("layer_"),
                    "layer should be layer_N: {layer}"
                )
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // 7. DontAsk — Read allowed, Write/Exec prompt
    // -----------------------------------------------------------------
    #[test]
    fn dont_ask_prompts_non_read() {
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Permissive,
                ..Default::default()
            },
            PermissionMode::DontAsk,
            HashMap::new(),
        );
        assert_eq!(
            gate.check("list", ToolTier::Read, &serde_json::json!({})),
            GateDecision::Allow
        );
        assert!(
            matches!(
                gate.check("create", ToolTier::Write, &serde_json::json!({})),
                GateDecision::NeedsApproval { .. }
            ),
            "DontAsk should prompt Write"
        );
    }

    // -----------------------------------------------------------------
    // 8. Empty args does not panic
    // -----------------------------------------------------------------
    #[test]
    fn empty_args_no_panic() {
        let gate = ApprovalGate::new(
            Default::default(),
            PermissionMode::BypassPermissions,
            HashMap::new(),
        );
        let _ = gate.check("", ToolTier::Read, &serde_json::json!({}));
    }

    // -----------------------------------------------------------------
    // 9. set_mode changes behaviour
    // -----------------------------------------------------------------
    #[test]
    fn set_mode_updates_gate_behavior() {
        let mut gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Permissive,
                ..Default::default()
            },
            PermissionMode::Default,
            HashMap::new(),
        );
        // Default: Read auto-approved
        assert_eq!(
            gate.check("cat", ToolTier::Read, &serde_json::json!({})),
            GateDecision::Allow
        );
        // Switch to Plan: Read now prompts
        gate.set_mode(PermissionMode::Plan);
        assert!(
            matches!(
                gate.check("cat", ToolTier::Read, &serde_json::json!({})),
                GateDecision::NeedsApproval { .. }
            ),
            "Plan mode should prompt after set_mode"
        );
    }

    // -----------------------------------------------------------------
    // 10. Auto mode (LLM classifier) — prompt by default
    // -----------------------------------------------------------------
    #[test]
    fn auto_mode_prompts_everything() {
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Permissive,
                ..Default::default()
            },
            PermissionMode::Auto,
            HashMap::new(),
        );
        for (name, tier) in [
            ("read", ToolTier::Read),
            ("write", ToolTier::Write),
            ("exec", ToolTier::Exec),
        ] {
            let d = gate.check(name, tier, &serde_json::json!({}));
            assert!(
                matches!(d, GateDecision::NeedsApproval { .. }),
                "Auto mode should prompt {tier:?}, got {d:?}"
            );
        }
    }

    // -----------------------------------------------------------------
    // 11. User override Prompt forces NeedsApproval
    // -----------------------------------------------------------------
    #[test]
    fn user_override_prompt_forces_needs_approval() {
        let mut overrides = HashMap::new();
        overrides.insert("safe-tool".into(), ApprovalOverride::Prompt);
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Permissive,
                ..Default::default()
            },
            PermissionMode::BypassPermissions,
            overrides,
        );
        // Even though BypassPermissions auto-approves everything, the
        // user override forces a prompt.
        let d = gate.check("safe-tool", ToolTier::Read, &serde_json::json!({}));
        assert!(
            matches!(d, GateDecision::NeedsApproval { .. }),
            "user override Prompt should force NeedsApproval, got {d:?}"
        );
    }

    // -----------------------------------------------------------------
    // 12. Chain NeedsApproval + mode that auto-approves tier = Allow
    // -----------------------------------------------------------------
    #[test]
    fn chain_needs_approval_with_auto_approve_mode_allows() {
        // Chain returns NeedsApproval (e.g. PolicyMode::Prompt),
        // but mode = BypassPermissions which auto-approves.
        let chain = CapabilityChainV2 {
            mode: PolicyMode::Prompt, // layer 5 → NeedsApproval
            ..Default::default()
        };
        let gate = ApprovalGate::new(chain, PermissionMode::BypassPermissions, HashMap::new());
        // NeedsApproval from chain, but BypassPermissions → Allow
        assert_eq!(
            gate.check("any", ToolTier::Exec, &serde_json::json!({})),
            GateDecision::Allow
        );
    }

    // -----------------------------------------------------------------
    // 13. mode() getter
    // -----------------------------------------------------------------
    #[test]
    fn mode_getter_returns_stored_mode() {
        let gate = ApprovalGate::new(Default::default(), PermissionMode::Plan, HashMap::new());
        assert_eq!(gate.mode(), PermissionMode::Plan);
    }

    // -----------------------------------------------------------------
    // 14. DontAsk mode + Exec needs approval
    // -----------------------------------------------------------------
    #[test]
    fn test_dont_ask_mode_prompts_exec() {
        let gate = ApprovalGate::new(
            CapabilityChainV2 {
                mode: PolicyMode::Prompt,
                ..Default::default()
            },
            PermissionMode::DontAsk,
            HashMap::new(),
        );
        // DontAsk + Exec should need approval (Exec is never auto-approved in DontAsk)
        match gate.check("exec_tool", ToolTier::Exec, &serde_json::json!({})) {
            GateDecision::NeedsApproval { .. } => {} // expected
            other => panic!("DontAsk + Exec should need approval, got: {other:?}"),
        }
        // DontAsk + Read should auto-approve
        match gate.check("read_tool", ToolTier::Read, &serde_json::json!({})) {
            GateDecision::Allow => {} // expected
            other => panic!("DontAsk + Read should auto-approve, got: {other:?}"),
        }
    }
}
