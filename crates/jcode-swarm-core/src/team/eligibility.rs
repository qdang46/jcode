//! Agent eligibility for team membership — port of
//! `AGENT_ELIGIBILITY_REGISTRY` in `types.ts`.
//!
//! Team members must be able to *write* JSON files into peer inboxes, so
//! read-only agents are hard-rejected.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    Eligible,
    Conditional,
    HardReject,
}

/// Returns the eligibility verdict and an explanation for the given agent type.
pub fn eligibility(agent_type: &str) -> (Eligibility, &'static str) {
    match agent_type {
        "sisyphus" | "sisyphus-junior" | "atlas" => (Eligibility::Eligible, ""),
        "hephaestus" => (
            Eligibility::Conditional,
            "agent 'hephaestus' lacks teammate permission by default; \
             grant it or use subagent_type 'sisyphus' instead",
        ),
        "oracle" | "librarian" | "explore" | "multimodal-looker" | "metis" | "momus"
        | "prometheus" => (
            Eligibility::HardReject,
            "agent is read-only and cannot write to the team mailbox; \
             use delegate-task / subagent for read-only analysis instead",
        ),
        // jcode-native default worker and any unknown custom worker are eligible.
        _ => (Eligibility::Eligible, ""),
    }
}

/// `Ok(())` if the agent may be a team member, else `Err(reason)`.
pub fn assert_eligible(agent_type: &str) -> Result<(), String> {
    match eligibility(agent_type) {
        (Eligibility::HardReject, msg) => Err(msg.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_agents_rejected() {
        for a in [
            "oracle",
            "librarian",
            "explore",
            "metis",
            "momus",
            "multimodal-looker",
        ] {
            assert!(assert_eligible(a).is_err(), "{a} must be rejected");
        }
    }

    #[test]
    fn workers_eligible() {
        for a in ["sisyphus", "sisyphus-junior", "atlas", "some-custom-worker"] {
            assert!(assert_eligible(a).is_ok(), "{a} must be eligible");
        }
    }

    #[test]
    fn hephaestus_is_conditional_but_not_hard_rejected() {
        assert_eq!(eligibility("hephaestus").0, Eligibility::Conditional);
        assert!(assert_eligible("hephaestus").is_ok());
    }
}
