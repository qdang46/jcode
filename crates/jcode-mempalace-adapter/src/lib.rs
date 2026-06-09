// =====================================================================
// jcode-mempalace-adapter — bridge jcode's MemoryEntry ↔ mempalace's Drawer
// =====================================================================
//
// This crate provides the **type-conversion layer** between jcode's
// `MemoryEntry` / `MemoryCategory` / `MemoryScope` and mempalace's
// `Drawer` / `DrawerKind` / `MemoryScope`.
//
// Without the "backend" feature, it defines local mirror types
// (`Drawer`, `DrawerKind`, `DrawerId`, `MemoryScope`) that match
// mempalace's public surface exactly — zero dependency on mempalace-core.
//
// With `features = ["backend"]`, the crate pulls in `mempalace-core`
// (rusqlite 0.33, now aligned) and provides:
// - `MempalaceAdapter` — runtime wrapper around `Palace` implementing
//   the 8 memory-tool actions (remember/recall/search/list/forget/tag/link/related)
// - `migrate::migrate_to_mempalace` — data migration from jcode JSON
//   MemoryGraph files to mempalace Drawer + KG format
//
// # Issues implemented
//
// - #355: Type-conversion layer (mirror types, round-trip conversion)
// - #356: Data migration tool (`migrate` module, behind "backend")
// - #357: MempalaceAdapter for MemoryTool dispatch (behind "backend")
// - #358: MempalaceAdapter exposes palace for prompt injection (behind "backend")

pub mod convert;

// Coordination adapter — bridges mempalace's coordination module into jcode.
#[cfg(feature = "backend")]
pub mod coordination;

// Migration tool — only available when the full mempalace runtime is linked.
#[cfg(feature = "backend")]
pub mod migrate;

// Re-export key mempalace-core types for downstream consumers.
#[cfg(feature = "backend")]
pub use mempalace_core;
#[cfg(feature = "backend")]
pub use mempalace_core::{
    Drawer as MpDrawer, DrawerId as MpDrawerId, DrawerKind as MpDrawerKind, Embedder,
    MemoryProvider, Palace, PalaceBuilder, PalaceConfig, SearchHit, SearchScope,
};

/// Convert the mirror `Drawer` to a real `mempalace_core::Drawer`.
///
/// This is needed when the `backend` feature is enabled because the
/// conversion functions in `convert.rs` produce mirror types, but the
/// Palace runtime needs real types.
#[cfg(feature = "backend")]
pub fn mirror_drawer_to_real(drawer: &Drawer) -> MpDrawer {
    let kind = match &drawer.kind {
        DrawerKind::Fact => MpDrawerKind::Fact,
        DrawerKind::Event => MpDrawerKind::Event,
        DrawerKind::Discovery => MpDrawerKind::Discovery,
        DrawerKind::Preference => MpDrawerKind::Preference,
        DrawerKind::Advice => MpDrawerKind::Advice,
        DrawerKind::Raw => MpDrawerKind::Raw,
        DrawerKind::Entity => MpDrawerKind::Entity,
        DrawerKind::Correction => MpDrawerKind::Correction,
        DrawerKind::Custom(s) => MpDrawerKind::Custom(s.clone()),
    };

    let mut real = MpDrawer::new(&drawer.content);
    real.id = drawer.id.as_ref().map(|id| MpDrawerId::new(&id.0));
    real.kind = kind;
    real.tags = drawer.tags.clone();
    real.metadata = drawer.metadata.clone();
    real.created_at = drawer.created_at;
    real.updated_at = drawer.updated_at;
    real.active = drawer.active;
    real.trust = drawer.trust.clone();
    real.access_count = drawer.access_count;
    real.superseded_by = drawer
        .superseded_by
        .as_ref()
        .map(|id| MpDrawerId::new(&id.0));
    real.reinforcements = drawer
        .reinforcements
        .iter()
        .map(|r| mempalace_core::palace::Reinforcement {
            session_id: r.session_id.clone(),
            message_index: r.message_index,
            timestamp: r.timestamp,
        })
        .collect();
    real.confidence = drawer.confidence;
    real.consolidation_strength = drawer.consolidation_strength;
    real.derived_from = drawer
        .derived_from
        .iter()
        .map(|id| MpDrawerId::new(&id.0))
        .collect();
    real
}

// Re-export mirror types at crate root for ergonomic imports.
pub use convert::{
    Drawer, DrawerId, DrawerKind, MemoryScope, MpReinforcement, category_to_kind,
    drawer_to_memory_entry, jcode_scope_from_mp, kind_to_category, memory_entry_to_drawer,
    mp_scope_from_jcode, string_to_trust, trust_to_string,
};

// =====================================================================
// MempalaceAdapter — runtime bridge (feature = "backend")
// =====================================================================
//
// Wraps a `Palace` instance and exposes the 8 memory-tool actions that
// jcode's `MemoryTool` dispatches to.  Also provides access to the
// underlying `Palace` for the per-turn prompt injection pipeline (#358).

#[cfg(feature = "backend")]
pub struct MempalaceAdapter {
    palace: mempalace_core::Palace,
}

#[cfg(feature = "backend")]
impl MempalaceAdapter {
    /// Open a mempalace at the given path and wrap it in an adapter.
    pub async fn open(palace_path: &std::path::Path) -> anyhow::Result<Self> {
        use mempalace_core::{Embedder, PalaceBuilder, PalaceConfig};

        let mut config = PalaceConfig::default();
        config.palace_path = palace_path.to_path_buf();

        let embedder: std::sync::Arc<dyn Embedder> = match mempalace_core::embedder_from_env() {
            Ok(boxed) => std::sync::Arc::from(boxed),
            Err(_) => std::sync::Arc::new(mempalace_core::NullEmbedder::new(384)),
        };

        let palace = PalaceBuilder::new()
            .config(config)
            .embedder(embedder)
            .open()
            .await?;

        Ok(Self { palace })
    }

    /// Borrow the underlying Palace for prompt injection / search.
    pub fn palace(&self) -> &mempalace_core::Palace {
        &self.palace
    }

    /// "remember" — file a new memory as a Drawer.
    pub async fn remember(
        &self,
        content: &str,
        category: &jcode_memory_types::MemoryCategory,
        tags: &[String],
        scope: jcode_memory_types::MemoryScope,
        source: Option<&str>,
    ) -> anyhow::Result<String> {
        use mempalace_core::{Drawer as MpDrawer, DrawerKind as MpKind, MemoryProvider};

        let kind = match category {
            jcode_memory_types::MemoryCategory::Fact => MpKind::Fact,
            jcode_memory_types::MemoryCategory::Preference => MpKind::Preference,
            jcode_memory_types::MemoryCategory::Entity => MpKind::Entity,
            jcode_memory_types::MemoryCategory::Correction => MpKind::Correction,
            jcode_memory_types::MemoryCategory::Custom(s) => MpKind::Custom(s.clone()),
        };

        let mut drawer = MpDrawer::new(content);
        drawer.kind = kind;
        drawer.tags = tags.to_vec();
        if let Some(src) = source {
            drawer
                .metadata
                .insert("source".to_string(), serde_json::json!(src));
        }
        let wing = match scope {
            jcode_memory_types::MemoryScope::Project => Some("project".to_string()),
            jcode_memory_types::MemoryScope::Global => None,
            jcode_memory_types::MemoryScope::All => None,
        };
        drawer.wing = wing;

        let id = self.palace.add_drawer(drawer).await?;
        Ok(id.to_string())
    }

    /// "search" — natural-language search via Palace.
    pub async fn search(
        &self,
        query: &str,
        scope: jcode_memory_types::MemoryScope,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, f64)>> {
        use mempalace_core::{MemoryProvider, SearchScope};

        let search_scope = match scope {
            jcode_memory_types::MemoryScope::Project => {
                SearchScope::new().wing("project").limit(limit)
            }
            _ => SearchScope::new().limit(limit),
        };

        let hits = self.palace.search(query, &search_scope).await?;
        Ok(hits.into_iter().map(|h| (h.text, h.similarity)).collect())
    }

    /// "forget" — remove a drawer by ID.
    pub async fn forget(&self, id: &str) -> anyhow::Result<bool> {
        use mempalace_core::{DrawerId as MpDrawerId, MemoryProvider};
        let found = self.palace.forget(&MpDrawerId::new(id)).await?;
        Ok(found)
    }

    /// "tag" — add tags to a drawer.
    pub async fn tag(&self, id: &str, tags: &[String]) -> anyhow::Result<()> {
        use mempalace_core::{DrawerId as MpDrawerId, MemoryProvider};
        for tag in tags {
            self.palace.tag(&MpDrawerId::new(id), tag).await?;
        }
        Ok(())
    }

    /// "link" — create a typed edge between two drawers.
    pub async fn link(&self, from_id: &str, to_id: &str, weight: f32) -> anyhow::Result<()> {
        use mempalace_core::{DrawerId as MpDrawerId, MemoryProvider};
        self.palace
            .link(&MpDrawerId::new(from_id), &MpDrawerId::new(to_id), weight)
            .await
    }

    /// "related" — get related drawers via KG traversal.
    pub async fn related(&self, id: &str, depth: usize) -> anyhow::Result<Vec<(String, f64)>> {
        use mempalace_core::{DrawerId as MpDrawerId, MemoryProvider};
        let hits = self.palace.related(&MpDrawerId::new(id), depth).await?;
        Ok(hits.into_iter().map(|h| (h.text, h.similarity)).collect())
    }

    /// "list" — get all drawers matching a scope.
    pub async fn list_all(
        &self,
        scope: jcode_memory_types::MemoryScope,
    ) -> anyhow::Result<Vec<(String, String, String)>> {
        use mempalace_core::{MemoryProvider, SearchScope};

        let search_scope = match scope {
            jcode_memory_types::MemoryScope::Project => SearchScope::new().wing("project"),
            _ => SearchScope::new(),
        };

        // Use a broad search to list everything
        let hits = self.palace.search("", &search_scope).await?;
        Ok(hits
            .into_iter()
            .map(|h| {
                let kind = h.wing.clone().unwrap_or_default();
                (h.text, kind, String::new())
            })
            .collect())
    }

    /// "recall" — semantic or cascade search.
    pub async fn recall(
        &self,
        query: &str,
        scope: jcode_memory_types::MemoryScope,
        limit: usize,
        mode: &str,
    ) -> anyhow::Result<Vec<(String, f64)>> {
        use mempalace_core::{MemoryProvider, SearchScope};

        let search_scope = match scope {
            jcode_memory_types::MemoryScope::Project => {
                SearchScope::new().wing("project").limit(limit)
            }
            _ => SearchScope::new().limit(limit),
        };

        if mode == "cascade" {
            let hits = self
                .palace
                .cascade_search(query, &search_scope, 2, limit)
                .await?;
            Ok(hits.into_iter().map(|h| (h.text, h.similarity)).collect())
        } else {
            let hits = self.palace.search(query, &search_scope).await?;
            Ok(hits.into_iter().map(|h| (h.text, h.similarity)).collect())
        }
    }
}

// =====================================================================
// MemoryProvider trait implementation (behind "backend")
// =====================================================================

#[cfg(feature = "backend")]
#[async_trait::async_trait]
impl jcode_memory_types::MemoryProvider for MempalaceAdapter {
    async fn remember(
        &self,
        entry: jcode_memory_types::MemoryEntry,
        scope: jcode_memory_types::MemoryScope,
    ) -> anyhow::Result<String> {
        let category = &entry.category;
        let tags = entry.tags.clone();
        let source = entry.source.as_deref();
        let content = entry.content.clone();
        self.remember(&content, category, &tags, scope, source).await
    }

    async fn recall(
        &self,
        query: &str,
        scope: jcode_memory_types::MemoryScope,
        limit: usize,
        mode: &str,
    ) -> anyhow::Result<Vec<(jcode_memory_types::MemoryEntry, f32)>> {
        use mempalace_core::{MemoryProvider as MpProvider, SearchScope};

        let search_scope = match scope {
            jcode_memory_types::MemoryScope::Project => {
                SearchScope::new().wing("project").limit(limit)
            }
            _ => SearchScope::new().limit(limit),
        };

        let hits = if mode == "cascade" {
            self.palace
                .cascade_search(query, &search_scope, 2, limit)
                .await?
        } else {
            MpProvider::search(&self.palace, query, &search_scope).await?
        };

        let entries: Vec<_> = hits
            .into_iter()
            .map(|h| {
                let mut entry = jcode_memory_types::MemoryEntry::new(
                    jcode_memory_types::MemoryCategory::Fact,
                    &h.text,
                );
                entry.id = format!("mp-{}", uuid::Uuid::new_v4());
                (entry, h.similarity as f32)
            })
            .collect();
        Ok(entries)
    }

    async fn search(
        &self,
        query: &str,
        scope: jcode_memory_types::MemoryScope,
    ) -> anyhow::Result<Vec<jcode_memory_types::MemoryEntry>> {
        use mempalace_core::{MemoryProvider as MpProvider, SearchScope};

        let search_scope = match scope {
            jcode_memory_types::MemoryScope::Project => SearchScope::new().wing("project"),
            _ => SearchScope::new(),
        };

        let hits = MpProvider::search(&self.palace, query, &search_scope).await?;
        let entries = hits
            .into_iter()
            .map(|h| {
                let mut entry = jcode_memory_types::MemoryEntry::new(
                    jcode_memory_types::MemoryCategory::Fact,
                    &h.text,
                );
                entry.id = format!("mp-{}", uuid::Uuid::new_v4());
                entry
            })
            .collect();
        Ok(entries)
    }

    async fn list_all(
        &self,
        scope: jcode_memory_types::MemoryScope,
    ) -> anyhow::Result<Vec<jcode_memory_types::MemoryEntry>> {
        let results = MempalaceAdapter::list_all(self, scope).await?;
        let entries = results
            .into_iter()
            .map(|(text, _kind, _extra)| {
                let mut entry = jcode_memory_types::MemoryEntry::new(
                    jcode_memory_types::MemoryCategory::Fact,
                    &text,
                );
                entry.id = format!("mp-{}", uuid::Uuid::new_v4());
                entry
            })
            .collect();
        Ok(entries)
    }

    async fn forget(&self, id: &str) -> anyhow::Result<bool> {
        self.forget(id).await
    }

    async fn tag(&self, id: &str, tag: &str) -> anyhow::Result<()> {
        self.tag(id, &[tag.to_string()]).await
    }

    async fn link(&self, from_id: &str, to_id: &str, weight: f32) -> anyhow::Result<()> {
        self.link(from_id, to_id, weight).await
    }

    async fn related(
        &self,
        id: &str,
        depth: usize,
    ) -> anyhow::Result<Vec<jcode_memory_types::MemoryEntry>> {
        let results = self.related(id, depth).await?;
        let entries = results
            .into_iter()
            .map(|(text, _score)| {
                let mut entry = jcode_memory_types::MemoryEntry::new(
                    jcode_memory_types::MemoryCategory::Fact,
                    &text,
                );
                entry.id = format!("mp-{}", uuid::Uuid::new_v4());
                entry
            })
            .collect();
        Ok(entries)
    }

    async fn get_prompt_memories(
        &self,
        limit: usize,
        scope: jcode_memory_types::MemoryScope,
    ) -> anyhow::Result<Vec<jcode_memory_types::MemoryEntry>> {
        let results = MempalaceAdapter::list_all(self, scope).await?;
        let entries = results
            .into_iter()
            .take(limit)
            .map(|(text, _kind, _extra)| {
                let mut entry = jcode_memory_types::MemoryEntry::new(
                    jcode_memory_types::MemoryCategory::Fact,
                    &text,
                );
                entry.id = format!("mp-{}", uuid::Uuid::new_v4());
                entry
            })
            .collect();
        Ok(entries)
    }
}

// =====================================================================
// GraphOperations trait implementation (behind "backend")
// =====================================================================
//
// Loads all drawers from the Palace via search/list_all and constructs
// a MemoryGraph. This is less efficient than the MemoryManager's direct
// graph access but maintains API compatibility.

#[cfg(feature = "backend")]
#[async_trait::async_trait]
impl jcode_memory_types::GraphOperations for MempalaceAdapter {
    async fn load_project_graph(&self) -> anyhow::Result<jcode_memory_types::MemoryGraph> {
        let entries = MempalaceAdapter::list_all(
            self,
            jcode_memory_types::MemoryScope::Project,
        )
        .await?;
        let mut graph = jcode_memory_types::MemoryGraph::new();
        for (text, _kind, _extra) in entries {
            let entry = jcode_memory_types::MemoryEntry::new(
                jcode_memory_types::MemoryCategory::Fact,
                &text,
            );
            graph.add_memory(entry);
        }
        Ok(graph)
    }

    async fn load_global_graph(&self) -> anyhow::Result<jcode_memory_types::MemoryGraph> {
        let entries = MempalaceAdapter::list_all(
            self,
            jcode_memory_types::MemoryScope::Global,
        )
        .await?;
        let mut graph = jcode_memory_types::MemoryGraph::new();
        for (text, _kind, _extra) in entries {
            let entry = jcode_memory_types::MemoryEntry::new(
                jcode_memory_types::MemoryCategory::Fact,
                &text,
            );
            graph.add_memory(entry);
        }
        Ok(graph)
    }

    async fn save_project_graph(
        &self,
        _graph: &jcode_memory_types::MemoryGraph,
    ) -> anyhow::Result<()> {
        anyhow::bail!("save_project_graph not supported by MempalaceAdapter; memories are stored as Drawers in the Palace")
    }

    async fn save_global_graph(
        &self,
        _graph: &jcode_memory_types::MemoryGraph,
    ) -> anyhow::Result<()> {
        anyhow::bail!("save_global_graph not supported by MempalaceAdapter; memories are stored as Drawers in the Palace")
    }
}

// ---- tests ------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::convert::*;
    use jcode_memory_types::{MemoryCategory, MemoryEntry, MemoryScope as JcodeScope, TrustLevel};

    fn test_entry(content: &str, category: MemoryCategory) -> MemoryEntry {
        MemoryEntry {
            id: "mem-test".to_string(),
            category,
            content: content.to_string(),
            tags: vec!["test".to_string()],
            search_text: content.to_lowercase(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            access_count: 0,
            source: Some("test".to_string()),
            trust: TrustLevel::Medium,
            strength: 1,
            active: true,
            superseded_by: None,
            reinforcements: vec![],
            embedding: None,
            confidence: 1.0,
        }
    }

    #[test]
    fn round_trip_conversion_preserves_content() {
        let original = test_entry("use Rust for memory", MemoryCategory::Fact);
        let drawer = memory_entry_to_drawer(&original, JcodeScope::Project);
        let back = drawer_to_memory_entry(&drawer);
        assert_eq!(back.content, original.content);
        assert_eq!(back.category, original.category);
        assert_eq!(back.tags, original.tags);
        assert_eq!(back.confidence, original.confidence);
        assert_eq!(back.active, original.active);
    }

    #[test]
    fn category_to_drawer_kind_maps_correctly() {
        assert_eq!(category_to_kind(&MemoryCategory::Fact), DrawerKind::Fact);
        assert_eq!(
            category_to_kind(&MemoryCategory::Preference),
            DrawerKind::Preference
        );
        assert_eq!(
            category_to_kind(&MemoryCategory::Entity),
            DrawerKind::Entity
        );
        assert_eq!(
            category_to_kind(&MemoryCategory::Correction),
            DrawerKind::Correction
        );
        assert_eq!(
            category_to_kind(&MemoryCategory::Custom("snippet".into())),
            DrawerKind::Custom("snippet".into())
        );
    }

    #[test]
    fn kind_to_category_maps_correctly() {
        assert_eq!(kind_to_category(&DrawerKind::Fact), MemoryCategory::Fact);
        assert_eq!(
            kind_to_category(&DrawerKind::Preference),
            MemoryCategory::Preference
        );
        assert_eq!(
            kind_to_category(&DrawerKind::Entity),
            MemoryCategory::Entity
        );
        assert_eq!(
            kind_to_category(&DrawerKind::Correction),
            MemoryCategory::Correction
        );
        assert_eq!(
            kind_to_category(&DrawerKind::Custom("ref".into())),
            MemoryCategory::Custom("ref".into())
        );
        // Non-jcode kinds map to Fact as a safe default.
        assert_eq!(kind_to_category(&DrawerKind::Event), MemoryCategory::Fact);
        assert_eq!(
            kind_to_category(&DrawerKind::Discovery),
            MemoryCategory::Fact
        );
        assert_eq!(kind_to_category(&DrawerKind::Advice), MemoryCategory::Fact);
        assert_eq!(kind_to_category(&DrawerKind::Raw), MemoryCategory::Fact);
    }

    #[test]
    fn scope_conversion_round_trips() {
        let pairs = [
            (JcodeScope::Project, MemoryScope::Local),
            (JcodeScope::Global, MemoryScope::Global),
            (JcodeScope::All, MemoryScope::All),
        ];
        for (jcode, mp) in &pairs {
            assert_eq!(mp_scope_from_jcode(jcode.clone()), mp.clone());
            assert_eq!(jcode_scope_from_mp(mp), jcode.clone());
        }
    }

    #[test]
    fn drawer_builder_sets_defaults() {
        let d = Drawer::new("hello");
        assert_eq!(d.content, "hello");
        assert_eq!(d.kind, DrawerKind::Raw);
        assert!(d.active);
        assert!((d.confidence - 1.0).abs() < 0.01);
        assert_eq!(d.consolidation_strength, 1);
        assert!(d.tags.is_empty());
    }

    #[test]
    fn half_life_days_matches_jcode() {
        assert!((DrawerKind::Correction.half_life_days() - 365.0).abs() < 0.01);
        assert!((DrawerKind::Preference.half_life_days() - 90.0).abs() < 0.01);
        assert!((DrawerKind::Entity.half_life_days() - 60.0).abs() < 0.01);
        assert!((DrawerKind::Fact.half_life_days() - 30.0).abs() < 0.01);
        assert!((DrawerKind::Custom("x".into()).half_life_days() - 45.0).abs() < 0.01);
    }
}
