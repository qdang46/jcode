// =====================================================================
// MemoryProvider trait — abstract memory backend
// =====================================================================
//
// This trait defines the common interface for all memory backends in
// jcode. Both the legacy `MemoryManager` (JSON-based MemoryGraph) and
// the `MempalaceAdapter` (SQLite + vector search) implement this trait,
// allowing the rest of the system to be backend-agnostic.
//
// # Design decisions
//
// - Methods are async to accommodate both sync (MemoryManager) and
//   async-first (MempalaceAdapter/Palace) backends. The MemoryManager
//   impl wraps its sync calls in tokio::task::spawn_blocking or
//   tokio::task::block_in_place as needed.
// - All methods take `&self` — implementations are expected to be
//   thread-safe (Send + Sync) and carry their own interior mutability
//   or Arc-wrapped state.
// - Return types use `Vec<MemoryEntry>` rather than raw (text, score)
//   tuples so callers can inspect all metadata (tags, category, trust,
//   confidence, timestamps).

use crate::{MemoryEntry, MemoryGraph, MemoryScope};
use anyhow::Result;
use async_trait::async_trait;

/// Abstract memory backend.
///
/// Implementations:
/// - `MemoryManager` (jcode-base) — legacy JSON-based storage
/// - `MempalaceAdapter` (jcode-mempalace-adapter) — SQLite + vector search
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    /// Store a new memory entry.
    async fn remember(
        &self,
        entry: MemoryEntry,
        scope: MemoryScope,
    ) -> Result<String>;

    /// Recall memories matching a query.
    ///
    /// `mode` controls retrieval strategy:
    /// - `"recent"` — most recently created/updated memories
    /// - `"semantic"` — embedding-based similarity search
    /// - `"cascade"` — semantic + graph traversal expansion
    async fn recall(
        &self,
        query: &str,
        scope: MemoryScope,
        limit: usize,
        mode: &str,
    ) -> Result<Vec<(MemoryEntry, f32)>>;

    /// Search memories by keyword/text match.
    async fn search(
        &self,
        query: &str,
        scope: MemoryScope,
    ) -> Result<Vec<MemoryEntry>>;

    /// List all memories in the given scope.
    async fn list_all(
        &self,
        scope: MemoryScope,
    ) -> Result<Vec<MemoryEntry>>;

    /// Remove a memory by ID.
    async fn forget(&self, id: &str) -> Result<bool>;

    /// Add a tag to a memory.
    async fn tag(&self, id: &str, tag: &str) -> Result<()>;

    /// Create a weighted link between two memories.
    async fn link(&self, from_id: &str, to_id: &str, weight: f32) -> Result<()>;

    /// Get memories related to the given memory via graph traversal.
    async fn related(
        &self,
        id: &str,
        depth: usize,
    ) -> Result<Vec<MemoryEntry>>;

    /// Get memories formatted for prompt injection.
    async fn get_prompt_memories(
        &self,
        limit: usize,
        scope: MemoryScope,
    ) -> Result<Vec<MemoryEntry>>;

    /// Get graph statistics: (total_memories, total_tags, total_edges, total_clusters).
    async fn graph_stats(&self) -> Result<(usize, usize, usize, usize)> {
        let all = self.list_all(MemoryScope::All).await?;
        let count = all.len();
        let tags = all.iter().flat_map(|e| e.tags.iter()).count();
        Ok((count, tags, 0, 0))
    }

    /// Load all memory entries across both project and global scopes.
    async fn load_all_entries(&self) -> Result<Vec<MemoryEntry>> {
        self.list_all(MemoryScope::All).await
    }
}

/// Trait for graph-level memory operations (load/save full MemoryGraph).
///
/// Separated from `MemoryProvider` because some backends (e.g. MempalaceAdapter)
/// do not expose raw graph persistence. Default implementations return a
/// "not supported" error, allowing each backend to opt in.
#[async_trait]
pub trait GraphOperations: Send + Sync {
    /// Load the project-scoped memory graph.
    async fn load_project_graph(&self) -> Result<MemoryGraph> {
        anyhow::bail!("graph operations not supported by this backend")
    }

    /// Load the global-scoped memory graph.
    async fn load_global_graph(&self) -> Result<MemoryGraph> {
        anyhow::bail!("graph operations not supported by this backend")
    }

    /// Save the project-scoped memory graph.
    async fn save_project_graph(&self, _graph: &MemoryGraph) -> Result<()> {
        anyhow::bail!("graph operations not supported by this backend")
    }

    /// Save the global-scoped memory graph.
    async fn save_global_graph(&self, _graph: &MemoryGraph) -> Result<()> {
        anyhow::bail!("graph operations not supported by this backend")
    }
}
