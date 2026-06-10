//! Shared ffs integration: engine holder, file picker, ripgrep fallbacks.
//!
//! Mirrors opencode's `Fff.available() ? fffLayer : ripgrepLayer` — prefer ffs
//! **crate APIs** (`ffs-search`, `ffs-symbol`, `ffs-engine`); fall back to `rg`
//! only when ffs path fails or `JCODE_DISABLE_FFS` is set.
//!
//! Do **not** shell out to the `ffs` CLI — that pattern is for ffs-mcp only
//! (MCP server runs inside the ffs binary). jcode links the crates directly.

mod backend;
mod engine_nav;
mod fallback;
mod picker;

pub use backend::{
    DEFAULT_ENGINE_TOKEN_BUDGET, engine_holder, ffs_preferred, workspace_root,
};
pub use engine_nav::{
    CallHit, RefDefinition, RefUsage, collect_definitions, collect_usages, find_callee_sites,
    find_call_sites, format_call_hits, format_dispatch, format_flow_card, format_refs,
    format_symbol_hits,
};
pub use fallback::{
    find_fuzzy_walkdir, format_grep_hits, glob_crate, glob_ripgrep, grep_ripgrep, grep_walkdir,
    rg_available, GrepHit,
};
pub use picker::{find_files, with_file_picker};
