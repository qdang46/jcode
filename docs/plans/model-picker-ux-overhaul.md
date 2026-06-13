# Model Picker UX Overhaul (v2)

## Goal

Port OpenCode's model picker UX into jcode: searchable, sectioned list (Favorites → Recent → Provider), model metadata display, persistent state, auth refresh, provider selector, model tooltips, arrow wrap, cyclic switching.

## Reference

OpenCode model picker code (cloned at `/tmp/feature-research/opencode`):
- `packages/tui/src/component/dialog-model.tsx` — TUI model dialog (favorites, recents, search, provider list, actions)
- `packages/tui/src/component/dialog-variant.tsx` — variant sub-dialog (reasoning effort)
- `packages/tui/src/component/dialog-provider.tsx` — provider selector
- `packages/tui/src/component/model-tooltip.tsx` — model info hover panel
- `packages/tui/src/context/local.tsx` — model state persistence (model.json: recent[], favorite[], variant {})
- `packages/app/src/context/models.tsx` — app model store (Persist.global, visibility toggle, latest detection)
- `packages/app/src/components/dialog-select-model.tsx` — React model picker (grouped list, fuzzy search, tags)
- `packages/ui/src/components/provider-icon.tsx` — provider icon component

## Phases

### Phase 1: Foundation — Model State Persistence

**Goal**: Save/load model favorites, recents, and per-model reasoning effort to a file so the data survives restarts.

**Files to change**:
- `.jcode/state/model_prefs.json` (new file, or extend existing config)

**New data structures** (in `jcode-base/src/config.rs`):
```rust
/// Per-model user preferences (cross-session, mutates via picker interactions).
pub struct ModelPreferences {
    pub recent: Vec<ModelKey>,       // MRU list, max 5
    pub favorite: Vec<ModelKey>,     // user-pinned
    pub variant: HashMap<String, Option<String>>,  // model_key -> reasoning_effort
}

pub struct ModelKey {
    pub provider_id: String,
    pub model_id: String,
}
```

**Persist path**: `~/.jcode/state/model_prefs.json` — atomic JSON write, same pattern as `session.json`.

**grep targets**:
- `crates/jcode-base/src/session.rs` — session.json read/write pattern (JSON, atomic write)
- `crates/jcode-base/src/config.rs` — Config struct for ModelPreferences field

### Phase 2: Picker Data Model — Entries from Favorites + Recents + All Models

**Goal**: The model picker shows sections: Favorites → Recent → Provider-grouped models, matching OpenCode's `DialogModel` component.

**Files to change**:
- `crates/jcode-tui/src/tui/mod.rs` — `PickerEntry` struct (already has `is_favorite`, `usage_score`, `created_date`, `effort` — may need `provider_name`, `cost`, `is_recent`)
- `crates/jcode-tui/src/tui/app/inline_interactive.rs` — model entry building (`populate_model_picker_options` picks models from provider catalog)
- `crates/jcode-base/src/provider/mod.rs` — model catalog accessors for cost/metadata

**Key code reference** (OpenCode):
```typescript
// dialog-model.tsx:23-127 — options() builds sections:
// 1. favoriteOptions — user-pinned models (category "Favorites")
// 2. recentOptions — MRU models not in favorites (category "Recent")
// 3. providerOptions — all remaining models grouped by provider.name
//    with footer: "Free" for cost.input === 0, sorted by release_date
// 4. popularProviders — unconnected providers to connect

// On select:
// local.model.set({ providerID, modelID }, { recent: true })
// If model has variants → open variant sub-dialog
```

**grep targets**:
- `inline_interactive.rs:populate_model_picker_options` — where model entries are created from catalog
- `inline_interactive.rs:handle_inline_interactive_key` — keyboard navigation
- `ui_overlays.rs:draw_inline_interactive` — visual rendering of the picker overlay

### Phase 3: Render Sections — Grouped List with Visual Tags

**Goal**: Render the picker with:
- Section headers (Favorites, Recent, Provider Name)
- Model name + provider name (in dim)
- Tags: "Current", "Favorite", "Free", "Latest"
- Search box at top with fuzzy matching

**Files to change**:
- `crates/jcode-tui/src/tui/ui_overlays.rs` — `draw_inline_interactive` or new `draw_model_picker` overlay function
- `crates/jcode-tui/src/tui/app/inline_interactive.rs` — add sectioning logic to `apply_inline_interactive_filter`

**Control flow**:
```
model picker opens
  → Phase 2 builds entries: favorites + recents + all_models
  → apply_inline_interactive_filter() groups them
  → renderer draws section headers + entries with tags
  → keyboard: arrows to navigate, Enter to select
  → onSelect: model.set() + push to recent + save prefs
```

**OpenCode render reference** (dialog-model.tsx:154-180):
```
<DialogSelect>
  sections: Favorites / Recent / {provider name}
  each item: title (model name), description (provider), footer (cost)
  actions: "Connect provider", "Favorite" toggle
  search: fuzzysort on title + description
```

### Phase 4: Favorites Toggle

**Goal**: Users can favorite/unfavorite models via keybinding in the picker (e.g., `f` key or action button).

**Files to change**:
- `crates/jcode-tui/src/tui/app/inline_interactive.rs` — handle `f` key in picker mode → toggle favorite + save prefs
- `PickerEntry` — `is_favorite` field already exists
- `ModelPreferences` — write to `model_prefs.json`

**OpenCode reference** (dialog-model.tsx:166-172):
```
{ command: "model.dialog.favorite", title: "Favorite",
  onTrigger: () => local.model.toggleFavorite(option.value) }
```

### Phase 5: Model Info Display

**Goal**: Show provider name, cost, release date, status in the picker list.

**Files to change**:
- `PickerEntry` — add cost, status, provider_name fields (or derive from `PickerOption`)
- `ui_overlays.rs` — render additional info columns/tags

**OpenCode reference** (dialog-model.tsx:74-83):
```
<div class="w-full flex items-center gap-x-2 text-13-regular">
  <span class="truncate">{i.name}</span>           // model name
  <Show when={isFree(..)}><Tag>Free</Tag></Show>    // cost tag
  <Show when={i.latest}><Tag>Latest</Tag></Show>    // latest tag
</div>
```

### Phase 6: Auth Refresh Fix

**Goal**: `/model <name>` and model picker Enter use `set_model_with_auth_refresh` instead of direct `set_model`, so switching to a model on a different provider auto-refreshes auth instead of failing.

**Files to change**:
- `crates/jcode-tui/src/tui/app/model_context.rs:1052-1084` — replace `app.provider.set_model()` with `app.provider.set_model_with_auth_refresh()`
- `crates/jcode-tui/src/tui/app/inline_interactive.rs:2737-2788` — replace `self.provider.set_route_selection()` with `set_model_with_auth_refresh` wrapper

**Key code reference** (mod.rs:212-232):
```rust
pub fn set_model_with_auth_refresh(
    &self,
    model: &str,
) -> Result<()> {
    self.set_model(model).or_else(|e| {
        self.on_auth_changed();
        self.set_model(model)
    })
}
```

**grep targets**:
- `mod.rs:set_model_with_auth_refresh` — the function to call
- `mod.rs:212-232` — the implementation (retry + on_auth_changed)

### Phase 7: Variant Sub-Dialog

**Goal**: After selecting a model with variant options (reasoning effort), show a sub-dialog to pick the variant — matching OpenCode's `DialogVariant` component.

**OpenCode reference** (dialog-model.tsx:139-152):
```typescript
function onSelect(providerID, modelID) {
  local.model.set({ providerID, modelID }, { recent: true })
  const list = local.model.variant.list()
  const cur = local.model.variant.selected()
  if (list.length > 0) {
    dialog.replace(() => <DialogVariant />)
    return
  }
  dialog.clear()
}
```

**Files to change**:
- New: `crates/jcode-tui/src/tui/model_variant.rs` — variant picker overlay
- `inline_interactive.rs` — `handle_inline_interactive_key` Enter handler for model picker

### Phase 8: Search Improvements

**Goal**: Better fuzzy search scoring: match against model name + provider name + model ID, with weighted scoring.

### Phase 9: Provider Selector Sub-Dialog

**Goal**: Model picker has a "Connect provider" action that opens a provider list. Selecting a provider triggers login/auth flow.

**OpenCode reference** (dialog-model.tsx:157-165):
```typescript
actions: [
  { command: "model.dialog.provider",
    title: connected() ? "Connect provider" : "View all providers",
    onTrigger() { dialog.replace(() => <DialogProvider />) },
  },
]
```

**Files to change**:
- New or reuse: `crates/jcode-tui/src/tui/provider_picker.rs` — provider list overlay (or reuse `account_picker_overlay`)
- `inline_interactive.rs` — picker action key (`p` or action button) that shows provider list
- On provider select: call existing login flow (`start_login_provider`)

**Key UX**:
```
Model picker → action bar: [Connect provider]
  → Provider list: Anthropic, OpenAI, OpenRouter, Copilot, ...
  → Select one → login/auth flow
  → Back to model picker with new provider's models
```

### Phase 10: Model Tooltip / Info Panel

**Goal**: Selecting a model in the picker shows a tooltip/info panel with cost, context window, latency, release date.

**OpenCode reference** (dialog-model.tsx:56-65, model-tooltip.tsx):
```typescript
itemWrapper={(item, node) => (
  <Tooltip class="w-full" placement="right-start" gutter={12}
    value={<ModelTooltip model={item} latest={item.latest} free={isFree(...)} />}>
    {node}
  </Tooltip>
)}
```

**Files to change**:
- `crates/jcode-tui/src/tui/ui_overlays.rs` — when a picker entry is highlighted, show a side panel with model details
- `inline_interactive.rs` — pass model metadata (cost, context, latency) through `PickerEntry` / `PickerOption`
- `PickerOption` — add `context_window: Option<u32>`, `latency_ms: Option<u32>`, `cost_per_million: Option<f64>`

### Phase 11: Arrow Wrap-Around Navigation

**Goal**: At the last entry in the picker, pressing Down arrow wraps to the first entry (and vice versa for Up at the first entry).

**OpenCode reference** (DialogSelect component — built-in wrapping):
```typescript
// When at last item and pressing Down → selected = 0
// When at first item and pressing Up → selected = last
```

**Files to change**:
- `inline_interactive.rs::handle_inline_interactive_key` — lines 2398-2405 (Up) and 2423-2432 (Down): replace `saturating_sub/min` with wrapping logic

### Phase 12: Cyclic Recent Model Switching

**Goal**: Tab/Shift+Tab cycle through recent models (inline, not in picker mode). OpenCode cycles through `modelStore.recent` array.

**OpenCode reference** (local.tsx:272-285):
```typescript
cycle(direction: 1 | -1) {
  const current = currentModel()
  const recent = modelStore.recent
  const index = recent.findIndex(x => x.providerID === current.providerID && x.modelID === current.modelID)
  let next = index + direction
  if (next < 0) next = recent.length - 1
  if (next >= recent.length) next = 0
  setModelStore("model", a.name, { ...recent[next] })
}
```

**Files to change**:
- `crates/jcode-tui/src/tui/app/inline_interactive.rs` — add `cycle_model(direction)` method using recent list
- Wire to Tab/Shift+Tab when NOT in picker mode (currently Tab in picker cycles entries, which is fine)
- For non-picker mode: Tab cycles through `model_prefs.recent` array

### Phase 13: "Free" / "Latest" Visual Tags

**Goal**: Render cost and freshness tags next to model names in the picker.

**OpenCode reference** (dialog-model.tsx:73-83):
```typescript
<div class="w-full flex items-center gap-x-2 text-13-regular">
  <span class="truncate">{i.name}</span>
  <Show when={isFree(i.provider.id, i.cost)}><Tag>Free</Tag></Show>
  <Show when={i.latest}><Tag>Latest</Tag></Show>
</div>
```

**Files to change**:
- `ui_overlays.rs` — in `draw_inline_interactive`, after model name, render `[Free]` `[Latest]` tags in dim/accent colors
- `PickerEntry` — add `is_free: bool`, `is_latest: bool`, `cost_per_million: Option<f64>` fields
```

---

## File Index

| File | Role |
|------|------|
| `crates/jcode-tui/src/tui/mod.rs` | `PickerEntry`, `PickerOption` structs |
| `crates/jcode-tui/src/tui/app/inline_interactive.rs` | Model picker entries, keyboard handling, filter |
| `crates/jcode-tui/src/tui/ui_overlays.rs` | `draw_inline_interactive` — picker overlay rendering |
| `crates/jcode-tui/src/tui/app/model_context.rs` | `/model <name>` command handler |
| `crates/jcode-base/src/provider/mod.rs` | `set_model()`, `set_model_with_auth_refresh()` |
| `crates/jcode-base/src/provider/accessors.rs` | `reconcile_auth_if_provider_missing()` |
| `crates/jcode-base/src/config.rs` | Config struct (add `model_preferences`) |
| `.jcode/state/model_prefs.json` | New file: recent[], favorite[], variant{} |
| `crates/jcode-tui/src/tui/model_variant.rs` | Phase 7: variant sub-dialog |
| `crates/jcode-tui/src/tui/provider_picker.rs` | Phase 9: provider selector |

## Implementation Order

1. **Phase 6** (2-line auth fix, highest impact)
2. **Phase 1** (persistence foundation — model_prefs.json)
3. **Phase 2 + 3** (picker sections + render — the core UI change)
4. **Phase 4** (favorites toggle with persistence)
5. **Phase 11** (arrow wrap — trivial, makes navigation feel right)
6. **Phase 13** (Free/Latest tags — simple visual polish)
7. **Phase 5** (model info display — cost, context window, latency)
8. **Phase 8** (search improvements — multi-key fuzzysort)
9. **Phase 7** (variant sub-dialog after model selection)
10. **Phase 12** (cyclic recent switching via Tab)
11. **Phase 10** (model tooltip info panel)
12. **Phase 9** (provider selector sub-dialog)

## Acceptance Criteria

1. `/model <name>` switches model without requiring re-auth
2. Model picker shows sections: Favorites → Recent → Provider
3. Arrow keys wrap around at list boundaries
4. Search filters all sections with fuzzy matching (name + provider + id)
5. `f` key toggles favorite on current selection
6. Favorite/recent selections persist across `~/.jcode/state/model_prefs.json`
7. Tab/Shift+Tab cycle through recent models
8. Model info shown: name, provider, cost tag (Free/paid), freshness (Latest), context window, latency
9. Highlighted model shows tooltip/info panel with details
10. Models with reasoning effort show variant sub-dialog after selection
11. "Connect provider" action opens provider selector → login flow
12. Model picker visual matches OpenCode layout parity
