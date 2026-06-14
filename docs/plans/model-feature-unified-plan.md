# Model Feature — Unified Implementation Plan
> Research across 9 repos: OpenCode, Codex, Claude Code, Codebuff, oh-my-pi, oh-my-openagent, pi-agent-rust, oh-my-claudecode, oh-my-codex

## 1. Executive Summary

jcode's model feature is 60% there — has model picker, switching, provider catalog. Missing: persistence to config (both `/model` and picker), proper sectioned picker UI (Favorites → Recent → Provider), model/search improvements, and server/client mode sync. We'll copy OpenCode's architecture (proven, open-source) with Codex's persistence pattern.

## 2. Cross-Repo Patterns

| Feature | OpenCode | Codex | Claude Code | oh-my-pi | Best Practice |
|---------|----------|-------|-------------|----------|---------------|
| **Persistence** | model.json (recent[], favorite[], variant{}) | `~/.codex/config.toml` (model field) | `~/.claude/settings.json` (model NOT persisted by /model) | `~/.omp/config.yml` | **Codex**: `/model` writes to config file — survives restart |
| **Fallback chain** | CLI arg → config → recent → provider default → first | CLI flag → config → profile → default | settings.json → modelType → default | role-based (smol/ default/slow) → fallback chain | **OpenCode**: 5-tier, recents as fallback |
| **Picker sections** | Favorites → Recent → Provider → Popular | Auto models → All models (flat) | Terminal picker (simple) | Provider tabs + search | **OpenCode**: Favorites + Recent + Provider |
| **Search** | fuzzysort (title*2 + category) | Basic text filter | None | Basic filter | **OpenCode**: multi-key fuzzy |
| **Variant** | Sub-dialog after selection | Reasoning effort popup | Alt+R | — | **Codex**: auto popup after model select |
| **Provider selector** | "Connect provider" in picker | — | — | — | **OpenCode** |
| **Favorites** | Toggle per model in picker | — | — | — | **OpenCode** |
| **Cyclic switch** | Tab cycle through recent | — | Tab cycle | alt+m/alt+p | **OpenCode**: Tab → recent cycle |

## 3. Architecture Decision

### Chosen Approach: OpenCode's 3-layer + Codex's persistence

- **Layer 1 — Persistence** (Codex): `/model` + picker write to `~/.jcode/config.toml` (DONE, but buggy). Add `model_prefs.json` for favorites/recents/variant.
- **Layer 2 — Store/Sync**: Sync config on model change for daemon mode. `SetModel`/`SetRoute` both save to config server-side. Client-side cycle_model should go through server protocol, not local provider.
- **Layer 3 — Picker UI** (OpenCode): Sectioned: Favorites → Recent → Provider. fuzzysort multi-key. Variant sub-dialog. Provider selector.

### Alternatives Considered

| Approach | Source | Pros | Cons | Decision |
|----------|--------|------|------|----------|
| Config-only persist | Codex | Simple, 1 file | Lose recents/favorites | DONE as base layer |
| model.json persist | OpenCode | Favorites, recents, variant per model | Extra file | ADD for model_prefs.json |
| Flat model list | Codex | Simple | No grouping, harder to navigate | REJECT — use OpenCode sections |
| Role-based (smol/slow/plan) | oh-my-pi | Powerful for complex workflows | Over-engineered for current needs | DEFER |

## 4. Current Issues & Root Causes

| Issue | Root Cause | Fix |
|-------|-----------|-----|
| Model not persisting on `/model` in daemon | `key_handling.rs` `/model` sends `SetModel` to server, server saves to config (DONE). But user's binary was OLD | DONE: `apply_set_model` + `apply_set_route` both save |
| Model not persisting on picker in daemon | Picker sends `SetRoute`, `apply_set_route` was missing save | DONE |
| `cycle_model` shows error "not available" | Client-side `available_models_for_switching` returns empty for daemon proxy provider | Need: cycle through server protocol |
| Panic on SectionHeader Enter | SectionHeader entries have empty options vec | DONE |
| DeepSeek 400 tool name | Tool names contain chars not matching `^[a-zA-Z0-9_-]+$` | DONE: sanitize in request building |

## 5. Remaining Work

### Phase A — Fix cycle_model for daemon mode
- `cycle_model` currently calls `self.provider.available_models_for_switching()` (client-side)
- In daemon mode, this returns empty because the client proxy doesn't have the model list
- Fix: send `CycleModel` protocol message to server → server cycles and returns result
- OR: better, make `cycle_model` use `cycle_recent_model` which uses saved recents

### Phase B — model_prefs.json (Favorites + Recents + Variant)
- jcode already has `model_picker_favorites_store` and `model_picker_usage_store`
- Need unified `model_prefs.json` with `recent[]`, `favorite[]`, `variant{}`
- Write on every model switch (both `/model` and picker)
- Read on startup → populate model picker sections

### Phase C — Picker Sections Refinement
- `SectionHeader` infrastructure is DONE (PickerAction variant, filter handles it)
- Need to actually render section headers in `draw_inline_interactive` or overlay
- Need to group entries by Favorites/Recent/Provider in `build_model_picker_entries`

### Phase D — Variant Sub-Dialog
- After selecting a model with variants, show a sub-picker for reasoning effort
- OpenCode pattern: `dialog.replace(() => <DialogVariant />)`
- jCode's picker infrastructure supports nested pickers

### Phase E — Provider Selector
- "Connect provider" action in model picker
- Opens provider list → login flow
- jCode already has `login_picker_overlay` and `account_picker_overlay`

### Phase F — Search Improvements
- Multi-key fuzzy (name ×3 + provider ×2 + method ×1) — DONE
- What's missing: scoring with favorite/recommended bonus — DONE
- Need: sort results by (favorite > recent > usage > recommended) within each section

## 6. Implementation Priority

1. **Phase A** (cycle_model fix) — minimal code change, high impact
2. **Phase B** (model_prefs.json) — foundation for recents/favorites
3. **Phase C** (picker sections render) — visual improvement
4. **Phase D** (variant sub-dialog) — UX improvement
5. **Phase E** (provider selector) — UX improvement
6. **Phase F** (search refinement) — polish

## 7. Repo References

| Feature | Repo | File | 
|---------|------|------|
| Model persistence | opencode | `packages/tui/src/context/local.tsx:134-158` |
| Model picker UI | opencode | `packages/tui/src/component/dialog-model.tsx` |
| DialogSelect component | opencode | `packages/tui/src/ui/dialog-select.tsx` |
| Model store (app) | opencode | `packages/app/src/context/models.tsx` |
| Config persist | codex | `config/src/config_toml.rs:139-146` |
| Model popup | codex | `tui/src/chatwidget/model_popups.rs:170-237` |
| Reasoning effort popup | codex | `tui/src/chatwidget/model_popups.rs` |
| Model selector TUI | oh-my-pi | `packages/coding-agent/src/modes/components/model-selector.tsx` |
| Settings schema | oh-my-pi | `packages/coding-agent/src/config/settings-schema.ts:376-388` |
| Fallback chain | oh-my-openagent | `packages/model-core/src/model-resolution-pipeline.ts:75-256` |
