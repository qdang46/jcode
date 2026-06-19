#![allow(
    unknown_lints,
    dead_code,
    unused_imports,
    unused_variables,
    clippy::collapsible_match,
    clippy::manual_checked_ops,
    clippy::unnecessary_sort_by,
    clippy::useless_conversion,
    clippy::needless_pass_by_value,
    clippy::module_name_repetitions,
    clippy::large_enum_variant,
    clippy::result_large_err,
    clippy::empty_line_after_doc_comments,
    clippy::needless_range_loop,
    clippy::manual_clamp,
    clippy::redundant_locals
)]

//! Application core for jcode (upper layer).
//!
//! This crate holds the server/tool/agent layer and its presentation-adjacent
//! leaves. The foundational layer it builds on (provider, auth, config, session,
//! message, memory, telemetry, ...) lives in the `jcode-base` crate and is
//! re-exported here via `pub use jcode_base::*`, so every existing
//! `crate::<module>` path (e.g. `crate::config`, `crate::provider`) keeps
//! resolving unchanged across this crate and the root `jcode` crate, which in
//! turn re-exports this crate via `pub use jcode_app_core::*`.

// Foundational layer: re-export every `jcode-base` module so `crate::<module>`
// paths resolve here exactly as they did before the split.
pub use jcode_base::*;

// Upper layer (server / tool / agent and supporting leaves).
pub mod agent;
pub mod ambient;
pub mod ambient_runner;
pub mod ambient_scheduler;
pub mod build;
pub mod catchup;
pub mod channel;
pub mod dcg_bridge;
pub mod doctor;
pub mod execution_policy;
pub mod external_auth;
pub mod mission;
pub mod network_retry;
pub mod notifications;
pub mod overnight;
pub mod perf;
pub mod plugin;
pub mod replay;
pub mod restart_snapshot;
pub mod server;
pub mod server_spawn;
pub mod session_launch;
pub mod session_rebuild;
pub mod setup_hints;
pub mod ssh_remote;
pub mod startup_profile;
pub mod tool;
pub mod update;
pub mod yolo_classifier;

#[cfg(feature = "dcp")]
pub mod dcp_bridge;
#[cfg(feature = "dcp")]
pub mod dcp_plugin;

#[cfg(feature = "rtco")]
pub mod rtco_filter;

use std::sync::Mutex;

static CURRENT_SESSION_ID: Mutex<Option<String>> = Mutex::new(None);

pub fn set_current_session(session_id: &str) {
    if let Ok(mut guard) = CURRENT_SESSION_ID.lock() {
        *guard = Some(session_id.to_string());
    }
}

pub fn get_current_session() -> Option<String> {
    CURRENT_SESSION_ID.lock().ok()?.clone()
}
pub use jcode_swarm_core::team;
