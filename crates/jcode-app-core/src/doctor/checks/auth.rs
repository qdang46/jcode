//! Auth checks (offline only): count configured providers, validate `auth.json`
//! parse, and tighten `auth.json` permissions to 0600 (unix, auto-fixable).
//!
//! Live credential/connectivity verification deliberately stays in
//! `jcode auth doctor` / `jcode auth-test` / `jcode provider-doctor`.

use super::super::types::{CheckCategory, DoctorOptions, Finding};

/// (provider, oauth file basename under JCODE_HOME, env vars that count as configured).
const PROVIDERS: &[(&str, &str, &[&str])] = &[
    (
        "anthropic",
        "auth.json",
        &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ],
    ),
    ("openai", "openai-auth.json", &["OPENAI_API_KEY"]),
    (
        "gemini",
        "gemini_oauth.json",
        &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
    ),
    (
        "antigravity",
        "antigravity_oauth.json",
        &["ANTIGRAVITY_API_KEY"],
    ),
    ("openrouter", "", &["OPENROUTER_API_KEY"]),
    ("deepseek", "", &["DEEPSEEK_API_KEY"]),
    ("groq", "", &["GROQ_API_KEY"]),
    ("xai", "", &["XAI_API_KEY"]),
    ("zai", "", &["ZHIPU_API_KEY", "ZAI_API_KEY"]),
    ("cohere", "", &["COHERE_API_KEY"]),
    ("fireworks", "", &["FIREWORKS_API_KEY"]),
    ("minimax", "", &["MINIMAX_API_KEY"]),
    ("mistral", "", &["MISTRAL_API_KEY"]),
    ("openai-compatible", "", &["OPENAI_COMPAT_API_KEY"]),
    ("perplexity", "", &["PERPLEXITY_API_KEY"]),
    ("togetherai", "", &["TOGETHER_API_KEY"]),
];

pub fn check_auth(opts: &DoctorOptions, out: &mut Vec<Finding>) {
    let home = match crate::storage::jcode_dir() {
        Ok(h) => h,
        Err(_) => {
            out.push(Finding::warn(
                CheckCategory::Auth,
                "cannot resolve JCODE_HOME; skipping auth checks",
            ));
            return;
        }
    };

    let mut configured = 0usize;
    for (name, oauth_file, envs) in PROVIDERS {
        let has_oauth = !oauth_file.is_empty() && home.join(oauth_file).is_file();
        let env_hit = envs.iter().find(|e| {
            std::env::var(e)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
        });
        let detail = match (has_oauth, env_hit) {
            (true, Some(e)) => Some(format!("oauth file + env {e} (env wins at runtime)")),
            (true, None) => Some(format!("oauth file ~/.jcode/{oauth_file}")),
            (false, Some(e)) => Some(format!("env {e}")),
            (false, None) => None,
        };
        if let Some(detail) = detail {
            configured += 1;
            out.push(Finding::ok(
                CheckCategory::Auth,
                format!("{name}: {detail}"),
            ));
        }
    }
    if configured == 0 {
        out.push(
            Finding::warn(CheckCategory::Auth, "no providers configured")
                .with_remediation("run `jcode login --provider <name>`"),
        );
    } else {
        out.push(
            Finding::ok(
                CheckCategory::Auth,
                format!("{configured} provider(s) configured"),
            )
            .with_detail("run `jcode auth doctor` / `jcode auth-test` for live verification"),
        );
    }

    let auth_json = home.join("auth.json");
    if auth_json.is_file() {
        if let Ok(text) = std::fs::read_to_string(&auth_json)
            && serde_json::from_str::<serde_json::Value>(&text).is_err()
        {
            out.push(
                Finding::fail(CheckCategory::Auth, "auth.json is not valid JSON")
                    .with_remediation("re-run `jcode login`, or fix/remove auth.json"),
            );
        }
        check_auth_permissions(opts, &auth_json, out);
    }
}

#[cfg(unix)]
fn check_auth_permissions(
    opts: &DoctorOptions,
    auth_json: &std::path::Path,
    out: &mut Vec<Finding>,
) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(auth_json) else {
        return;
    };
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        let f = Finding::warn(
            CheckCategory::Auth,
            format!("auth.json is group/world accessible (mode {mode:o})"),
        )
        .with_remediation(format!("chmod 600 {}", auth_json.display()));
        let path = auth_json.to_path_buf();
        out.push(super::super::fix::try_autofix(opts, f, move || {
            super::super::fix::chmod(&path, 0o600)?;
            Ok("set mode to 600".to_string())
        }));
    }
}

#[cfg(not(unix))]
fn check_auth_permissions(
    _opts: &DoctorOptions,
    _auth_json: &std::path::Path,
    _out: &mut Vec<Finding>,
) {
}
