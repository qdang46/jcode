use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ffs_engine::{Engine, EngineConfig};

static ENGINE: OnceLock<Mutex<EngineState>> = OnceLock::new();

struct EngineState {
    root: PathBuf,
    budget: u64,
    engine: Arc<Engine>,
}

/// Default token budget for engine-backed tools (matches ffs-mcp).
pub const DEFAULT_ENGINE_TOKEN_BUDGET: u64 = 25_000;

/// Prefer ffs when not explicitly disabled (opencode: `Fff.available()`).
pub fn ffs_preferred() -> bool {
    !matches!(
        std::env::var("JCODE_DISABLE_FFS").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

pub fn workspace_root(
    working_dir: Option<&PathBuf>,
    resolve: impl FnOnce(&Path) -> PathBuf,
    explicit: Option<&Path>,
) -> PathBuf {
    if let Some(p) = explicit {
        return resolve(p);
    }
    working_dir
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
}

fn state_cell() -> &'static Mutex<EngineState> {
    ENGINE.get_or_init(|| {
        Mutex::new(EngineState {
            root: PathBuf::new(),
            budget: 0,
            engine: Arc::new(Engine::new(EngineConfig::default())),
        })
    })
}

/// Lazy shared engine — cold index on first use per workspace root.
pub fn engine_holder(root: &Path, token_budget: u64) -> Arc<Engine> {
    let mut guard = state_cell().lock().expect("ffs engine lock");
    if guard.root == root && guard.budget == token_budget {
        return guard.engine.clone();
    }
    let cfg = EngineConfig {
        total_token_budget: token_budget,
        ..EngineConfig::default()
    };
    let engine = Arc::new(Engine::new(cfg));
    engine.index(root);
    guard.root = root.to_path_buf();
    guard.budget = token_budget;
    guard.engine = engine.clone();
    engine
}
