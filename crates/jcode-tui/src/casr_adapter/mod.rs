//! CASR (Cross Agent Session Resumer) stub module.
//!
//! The CASR feature was removed, but references remain in inline_interactive.rs,
//! helpers.rs, session_picker.rs, and tui_launch.rs. These stubs return the
//! session_id/path as-is so the build passes and the resume-target buttons
//! gracefully degrade to default import behavior.

pub fn imported_session_id_for_provider(_provider_slug: &str, session_id: &str) -> String {
    session_id.to_string()
}

pub fn imported_codex_session_id(session_id: &str) -> String {
    session_id.to_string()
}

pub fn imported_pi_session_id(session_path: &str) -> String {
    session_path.to_string()
}

pub fn imported_opencode_session_id(session_id: &str) -> String {
    session_id.to_string()
}

pub fn imported_claude_code_session_id(session_id: &str) -> String {
    session_id.to_string()
}
