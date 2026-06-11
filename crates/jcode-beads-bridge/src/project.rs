//! `BeadsProject` — facade over beads_rust project lifecycle.

use beads_rust::config::{self, CliOverrides, ConfigLayer, ConfigPaths, discover_beads_dir};
use beads_rust::storage::sqlite::SqliteStorage;
use beads_rust::sync::history::HistoryConfig;
use beads_rust::sync::{self, blocking_write_lock};

use anyhow::{Context, Result};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::info;

/// A discovered and opened beads_rust project.
pub struct BeadsProject {
    pub beads_dir: PathBuf,
    pub jsonl_path: PathBuf,
    pub config: ConfigLayer,
    _write_lock: Option<File>,
    storage: Mutex<SqliteStorage>,
}

impl BeadsProject {
    pub fn open(working_dir: &Path) -> Result<Self> {
        let beads_dir = discover_beads_dir(Some(working_dir))?;
        let (storage, paths): (SqliteStorage, ConfigPaths) =
            config::open_storage(&beads_dir, None, None)?;
        let jsonl_path = paths.jsonl_path.clone();
        let config = config::load_config(&beads_dir, Some(&storage), &CliOverrides::default())
            .map_err(|e| anyhow::anyhow!("config load failed: {e}"))?;
        let lock = blocking_write_lock(&beads_dir).ok();
        info!(?beads_dir, "Opened beads project");
        Ok(BeadsProject {
            beads_dir,
            jsonl_path,
            config,
            _write_lock: lock,
            storage: Mutex::new(storage),
        })
    }

    pub fn open_or_init(working_dir: &Path, prefix: &str) -> Result<Self> {
        match Self::open(working_dir) {
            Ok(p) => Ok(p),
            Err(_) => Self::init(working_dir, prefix),
        }
    }

    pub fn init(working_dir: &Path, prefix: &str) -> Result<Self> {
        let beads_dir = working_dir.join(".beads");
        fs::create_dir_all(&beads_dir)
            .with_context(|| format!("Failed to create {}/", beads_dir.display()))?;
        let db_path = beads_dir.join("beads.db");
        let storage = SqliteStorage::open(&db_path)
            .map_err(|e| anyhow::anyhow!("SQLite open failed: {e}"))?;
        let jsonl_path = beads_dir.join("issues.jsonl");
        let cfg_path = beads_dir.join("config.yaml");
        if !cfg_path.exists() {
            fs::write(&cfg_path, format!("id_prefix: \"{prefix}\"\n"))
                .with_context(|| "Failed to write config.yaml")?;
        }
        let config = config::load_config(&beads_dir, Some(&storage), &CliOverrides::default())
            .map_err(|e| anyhow::anyhow!("config load failed: {e}"))?;
        let lock = blocking_write_lock(&beads_dir).ok();
        info!(?beads_dir, prefix, "Initialised new beads project");
        Ok(BeadsProject {
            beads_dir,
            jsonl_path,
            config,
            _write_lock: lock,
            storage: Mutex::new(storage),
        })
    }

    pub fn flush(&self) -> Result<()> {
        let mut storage = self.storage.lock().unwrap();
        sync::auto_flush(
            &mut storage,
            &self.beads_dir,
            &self.jsonl_path,
            false,
            HistoryConfig::default(),
        )
        .map_err(|e| anyhow::anyhow!("JSONL flush failed: {e}"))?;
        Ok(())
    }

    pub fn storage(&self) -> std::sync::MutexGuard<'_, SqliteStorage> {
        self.storage.lock().unwrap()
    }

    pub fn storage_mut(&self) -> std::sync::MutexGuard<'_, SqliteStorage> {
        self.storage.lock().unwrap()
    }

    pub fn beads_dir(&self) -> &Path {
        &self.beads_dir
    }
}
