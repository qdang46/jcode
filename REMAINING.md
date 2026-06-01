# Ratatui → FrankenTUI Migration: IN PROGRESS

**Branch**: `feature/ratatui-to-frankentui`
**Last reviewed**: 2026-06-01
**Head**: `84b3c6e1`
**Status**: 🔴 NOT COMPILING — 529 compile errors across 52 files
**Warnings**: 43 (unused imports, unused variables — non-blocking)

---

## Quick Status Dashboard

| Category | Status | Count |
|----------|--------|-------|
| Compile errors | 🔴 BLOCKING | 529 errors |
| Production files with ratatui refs | 🔴 BLOCKING | 16 files |
| Test files with ratatui refs | 🟡 non-blocking | 16 files |
| ftui migration skeleton | 🟢 done | `src/tui/runtime.rs`, `src/tui/model.rs` |
| Branch compiles | 🔴 NO | — |

---

## Compile Error Breakdown (529 total)

### By Error Code

| Error code | Count | Description |
|-----------|-------|-------------|
| E0308 | 190 | Mismatched types (ftui vs ratatui Frame/Buffer/Rect) |
| E0599 | 285 | Method not found (missing compat impls, wrong API) |
| E0277 | 64 | Trait bound not satisfied (From conversions missing) |
| E0061 | 41 | Wrong argument count to functions |
| E0560 | 10 | Struct field not found (API mismatch) |
| E0609 | 9 | No field on type (Optional::fg/bg access) |
| E0616 | 6 | Private field accessed (App fields in runtime.rs) |
| E0631 | 5 | Type mismatch in function arguments |
| E0117 | 1 | Foreign trait impl (PackedRgba in compat.rs) |
| E0023 | 1 | Pattern match field count mismatch |
| E0223 | 1 | Ambiguous associated type |
| E0425 | 1 | Function not found |

### By File (top contributors to 529 errors)

| File | Errors | Primary issue |
|------|--------|--------------|
| `src/tui/login_picker.rs` | 76 | `fg_compat` not on Style, wrong Paragraph::render API |
| `src/tui/ui_messages.rs` | 58 | Line/Text From conversions, Buffer API mismatch |
| `src/tui/ui_pinned.rs` | 54 | Paragraph render API, `fg_compat`, color enums |
| `src/tui/session_picker.rs` | 38 | Terminal lifecycle still ratatui, `fg_compat` |
| `src/tui/info_widget.rs` | 23 | Paragraph API, block methods |
| `src/tui/ui_diagram_pane.rs` | 20 | Paragraph/block API |
| `src/tui/permissions.rs` | 17 | `ratatui::init`/restore + `fg_compat` |
| `src/tui/app/debug_bench.rs` | 17 | TestBackend vs ftui harness |
| `src/tui/ui.rs` | 14 | `fg_compat`, Paragraph API |
| `src/tui/account_picker_render.rs` | 12 | `fg_compat` |
| `src/tui/ui_file_diff.rs` | 11 | `Line::style` field removed |
| `src/tui/app/run_shell.rs` | 11 | `fg_compat`, Terminal type mismatch |
| `src/tui/ui_prepare.rs` | 10 | Various ftui API mismatches |
| `src/tui/ui_inline_interactive.rs` | 9 | Paragraph render API |
| `src/tui/session_picker/render.rs` | 9 | `fg_compat` |
| `src/tui/account_picker.rs` | 9 | `fg_compat` |
| `src/tui/ui_viewport.rs` | 8 | `fg_compat`, Paragraph API |
| `src/tui/ui_pinned_selection.rs` | 8 | `Line::style` field, color enum |
| `src/tui/ui_overlays.rs` | 8 | Color::Rgb pattern match, `fg_compat` |
| `src/tui/runtime.rs` | 8 | Private App fields accessed |
| `src/tui/app/remote/reconnect.rs` | 8 | Frame type mismatch |
| `src/tui/ui_pinned_utils.rs` | 7 | Various ftui API mismatches |
| `src/tui/mermaid.rs` | 6 | ProcessMemorySnapshot fields renamed |
| `src/tui/ui_messages_cache.rs` | 5 | MessageCacheContext fields missing |
| `src/tui/ui_input.rs` | 5 | `fg_compat` |
| `src/cli/terminalinit.rs` | 4 | `is_terminal`/`write_all`/`flush` missing |
| `src/tui/ui_pinned_layout.rs` | 5 | Various ftui API mismatches |
| `src/tui/ui_pinned_mermaid_debug.rs` | 3 | Ambiguous RenderResult type |
| `src/video_export.rs` | 2 | `ratatui::buffer::Buffer`, Color::Modifier |
| `src/replay.rs` | 2 | ratatui Buffer/Rect types |
| `src/tui/ui_theme.rs` | 4 | Color enum variants |
| `src/tui/ui_diff.rs` | 4 | Color enum, `MonoColor::Gray` |
| `src/tui/ui_tools.rs` | 2 | `fg_compat` |
| `src/tui/ui_header.rs` | 2 | `fg_compat` |
| `src/tui/ui/draw_recovery.rs` | 4 | `fg_compat` |
| `src/tui/compat.rs` | 1 | E0117: foreign trait impl |
| `src/tui/app/model_context.rs` | 1 | Frame type mismatch |
| `src/tui/app/replay.rs` | 3 | Frame type mismatch |
| `src/tui/app/remote.rs` | 1 | DefaultTerminal type |
| `src/tui/app/state_ui.rs` | 1 | — |
| `src/tui/app/navigation.rs` | 2 | — |
| `src/tui/app/tui_lifecycle.rs` | 4 | — |
| `src/tui/app/tui_lifecycle_runtime.rs` | 5 | — |
| `src/tui/app/debug_profile.rs` | 1 | — |
| `src/tui/app/debug_cmds.rs` | 7 | — |
| `src/tui/workspace_client.rs` | 4 | — |
| `src/tui/usage_overlay.rs` | 6 | — |

---

## Root Cause Categories

### 1. `fg_compat` / `bg_compat` method not found (~60 errors)

**Cause**: `StyleCompatExt` trait from `src/tui/compat.rs` is not imported or implemented for the right `Style` type.

**Affected files** (18 files, ~195 call sites):
`login_picker.rs`, `ui_pinned.rs`, `ui_messages.rs`, `ui_inline.rs`, `ui_inline_interactive.rs`, `ui_input.rs`, `ui_overlays.rs`, `ui_viewport.rs`, `ui_prepare.rs`, `account_picker.rs`, `account_picker_render.rs`, `session_picker/render.rs`, `permissions.rs`, `info_widget_model.rs`, `info_widget_swarm_background.rs`, `ui/draw_recovery.rs`, `app/run_shell.rs`, `usage_overlay.rs`

**Fix needed**: Either:
- Add `impl StyleCompatExt for ftui_style::Style` in `compat.rs`, OR
- Replace all `fg_compat(color)` calls with the correct ftui pattern (e.g., `.fg(color_to_packedrgba(&color))`)

### 2. `impl From<FtuiColor> for PackedRgba` — E0117 foreign trait impl (1 error, blocking 190+)

**Cause**: `compat.rs` line 27 has `impl From<FtuiColor> for PackedRgba` which is a foreign trait implementation — only allowed for types defined in the current crate.

**Current state**: This one impl was intended to be the "big fix" but it's written in the wrong crate (`jcode` can't impl foreign traits for foreign types).

**Fix needed**: Either:
- Move this impl into `ftui_render::cell` in the frankentui repo, OR
- Replace all `FtuiColor` → `PackedRgba` conversions with explicit `color_to_packedrgba(&color)` calls throughout the codebase

This fix would eliminate the `fg_compat` errors AND the majority of E0308 mismatched types errors.

### 3. Frame type mismatch: `ratatui::Frame` vs `ftui::Frame` (~50 errors)

**Cause**: Terminal event loop files (`app/model_context.rs`, `app/remote/reconnect.rs`, `app/replay.rs`) still use `ratatui::Terminal::draw()` which passes `ratatui::Frame`, but `ui::draw()` now expects `ftui_render::frame::Frame`.

**Affected files**: `app/model_context.rs`, `app/remote/reconnect.rs`, `app/replay.rs`, `app/remote.rs`

**Fix needed**: These files need the frankentui runtime integration — `run_frankentui()` is already in `runtime.rs` but the event loop paths still use ratatui Terminal.

### 4. Terminal lifecycle: `ratatui::init` / `ratatui::restore` (4 sites)

**Cause**: `permissions.rs` and `session_picker.rs` have their own self-contained TUI run loops using `ratatui::init()` / `ratatui::restore()`. These are standalone programs (not using the main App) that need to be migrated.

**Affected files**: `src/tui/permissions.rs` (lines 501, 573), `src/tui/session_picker.rs` (lines 1238, 1385)

**Fix needed**: Replace with `ftui_tty::TtyBackend` or use the frankentui runtime. Note: `permissions.rs` also calls `terminal.draw()` with its own render — this is a complete mini-TUI that needs migration.

### 5. `Paragraph::render` / `Block::render` API mismatch (~25 errors)

**Cause**: ftui's Paragraph widget uses a different render API than ratatui.

In ratatui: `paragraph.render(area, frame.buffer_mut())` or `frame.render_widget(&paragraph, area)`
In ftui: `paragraph.render_into(frame, area)` or `paragraph.render(frame, area)` with different signature

**Affected files**: `login_picker.rs`, `ui_pinned.rs`, `ui_messages.rs`, `ui_diagram_pane.rs`, `info_widget.rs`, `ui_inline_interactive.rs`, `ui_file_diff.rs`, `ui_prepare.rs`, `session_picker.rs`

**Fix needed**: Check ftui-render's actual Paragraph API and update call sites.

### 6. `Line::from(vec![...])` / `Text::from(lines)` — missing From impls (~58 errors)

**Cause**: ftui's `Line` doesn't have `impl From<Vec<Span>>` and `Text` doesn't have `impl From<Vec<Line>>`.

**Affected files**: `ui_messages.rs`, `ui_pinned.rs`, `ui_prepare.rs`, `ui_inline.rs`, `ui_inline_interactive.rs`, `ui_tools.rs`, `ui_header.rs`, `ui_pinned_selection.rs`, `ui_pinned_utils.rs`

**Fix needed**: Either add helper functions (`line_from_spans`, `text_from_lines` already exist in `compat.rs`) and update call sites, OR the compat module's `line_from_spans` / `text_from_lines` need to be made pub/exported and used everywhere.

### 7. `Color::Rgb(r, g, b)` — pattern match destructuring wrong (1 error)

**Cause**: In `ui_overlays.rs:624`, `Color::Rgb(...)` is matched as a 3-field variant but ftui's `Color::Rgb` is a single-field wrapper.

**Fix**: Change `Color::Rgb(r, g, b)` to `Color::Rgb(ftui_style::Rgb { r, g, b })` or `Color::Rgb(rgb_struct)`.

### 8. `Line::style` field removed (~3 errors)

**Cause**: `ftui_text::Line` no longer has a `.style` field. Code in `ui_file_diff.rs` and `ui_pinned_selection.rs` tries to access `line.style`.

**Fix needed**: Styles on lines are handled differently in ftui. Check ftui's Line API for the equivalent.

### 9. Private App fields accessed in `runtime.rs` (~6 errors)

**Cause**: `src/tui/runtime.rs` line 98-103 tries to access `app.reload_requested`, `app.rebuild_requested`, `app.update_requested`, `app.restart_requested`, `app.requested_exit_code`, `app.session` — all are private fields.

**Fix needed**: Either make fields pub or add accessor methods on App.

### 10. `terminal.draw()` signature mismatch — `buffer_mut` / `area` methods gone (~20 errors)

**Cause**: ftui's `Frame` doesn't have `buffer_mut()` or `area()` methods. Code using these on `ftui::Frame` won't compile.

**Affected files**: `app/model_context.rs`, `app/remote/reconnect.rs`, `permissions.rs`, `session_picker.rs`

### 11. Struct field changes (10 errors)

**Affected**:
- `ProcessMemorySnapshot`: `rss_bytes` → `resident_bytes`, `peak_rss_bytes` → `peak_resident_bytes`, `virtual_bytes` → `virtual_mem_bytes` (in `mermaid.rs`)
- `MessageCacheContext`: `diagram_mode`, `centered`, `mermaid_epoch`, `mermaid_aspect_bucket` fields missing (in `ui_messages_cache.rs`)

### 12. `cli/terminalinit.rs` — std I/O methods missing (4 errors)

**Cause**: `std::io::Stdout` doesn't have `is_terminal()`, `write_all()`, `flush()` in older Rust/MSRV. These are nightly/std versions.

**Fix needed**: Use `std::io::IsTerminal` trait (Rust 1.63+) or `crossterm::terminal::is_terminal()`.

### 13. `render_stateful_widget` / `render_widget` / `set_stringn` not found

**Cause**: These ratatui-specific methods don't exist on ftui equivalents.

### 14. `block::Block` missing methods: `title_bottom`, `title_style`

**Cause**: ftui Block doesn't have these methods that ratatui Block had.

### 15. Color enum variant mismatches (~15 errors)

**Cause**: Using old ratatui color patterns:
- `Color::White` → `Color::Mono(MonoColor::White)`
- `Color::Indexed(n)` → doesn't exist in ftui
- `Color::DarkGray` → `Color::Mono(MonoColor::BrightBlack)`
- `Color::Red` → `Color::Mono(MonoColor::Red)`
- `Color::Gray` on `MonoColor` → different enum
- `Color::Reset` → doesn't exist

**Affected files**: `session_picker.rs`, `ui_diff.rs`, `ui_theme.rs`, `ui_overlays.rs`

---

## Production Code Still Using ratatui (16 files)

These files import/use `ratatui` directly and **must** be migrated:

| File | Type | What uses ratatui |
|------|------|-------------------|
| `src/cli/terminal.rs` | Standalone init | `ratatui::init()`, `ratatui::restore()`, `DefaultTerminal` |
| `src/tui/permissions.rs` | Standalone mini-TUI | `ratatui::init()`, `ratatui::restore()`, `terminal.draw()` |
| `src/tui/session_picker.rs` | Standalone mini-TUI | `ratatui::init()`, `ratatui::restore()`, `terminal.draw()` |
| `src/tui/app/model_context.rs` | App module | `DefaultTerminal`, `terminal.draw()` with ratatui Frame |
| `src/tui/app/remote/reconnect.rs` | App module | `DefaultTerminal`, `ratatui::Backend`, `terminal.draw()` |
| `src/tui/app/replay.rs` | App module | `DefaultTerminal`, `terminal.draw()` with ratatui Frame |
| `src/tui/app/run_shell.rs` | App module | `DefaultTerminal`, `ratatui::Terminal`, `TestBackend`, `fg_compat` |
| `src/tui/app/remote.rs` | App module | `DefaultTerminal`, `Terminal`, `Backend` |
| `src/tui/app/event_wrappers.rs` | App module | `DefaultTerminal` |
| `src/tui/app/input.rs` | App module | `DefaultTerminal` |
| `src/tui/app/local.rs` | App module | `DefaultTerminal` |
| `src/tui/app/turn.rs` | App module | `DefaultTerminal` |
| `src/tui/ui_diagram_pane.rs` | UI module | `ratatui::style::Modifier` (import only — appears unused) |
| `src/replay.rs` | Standalone replay | `ratatui::buffer::Buffer`, `ratatui::layout::Rect` |
| `src/video_export.rs` | Video export | `ratatui::buffer::Buffer`, `ratatui::style::Color`, `Modifier::BOLD` |

### Comments-only ratatui references (can ignore)
- `src/tui/app.rs`: 1 comment about "ratatui's diff model"
- `src/cli/tui_launch.rs`: 1 comment saying "Run using frankentui runtime instead of ratatui"
- `crates/jcode-tui-mermaid/src/mermaid_widget.rs`: 1 doc comment about `ratatui-image`
- `crates/jcode-tui-mermaid/src/mermaid_content.rs`: 1 doc comment about "ratatui Lines"

---

## Test Files Still Using ratatui (16 files — non-blocking for compilation)

These use `ratatui::backend::TestBackend` and compile fine but are technically not migrated:

| File | ratatui refs |
|------|-------------|
| `src/tui/session_picker_tests.rs` | 4 |
| `src/tui/ui_tests/basic/frame_flicker.rs` | 6 |
| `src/tui/app/remote_tests.rs` | 2 |
| `src/tui/app/tests/scroll_copy_01/part_01.rs` | 22 |
| `src/tui/app/tests/scroll_copy_02/part_01.rs` | 4 |
| `src/tui/app/tests/scroll_copy_02/part_02.rs` | 6 |
| `src/tui/app/tests/scroll_copy_03.rs` | 2 |
| `src/tui/app/tests/state_model_poke_01/part_01.rs` | 8 |
| `src/tui/app/tests/state_model_poke_01/part_02.rs` | 8 |
| `src/tui/app/tests/state_model_poke_02/part_01.rs` | 6 |
| `src/tui/app/tests/remote_events_reload_01/part_01.rs` | 4 |
| `src/tui/app/tests/remote_events_reload_02/part_01.rs` | 12 |
| `src/tui/app/tests/commands_accounts_02/part_01.rs` | 2 |
| `src/tui/app/tests/support_failover/part_02.rs` | 2 |
| `crates/jcode-tui-mermaid/src/mermaid_tests/part_01.rs` | 2 |
| `crates/jcode-tui-mermaid/src/mermaid_tests/part_02.rs` | 2 |

**Note**: These won't block compilation but they will break when `ratatui` is removed from `Cargo.toml`. They need to be ported to `ftui_harness` or the ftui test backend pattern.

---

## Minimal Fixes Needed to Reach Compilation

If fixing incrementally, here's the recommended order:

### Tier 1: Unblock the most errors with fewest changes

1. **Fix `impl From<FtuiColor> for PackedRgba`** (E0117 in `compat.rs`) — This is the single most impactful fix. Move it to frankentui or replace all uses.
2. **Add `impl StyleCompatExt for ftui_style::Style`** — Makes `fg_compat`/`bg_compat` available.
3. **Fix `Line`/`Text` From impls** — Use existing `line_from_spans`/`text_from_lines` helpers throughout.

### Tier 2: Fix Frame/terminal type mismatches

4. **Fix `app/model_context.rs`** — Frame type mismatch in `terminal.draw()`.
5. **Fix `app/remote/reconnect.rs`** — Multiple Frame type mismatches.
6. **Fix `app/replay.rs`** — Frame type mismatch.

### Tier 3: Fix standalone TUI programs

7. **Fix `permissions.rs`** — Replace `ratatui::init`/`restore` + `terminal.draw()`.
8. **Fix `session_picker.rs`** — Replace `ratatui::init`/`restore` + `terminal.draw()`.
9. **Fix `cli/terminal.rs`** — Replace `ratatui::DefaultTerminal` with frankentui equivalent.

### Tier 4: Fix API mismatches

10. **Fix Paragraph/Block render API** — Update call sites for ftui's API.
11. **Fix Color enum variants** — Replace `Color::White` → `Color::Mono(...)` etc.
12. **Fix private App fields** — Add accessors or make pub.
13. **Fix struct field names** — `ProcessMemorySnapshot`, `MessageCacheContext`.

### Tier 5: Polish

14. **Fix `ui_diagram_pane.rs`** — Remove unused `Modifier` import.
15. **Fix `video_export.rs`** — Replace `ratatui::buffer::Buffer` with ftui Buffer.
16. **Fix `replay.rs`** — Replace `ratatui::buffer::Buffer` / `ratatui::layout::Rect`.

---

## What's Already Done (Good Foundation)

- ✅ `ftui` crates fully integrated in `Cargo.toml` (ftui, ftui-core, ftui-style, ftui-render, ftui-text, ftui-layout, ftui-widgets, ftui-runtime, ftui-tty)
- ✅ `src/tui/runtime.rs` — frankentui `AppWrapper` + `run_frankentui()` function exists
- ✅ `src/tui/model.rs` — frankentui `Model` struct defined
- ✅ `src/cli/terminalinit.rs` — frankentui-compatible TUI init exists (has compile errors)
- ✅ `src/tui/compat.rs` — compatibility layer exists (needs fixing)
- ✅ `src/cli/tui_launch.rs` — calls `run_frankentui()` (entry point ready)
- ✅ `src/tui/ui.rs` — `draw()` function uses `ftui_render::frame::Frame`
- ✅ All `jcode-tui-*` sub-crates migrated to ftui (no ratatui deps)
- ✅ 43 warnings only (unused imports, unused variables) — non-blocking

---

## Key Reference: ratatui → ftui Type Mapping

| ratatui | ftui | Status |
|---------|------|--------|
| `Style::default().fg(c)` | `Style::new().fg(PackedRgba::WHITE)` or `.fg(color_to_packedrgba(&c))` | Needs compat fix |
| `.fg_compat(color)` | `.fg(color_to_packedrgba(&color))` | Needs impl |
| `Color::White` | `Color::Mono(MonoColor::White)` | Partial |
| `Color::Rgb(r,g,b)` | `Color::Rgb(Rgb::new(r,g,b))` | Partial |
| `Line::from(vec![...])` | `Line::from_spans(vec![...])` | Needs helper |
| `Text::from(lines)` | `Text::from_lines(lines)` | Needs helper |
| `Layout::default().direction(Vertical)` | `Flex::vertical()` | ✅ Done |
| `Constraint::Length(n)` | `Constraint::Fixed(n)` | ✅ Done |
| `frame.area()` | `Rect::new(0, 0, frame.buffer.width(), frame.buffer.height())` | ✅ Done |
| `frame.buffer_mut()` | `&mut frame.buffer` | ✅ Done |
| `Paragraph::render(area, buf)` | `paragraph.render_into(frame, area)` | Needs update |
| `Block::bordered()` | `Block::new().borders(Borders::ALL)` | ✅ Done |
| `DefaultTerminal` | `ftui_tty::TtyBackend` | Not wired |
| `ratatui::init()` / `restore()` | `ftui_tty::TtyBackend::new()` / `drop()` | Not wired |
| `TestBackend::new(w, h)` | `ftui_harness::TestBackend::new(w, h)` | Not available yet |
| `buffer::Buffer` | `ftui_render::buffer::Buffer` | Partial |

---

## Warnings (43 total — non-blocking)

All warnings are unused imports/variables. No dead code, no deprecated items, no unsafe code warnings (at current compile state):

```
unused import: `ftui_core::geometry::Rect` (×4)
unused imports: `Constraint`, `Direction`, `Flex` (×3)
unused imports: `ftui_render::cell::PackedRgba` (×3)
unused variable: `terminal` (×2)
unused import: `Direction` (×2)
unused import: `ratatui::style::Modifier` (×1)
unused import: `ftui_widgets::paragraph::Paragraph` (×1)
unused import: `ftui_widgets::borders::BorderType` (×1)
unused import: `ftui_layout::Constraint` (×1)
unused import: `std::sync::Arc` (×1)
unused import: `jcode_tui_style::theme::blend_color` (×1)
field `focused_sessions` is never read (×1)
```

---

## Verification Commands

```bash
# Count compile errors
cargo check 2>&1 | grep "^error\[" | wc -l
# Current: 529

# Count warnings
cargo check 2>&1 | grep "warning:" | wc -l
# Current: 43

# Files with ratatui (production)
rg "use ratatui" --type rust src/ crates/ | grep -v test | grep -v _tests | grep -v _bench | wc -l
# Current: ~16 files

# Error breakdown by code
cargo check 2>&1 | grep "^error\[" | sort | uniq -c | sort -rn

# Errors by file
cargo check 2>&1 | grep "^error\[" -A1 | grep "  -->" | sed 's/ *--> //' | cut -d: -f1 | sort | uniq -c | sort -rn
```
