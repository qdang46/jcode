use chrono::{DateTime, Utc};
use jcode_message_types::ToolCall;
use jcode_session_types::SessionStatus;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionSource {
    Jcode,
    ClaudeCode,
    Codex,
    Pi,
    OpenCode,
    /// Any other provider known to CASR (gemini, cursor, cline, aider,
    /// amp, chatgpt, clawdbot, vibe, factory, openclaw, kiro, …).
    /// The string is the CASR provider slug (e.g. `"gemini"`,
    /// `"kiro"`). The TUI uses this to render a generic badge via
    /// `SessionSource::badge()`.
    Foreign(String),
}

impl SessionSource {
    pub fn badge(self) -> Option<String> {
        match self {
            Self::Jcode => None,
            Self::ClaudeCode => Some("🧵 Claude Code".to_string()),
            Self::Codex => Some("🧠 Codex".to_string()),
            Self::Pi => Some("π Pi".to_string()),
            Self::OpenCode => Some("◌ OpenCode".to_string()),
            Self::Foreign(slug) => Some(badge_for_foreign(&slug)),
        }
    }

    /// Short identifier used in the picker and for stable sorting.
    pub fn slug(&self) -> &'static str {
        match self {
            Self::Jcode => "jcode",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Pi => "pi-agent",
            Self::OpenCode => "opencode",
            Self::Foreign(_) => "foreign",
        }
    }
}

/// Map a CASR provider slug to a short badge string for the TUI.
/// Falls back to a title-cased dashed rendering for unknown slugs.
fn badge_for_foreign(slug: &str) -> String {
    let pretty = match slug {
        "gemini" => "✨ Gemini",
        "cursor" => "🖱 Cursor",
        "cline" => "🪶 Cline",
        "aider" => "🛠 Aider",
        "amp" => "⚡ Amp",
        "chatgpt" => "💬 ChatGPT",
        "clawdbot" => "🤖 ClawdBot",
        "vibe" => "🌀 Vibe",
        "factory" => "🏭 Factory",
        "openclaw" => "🐾 OpenClaw",
        "kiro" => "🪶 Kiro",
        _ => return title_case_dashed(slug),
    };
    pretty.to_string()
}

fn title_case_dashed(slug: &str) -> String {
    // Capitalize first letter of each dash-separated word.
    let mut out = String::with_capacity(slug.len());
    let mut at_word_start = true;
    for ch in slug.chars() {
        if ch == '-' || ch == '_' {
            out.push(' ');
            at_word_start = true;
        } else if at_word_start {
            for u in ch.to_uppercase() {
                out.push(u);
            }
            at_word_start = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ResumeTarget {
    JcodeSession {
        session_id: String,
    },
    ClaudeCodeSession {
        session_id: String,
        session_path: String,
    },
    CodexSession {
        session_id: String,
        session_path: String,
    },
    PiSession {
        session_path: String,
    },
    OpenCodeSession {
        session_id: String,
        session_path: String,
    },
    /// Any other foreign session discovered via CASR. `provider_slug` is
    /// the CASR provider slug (e.g. `"gemini"`, `"kiro"`, `"chatgpt"`).
    /// The session id and path are the source's own identifiers; the
    /// launcher passes the id through `casr::pipeline::derive_target_id`
    /// to produce a stable jcode session id on resume.
    ForeignSession {
        provider_slug: String,
        session_id: String,
        session_path: Option<String>,
    },
}

impl ResumeTarget {
    pub fn stable_id(&self) -> &str {
        match self {
            Self::JcodeSession { session_id } => session_id,
            Self::ClaudeCodeSession { session_id, .. } => session_id,
            Self::CodexSession { session_id, .. } => session_id,
            Self::PiSession { session_path } => session_path,
            Self::OpenCodeSession { session_id, .. } => session_id,
            Self::ForeignSession { session_id, .. } => session_id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SessionFilterMode {
    All,
    CatchUp,
    Saved,
    ClaudeCode,
    Codex,
    Pi,
    OpenCode,
    /// External CLI transcripts (Codex and/or Claude Code) shown together.
    /// Used by the first-run onboarding "continue where you left off" picker so
    /// it surfaces every external CLI the user is logged into, not just one.
    ExternalClis,
}

impl SessionFilterMode {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::CatchUp,
            Self::CatchUp => Self::Saved,
            Self::Saved => Self::ClaudeCode,
            Self::ClaudeCode => Self::Codex,
            Self::Codex => Self::Pi,
            Self::Pi => Self::OpenCode,
            Self::OpenCode => Self::All,
            // ExternalClis is an onboarding-only composite filter, not part of
            // the user-facing cycle; treat it as a no-op anchor.
            Self::ExternalClis => Self::All,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::All => Self::OpenCode,
            Self::CatchUp => Self::All,
            Self::Saved => Self::CatchUp,
            Self::ClaudeCode => Self::Saved,
            Self::Codex => Self::ClaudeCode,
            Self::Pi => Self::Codex,
            Self::OpenCode => Self::Pi,
            Self::ExternalClis => Self::All,
        }
    }

    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::CatchUp => Some("⏭ catch up"),
            Self::Saved => Some("📌 saved"),
            Self::ClaudeCode => Some("🧵 Claude Code"),
            Self::Codex => Some("🧠 Codex"),
            Self::Pi => Some("π Pi"),
            Self::OpenCode => Some("◌ OpenCode"),
            Self::ExternalClis => Some("🧠 Codex + 🧵 Claude Code"),
        }
    }
}

/// Session info for display in the interactive session picker.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionInfo {
    pub id: String,
    pub parent_id: Option<String>,
    pub short_name: String,
    pub icon: String,
    pub title: String,
    pub message_count: usize,
    pub user_message_count: usize,
    pub assistant_message_count: usize,
    pub created_at: DateTime<Utc>,
    pub last_message_time: DateTime<Utc>,
    pub last_active_at: Option<DateTime<Utc>>,
    pub working_dir: Option<String>,
    pub model: Option<String>,
    pub provider_key: Option<String>,
    pub is_canary: bool,
    pub is_debug: bool,
    pub saved: bool,
    pub save_label: Option<String>,
    pub status: SessionStatus,
    pub needs_catchup: bool,
    pub estimated_tokens: usize,
    /// First visible user prompt in the session, shown in compact list rows.
    pub first_user_prompt: Option<String>,
    pub messages_preview: Vec<PreviewMessage>,
    /// Lowercased searchable text used by picker filtering.
    pub search_index: String,
    /// Server name this session belongs to (if running).
    pub server_name: Option<String>,
    /// Server icon.
    pub server_icon: Option<String>,
    /// Human/session source classification shown in the UI.
    pub source: SessionSource,
    /// How this entry should be resumed when selected.
    pub resume_target: ResumeTarget,
    /// Backing external transcript/storage path when available.
    pub external_path: Option<String>,
}

/// A group of sessions under a server.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ServerGroup {
    pub name: String,
    pub icon: String,
    pub version: String,
    pub git_hash: String,
    pub is_running: bool,
    pub sessions: Vec<SessionInfo>,
}

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PreviewMessage {
    pub role: String,
    pub content: String,
    pub tool_calls: Vec<String>,
    pub tool_data: Option<ToolCall>,
    pub timestamp: Option<DateTime<Utc>>,
}

/// An item in the picker list, either a server/header row or a session row.
#[derive(Clone)]
pub enum PickerItem {
    ServerHeader {
        name: String,
        icon: String,
        version: String,
        session_count: usize,
    },
    Session,
    OrphanHeader {
        session_count: usize,
    },
    SavedHeader {
        session_count: usize,
    },
}

/// All `session_is_*` helpers take `&SessionSource` so they can be called
/// from a borrowed `SessionInfo` (where `session.source` is a shared
/// reference). Internally they pattern-match on the `SessionSource`
/// variant.
pub fn session_is_claude_code(source: &SessionSource, id: &str) -> bool {
    source == &SessionSource::ClaudeCode || id.starts_with("imported_cc_")
}

pub fn session_is_codex(source: &SessionSource, model: Option<&str>) -> bool {
    if source == &SessionSource::Codex {
        return true;
    }
    model
        .map(|model| model.to_ascii_lowercase().contains("codex"))
        .unwrap_or(false)
}

pub fn session_is_pi(
    source: &SessionSource,
    provider_key: Option<&str>,
    model: Option<&str>,
) -> bool {
    if source == &SessionSource::Pi {
        return true;
    }
    let provider_matches = provider_key
        .map(|key| {
            let key = key.to_ascii_lowercase();
            key == "pi" || key.starts_with("pi-")
        })
        .unwrap_or(false);
    let model_matches = model
        .map(|model| {
            let model = model.to_ascii_lowercase();
            model == "pi"
                || model.starts_with("pi-")
                || model.starts_with("pi/")
                || model.contains("/pi-")
        })
        .unwrap_or(false);
    provider_matches || model_matches
}

pub fn session_is_open_code(source: &SessionSource, provider_key: Option<&str>) -> bool {
    if source == &SessionSource::OpenCode {
        return true;
    }
    provider_key
        .map(|key| {
            let key = key.to_ascii_lowercase();
            key == "opencode" || key == "opencode-go" || key.contains("opencode")
        })
        .unwrap_or(false)
}

/// Catch-all: does this session belong to ANY CASR-registered provider
/// (not just the four hand-rolled ones)? Returns true for any
/// `SessionSource::Foreign(slug)`. Used by the filter to decide whether
/// a session should appear in the "external CLI" group of the TUI
/// session picker.
pub fn session_is_external_casr(source: &SessionSource) -> bool {
    matches!(source, SessionSource::Foreign(_))
        || session_is_claude_code(source, "")
        || session_is_codex(source, None)
        || session_is_pi(source, None, None)
        || session_is_open_code(source, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_target_stable_id_uses_durable_identifier() {
        let target = ResumeTarget::CodexSession {
            session_id: "abc".into(),
            session_path: "/tmp/session.json".into(),
        };
        assert_eq!(target.stable_id(), "abc");

        let target = ResumeTarget::PiSession {
            session_path: "/tmp/pi.jsonl".into(),
        };
        assert_eq!(target.stable_id(), "/tmp/pi.jsonl");
    }

    #[test]
    fn source_predicates_cover_provider_and_model_fallbacks() {
        assert!(session_is_claude_code(
            SessionSource::Jcode,
            "imported_cc_123"
        ));
        assert!(session_is_codex(
            SessionSource::Jcode,
            Some("openai/codex-mini")
        ));
        assert!(session_is_pi(SessionSource::Jcode, Some("pi-main"), None));
        assert!(session_is_pi(
            SessionSource::Jcode,
            None,
            Some("vendor/pi-fast")
        ));
        assert!(session_is_open_code(
            SessionSource::Jcode,
            Some("opencode-go")
        ));
    }
}
