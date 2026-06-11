use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::Result;
use ffs_search::{
    FfsMode, FilePicker, FilePickerOptions, FuzzySearchOptions, PaginationArgs, QueryParser,
    SharedFilePicker, SharedFrecency,
};

struct PickerSlot {
    root: PathBuf,
    picker: SharedFilePicker,
}

static PICKER: OnceLock<Mutex<Option<PickerSlot>>> = OnceLock::new();

fn cell() -> &'static Mutex<Option<PickerSlot>> {
    PICKER.get_or_init(|| Mutex::new(None))
}

fn get_or_init(root: &Path) -> Result<SharedFilePicker> {
    let mut guard = cell().lock().expect("ffs picker lock");
    #[allow(clippy::collapsible_if)]
    if let Some(slot) = guard.as_ref() {
        if slot.root == root {
            return Ok(slot.picker.clone());
        }
    }

    let shared = SharedFilePicker::default();
    let opts = FilePickerOptions {
        base_path: root.to_string_lossy().into_owned(),
        mode: FfsMode::Ai,
        enable_mmap_cache: false,
        enable_content_indexing: false,
        cache_budget: None,
        watch: true,
        follow_symlinks: false,
    };
    FilePicker::new_with_shared_state(shared.clone(), SharedFrecency::default(), opts)?;
    let _ = shared.wait_for_scan(Duration::from_secs(120));

    *guard = Some(PickerSlot {
        root: root.to_path_buf(),
        picker: shared.clone(),
    });
    Ok(shared)
}

/// Run `f` with a warm file picker for `root`.
pub fn with_file_picker<T, F>(root: &Path, f: F) -> Result<T>
where
    F: FnOnce(&FilePicker) -> Result<T>,
{
    let shared = get_or_init(root)?;
    let guard = shared.read().map_err(|e| anyhow::anyhow!("{e}"))?;
    let picker = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("file picker scan not ready"))?;
    f(picker)
}

/// Fuzzy file find via `FilePicker` (ffs-search API).
pub fn find_files(root: &Path, needle: &str, limit: usize) -> Result<Vec<String>> {
    with_file_picker(root, |picker| {
        let parser = QueryParser::default();
        let query = parser.parse(needle);
        let results = picker.fuzzy_search(
            &query,
            None,
            FuzzySearchOptions {
                pagination: PaginationArgs { offset: 0, limit },
                ..Default::default()
            },
        );
        Ok(results
            .items
            .iter()
            .map(|item| item.relative_path(picker))
            .collect())
    })
}
