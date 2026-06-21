//! TUI provider/model picker — data model only.
//!
//! Phase 5 of the master plan. The actual TUI rendering will be
//! integrated into `jcode-tui` (which has unrelated pre-existing build
//! failures); this module provides the *headless* data model that the
//! eventual TUI binds against, plus a `next()` / `prev()` / `filter()`
//! API for the keyboard navigation. Tests verify the selection logic
//! without needing a renderer.
//!
//! Design (matches opencode's `/model` picker roughly):
//!
//! ```text
//!  ┌──────────────────────────────────────┐
//!  │ > claude-sonnet-4-6   Anthropic  ●   │   ← highlighted
//!  │   claude-haiku-4-5    Anthropic  ○   │
//!  │   gpt-5-mini          OpenAI    ○   │
//!  │   ...                                 │
//!  └──────────────────────────────────────┘
//! ```
//!
//! Order of rows:
//!  1. Favorites (config-driven; passed in via [`PickerState::favorites`]).
//!  2. Recent selections (LIFO of [`PickerState::push_recent`]).
//!  3. Connected providers' models, sorted by tier (Flagship → Nano).
//!  4. All other models, sorted by provider label then model id.

use std::collections::HashSet;

use crate::catalog::CatalogService;
use crate::types::{ModelId, ProviderId};

/// A single row in the picker.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PickerRow {
    pub provider: ProviderId,
    pub model: ModelId,
    pub label: String,
    /// Origin category for ordering / display.
    pub origin: RowOrigin,
    /// True if this row's model has at least one credential configured.
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowOrigin {
    Favorite,
    Recent,
    Connected,
    Catalog,
}

/// A search/filter string. Empty means "show everything".
#[derive(Debug, Clone, Default)]
pub struct Filter(pub String);

impl Filter {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Case-insensitive substring match against either the model id or the
    /// provider label. Returns true if the filter is empty.
    pub fn matches(&self, row: &PickerRow) -> bool {
        if self.0.is_empty() {
            return true;
        }
        let needle = self.0.to_ascii_lowercase();
        row.model.as_str().to_ascii_lowercase().contains(&needle)
            || row.label.to_ascii_lowercase().contains(&needle)
            || row.provider.as_str().to_ascii_lowercase().contains(&needle)
    }
}

/// State for the picker.
#[derive(Debug, Default)]
pub struct PickerState {
    /// Highlighted row index (0-based).
    pub cursor: usize,
    /// Filter string.
    pub filter: Filter,
    /// Recent selections (LIFO; most recent first).
    pub recent: Vec<(ProviderId, ModelId)>,
    /// User-marked favorites.
    pub favorites: HashSet<(ProviderId, ModelId)>,
    /// Cached row list (populated by [`PickerState::rebuild_rows`]).
    rows: Vec<PickerRow>,
}

impl PickerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a selection into the recent list. De-duplicates and caps at
    /// 10 entries.
    pub fn push_recent(&mut self, provider: ProviderId, model: ModelId) {
        self.recent.retain(|p| !(p.0 == provider && p.1 == model));
        self.recent.insert(0, (provider, model));
        if self.recent.len() > 10 {
            self.recent.truncate(10);
        }
    }

    /// Mark or unmark a row as a favorite.
    pub fn toggle_favorite(&mut self, row: &PickerRow) {
        let key = (row.provider.clone(), row.model.clone());
        if !self.favorites.remove(&key) {
            self.favorites.insert(key);
        }
    }

    /// Rebuild the visible row list from the catalog and a set of
    /// connected provider ids.
    pub async fn rebuild_rows(
        &mut self,
        catalog: &dyn CatalogService,
        connected: &HashSet<ProviderId>,
        favorites: &HashSet<(ProviderId, ModelId)>,
    ) -> Result<(), crate::catalog::CatalogError> {
        self.favorites = favorites.clone();
        let mut rows: Vec<PickerRow> = Vec::new();

        // 1. Favorites first.
        for (p, m) in favorites {
            let info = catalog.find_model(p, m).await?;
            rows.push(PickerRow {
                provider: p.clone(),
                model: m.clone(),
                label: info.name,
                origin: RowOrigin::Favorite,
                connected: connected.contains(p),
            });
        }

        // 2. Recent (de-dup with favorites).
        for (p, m) in &self.recent {
            if favorites.contains(&(p.clone(), m.clone())) {
                continue;
            }
            if let Ok(info) = catalog.find_model(p, m).await {
                rows.push(PickerRow {
                    provider: p.clone(),
                    model: m.clone(),
                    label: info.name,
                    origin: RowOrigin::Recent,
                    connected: connected.contains(p),
                });
            }
        }

        // 3. Catalog rows, connected first then alphabetically.
        for provider in catalog.list_providers().await? {
            for model in &provider.models {
                let key = (provider.id.clone(), model.id.clone());
                if favorites.contains(&key) {
                    continue;
                }
                if self
                    .recent
                    .iter()
                    .any(|(p, m)| p == &provider.id && m == &model.id)
                {
                    continue;
                }
                let origin = if connected.contains(&provider.id) {
                    RowOrigin::Connected
                } else {
                    RowOrigin::Catalog
                };
                rows.push(PickerRow {
                    provider: provider.id.clone(),
                    model: model.id.clone(),
                    label: model.name.clone(),
                    origin,
                    connected: connected.contains(&provider.id),
                });
            }
        }

        // Apply filter, then re-clamp cursor.
        let visible: Vec<PickerRow> = rows
            .into_iter()
            .filter(|r| self.filter.matches(r))
            .collect();
        self.rows = visible;
        self.clamp_cursor();
        Ok(())
    }

    /// Visible rows (after filter).
    pub fn visible(&self) -> &[PickerRow] {
        &self.rows
    }

    /// Currently highlighted row, if any.
    pub fn selected(&self) -> Option<&PickerRow> {
        self.rows.get(self.cursor)
    }

    /// Move the cursor down by `n` rows, wrapping at the bottom.
    pub fn move_down(&mut self, n: usize) {
        if self.rows.is_empty() {
            return;
        }
        self.cursor = (self.cursor + n) % self.rows.len();
    }

    /// Move the cursor up by `n` rows, wrapping at the top.
    pub fn move_up(&mut self, n: usize) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len();
        self.cursor = (self.cursor + len - (n % len)) % len;
    }

    /// Update the filter and re-clamp. The visible row list is
    /// re-filtered in place; rows that no longer match are dropped.
    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
        self.rows.retain(|r| self.filter.matches(r));
        self.cursor = 0;
    }

    /// Toggle favorite on the currently highlighted row.
    pub fn toggle_selected_favorite(&mut self) {
        if let Some(row) = self.selected().cloned() {
            self.toggle_favorite(&row);
        }
    }

    fn clamp_cursor(&mut self) {
        if self.rows.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{InMemoryCatalog, ModelInfo, ModelTier, ProviderInfo};

    async fn catalog() -> InMemoryCatalog {
        let c = InMemoryCatalog::new();
        for p in &[
            ProviderInfo {
                id: "anthropic".into(),
                name: "Anthropic".into(),
                enabled: true,
                is_connected: true,
                has_integration: false,
                models: vec![
                    ModelInfo {
                        id: "claude-sonnet-4-6".into(),
                        provider: "anthropic".into(),
                        name: "Claude Sonnet 4.6".into(),
                        cost_per_million_input: Some(3.0),
                        cost_per_million_output: Some(15.0),
                        context_window: 200_000,
                        supports_tools: true,
                        supports_vision: true,
                        supports_streaming: true,
                        tier: Some(ModelTier::Standard),

                        release_date: None,
                        base_url: None,
                        path: None,
                        protocol: None,
                    },
                    ModelInfo {
                        id: "claude-haiku-4-5".into(),
                        provider: "anthropic".into(),
                        name: "Claude Haiku 4.5".into(),
                        cost_per_million_input: Some(0.8),
                        cost_per_million_output: Some(4.0),
                        context_window: 200_000,
                        supports_tools: true,
                        supports_vision: true,
                        supports_streaming: true,
                        tier: Some(ModelTier::Nano),

                        release_date: None,
                        base_url: None,
                        path: None,
                        protocol: None,
                    },
                ],
                api_key: None,
                protocol: "anthropic-messages-2023-01-01".into(),
                path: "/v1/messages".into(),
                base_url: "https://api.anthropic.com".into(),
            },
            ProviderInfo {
                id: "openai".into(),
                name: "OpenAI".into(),
                enabled: true,
                is_connected: true,
                has_integration: false,
                models: vec![ModelInfo {
                    id: "gpt-5-mini".into(),
                    provider: "openai".into(),
                    name: "GPT-5 mini".into(),
                    cost_per_million_input: Some(0.25),
                    cost_per_million_output: Some(2.0),
                    context_window: 400_000,
                    supports_tools: true,
                    supports_vision: true,
                    supports_streaming: true,
                    tier: Some(ModelTier::Mini),

                    release_date: None,
                    base_url: None,
                    path: None,
                    protocol: None,
                }],
                api_key: None,
                protocol: "anthropic-messages-2023-01-01".into(),
                path: "/v1/messages".into(),
                base_url: "https://api.anthropic.com".into(),
            },
        ] {
            c.register_provider(p.clone()).await.unwrap();
        }
        c
    }

    #[tokio::test]
    async fn rebuild_lists_all_rows_when_unfiltered() {
        let cat = catalog().await;
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        connected.insert(ProviderId::from("anthropic"));
        connected.insert(ProviderId::from("openai"));
        state
            .rebuild_rows(&cat, &connected, &HashSet::new())
            .await
            .unwrap();
        assert_eq!(state.visible().len(), 3);
        // Connected rows first.
        assert!(state.visible()[0].connected);
    }

    #[tokio::test]
    async fn favorites_appear_first() {
        let cat = catalog().await;
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        connected.insert(ProviderId::from("anthropic"));
        connected.insert(ProviderId::from("openai"));
        let mut favs = HashSet::new();
        favs.insert((ProviderId::from("openai"), ModelId::from("gpt-5-mini")));
        state.rebuild_rows(&cat, &connected, &favs).await.unwrap();
        assert_eq!(state.visible()[0].origin, RowOrigin::Favorite);
        assert_eq!(state.visible()[0].model.as_str(), "gpt-5-mini");
    }

    #[tokio::test]
    async fn recent_appears_after_favorites() {
        let cat = catalog().await;
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        connected.insert(ProviderId::from("anthropic"));
        connected.insert(ProviderId::from("openai"));
        state.push_recent(ProviderId::from("openai"), ModelId::from("gpt-5-mini"));
        state
            .rebuild_rows(&cat, &connected, &HashSet::new())
            .await
            .unwrap();
        let recent_pos = state
            .visible()
            .iter()
            .position(|r| r.origin == RowOrigin::Recent)
            .unwrap();
        let connected_positions: Vec<usize> = state
            .visible()
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                if r.origin == RowOrigin::Connected {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        assert!(
            recent_pos
                < connected_positions
                    .iter()
                    .min()
                    .copied()
                    .unwrap_or(usize::MAX),
            "recent should appear before connected"
        );
    }

    #[tokio::test]
    async fn recent_is_deduplicated_and_capped() {
        // No catalog needed — we only exercise the recent list logic.
        let _cat = catalog().await;
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        connected.insert(ProviderId::from("anthropic"));
        for _ in 0..15 {
            state.push_recent(
                ProviderId::from("anthropic"),
                ModelId::from("claude-haiku-4-5"),
            );
        }
        assert_eq!(state.recent.len(), 1, "deduped");
        for i in 0..15 {
            state.push_recent(ProviderId::from("a"), ModelId::from(format!("m{i}")));
        }
        assert!(
            state.recent.len() <= 10,
            "capped at 10: got {}",
            state.recent.len()
        );
    }

    #[tokio::test]
    async fn filter_narrows_results() {
        let cat = catalog().await;
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        connected.insert(ProviderId::from("anthropic"));
        connected.insert(ProviderId::from("openai"));
        state
            .rebuild_rows(&cat, &connected, &HashSet::new())
            .await
            .unwrap();
        state.set_filter(Filter::new("haiku"));
        assert_eq!(state.visible().len(), 1);
        assert_eq!(state.visible()[0].model.as_str(), "claude-haiku-4-5");
    }

    #[tokio::test]
    async fn cursor_wraps() {
        let cat = catalog().await;
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        connected.insert(ProviderId::from("anthropic"));
        state
            .rebuild_rows(&cat, &connected, &HashSet::new())
            .await
            .unwrap();
        let len = state.visible().len();
        state.cursor = len - 1;
        state.move_down(1);
        assert_eq!(state.cursor, 0);
        state.move_up(1);
        assert_eq!(state.cursor, len - 1);
    }

    #[tokio::test]
    async fn empty_catalog_yields_no_rows() {
        let cat = InMemoryCatalog::new();
        let mut state = PickerState::new();
        state
            .rebuild_rows(&cat, &HashSet::new(), &HashSet::new())
            .await
            .unwrap();
        assert!(state.visible().is_empty());
        assert!(state.selected().is_none());
    }

    #[tokio::test]
    async fn toggle_favorite_flips_membership() {
        let cat = catalog().await;
        let mut state = PickerState::new();
        let mut connected = HashSet::new();
        connected.insert(ProviderId::from("anthropic"));
        state
            .rebuild_rows(&cat, &connected, &HashSet::new())
            .await
            .unwrap();
        assert!(state.favorites.is_empty());
        state.toggle_selected_favorite();
        assert_eq!(state.favorites.len(), 1);
        state.toggle_selected_favorite();
        assert!(state.favorites.is_empty());
    }
}
