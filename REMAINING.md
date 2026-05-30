# Ratatui → FrankenTUI Migration: REMAINING WORK

**Branch**: `feature/ratatui-to-frankentui`
**Updated**: 2026-05-30
**Current errors**: 965 (down from 2081)
**Completion**: ~60%

---

## Quick Status Dashboard

| Category | Done | Remaining | Total |
|----------|------|-----------|-------|
| Beads (closed) | 14 | 25 (24 open + 1 in-progress) | 39 |
| Files with `ratatui::` | — | 34 | — |
| Files with `ftui::` | 7 | — | — |
| Compile errors | — | 965 | — |

---

## Error Breakdown (965 total)

| Code | Count | Meaning | Primary Cause |
|------|-------|---------|---------------|
| E0599 | 248 | No method found | ftui API differences |
| E0308 | 201 | Type mismatch | ratatui vs ftui types |
| E0609 | 181 | No field found | struct field differences |
| E0560 | 142 | Struct variant/field | Color enum, Constraint variants |
| E0277 | 82 | Trait bound | Color→PackedRgba, Serialize |
| E0061 | 51 | Wrong arg count | Constraint, stub functions |
| E0616 | 50 | Private field | buffer access patterns |
| E0433 | 4 | Unresolved type | missing imports |
| Other | 6 | Misc | orphan impl, etc. |

---

## Top Error Hotspots (files needing most work)

| # | File | Errors | Status | What's Needed |
|---|------|--------|--------|---------------|
| 1 | `src/tui/ui_prepare.rs` | 86 | ❌ Not ported | Full port to ftui types |
| 2 | `src/tui/ui_viewport.rs` | 59 | 🔧 Partial | Fix type mismatches, Frame API |
| 3 | `src/tui/ui_diagram_pane.rs` | 52 | 🔧 Partial | Fix remaining type issues |
| 4 | `src/tui/usage_overlay.rs` | 49 | ❌ Not ported | Full ratatui→ftui port |
| 5 | `src/tui/ui_messages.rs` | 28 | 🔧 Partial | Type resolution |
| 6 | `src/tui/session_picker.rs` | 28 | 🔧 Partial | Replace `ratatui::init/restore`, fix types |
| 7 | `src/tui/login_picker.rs` | 26 | 🔧 Partial | Fix type mismatches |
| 8 | `src/tui/app/debug_bench.rs` | 23 | ❌ Not ported | Replace TestBackend, DefaultTerminal |
| 9 | `src/tui/workspace_client.rs` | 21 | 🔧 Partial | WorkspaceMapModel API mismatch |
| 10 | `src/tui/ui_header.rs` | 18 | ❌ Not ported | Port to ftui Block+spans |
| 11 | `src/tui/ui_file_diff.rs` | 18 | 🔧 Partial | Fix remaining types |
| 12 | `crates/jcode-tui-messages/src/lib.rs` | 17 | 🔧 Stub | Complete DisplayMessage API |
| 13 | `crates/jcode-tui-mermaid/src/lib.rs` | 16 | 🔧 Stub | Complete stub functions |
| 14 | `src/tui/compat.rs` | 15 | 🔧 Broken | Fix orphan impl, conversion fns |
| 15 | `src/tui/permissions.rs` | 13 | ❌ Not ported | Replace `ratatui::init/restore` |
| 16 | `src/tui/account_picker_render.rs` | 12 | 🔧 Partial | PackedRgba vs Color |
| 17 | `crates/jcode-tui-render/src/box_utils.rs` | 12 | 🔧 Partial | Fix buffer operations |
| 18 | `src/tui/ui_overlays.rs` | 11 | 🔧 Partial | Fix remaining issues |
| 19 | `src/tui/ui_frame_metrics.rs` | 11 | 🔧 Partial | Fix Frame API |
| 20 | `src/tui/app/run_shell.rs` | 10 | 🔧 Partial | Replace DefaultTerminal |

---

## Stub Crates That Need Completion

These crates are partial stubs — their incomplete API surfaces block dozens of files from compiling.

### 1. `crates/jcode-tui-messages/` — Missing DisplayMessage variants

```
Missing constructors:
  - DisplayMessage::tool()
  - DisplayMessage::usage()
  - DisplayMessage::memory()
  - DisplayMessage::background_task()
  - DisplayMessage::overnight()
  - DisplayMessage::swarm()

Missing methods:
  - .with_title()

Type mismatches:
  - display_messages_from_rendered_messages() takes wrong type
```

**Blocks**: `app/debug_cmds.rs`, `app/inline_interactive.rs`, `app/local.rs`, `app/model_context.rs`, `app/remote/server_events.rs`, `app/replay.rs`, `app/state_ui*.rs`, `app/tui_lifecycle*.rs`, `app/turn_memory.rs`

### 2. `crates/jcode-tui-mermaid/` — Empty stub functions

```
Empty stubs (take 0 args, return ()):
  - debug_test_scroll()
  - debug_memory_benchmark()
  - debug_flicker_benchmark()
  - render_image_widget_scale()
  - clear_cache()

Missing fields:
  - DiagramInfo::hash

Type mismatches:
  - protocol_type() returns &str not Option-like
```

**Blocks**: `app/debug_cmds.rs`, `app/navigation.rs`, `ui_diagram_pane.rs`

### 3. `crates/jcode-tui-markdown/` — Incomplete API

```
Missing:
  - IncrementalMarkdownRenderer::new() takes 0 args (needs config)
  - IncrementalMarkdownRenderer::reset()
  - IncrementalMarkdownRenderer::set_width()
  - debug_memory_profile()
  - update() returns () not Vec<Line>
```

**Blocks**: `app/input.rs`, `app/tui_lifecycle.rs`, `app/debug_profile.rs`

### 4. `crates/jcode-tui-style/` — Partial API

```
Type mismatches:
  - activity_indicator() takes 1 arg (usize) not 3
  - DebugStats missing fields + Serialize derive
```

**Blocks**: `app/run_shell.rs`, `app/debug_bench.rs`

---

## Completely Unported Files (still using raw `ratatui::`)

| File | What ratatui types it uses |
|------|---------------------------|
| `src/cli/terminal.rs` | `DefaultTerminal`, `Terminal`, `CrosstermBackend`, `init`, `restore` |
| `src/tui/permissions.rs` | `ratatui::init()`, `ratatui::restore()` |
| `src/tui/session_picker.rs` | `ratatui::init()`, `ratatui::restore()` |
| `src/tui/app/turn.rs` | `DefaultTerminal` |
| `src/tui/app/event_wrappers.rs` | `DefaultTerminal` |
| `src/tui/app/local.rs` | `DefaultTerminal` |
| `src/tui/app/remote/reconnect.rs` | `DefaultTerminal`, `Terminal<B>`, `Backend` |
| `src/tui/app/run_shell.rs` | `DefaultTerminal`, `buffer::Buffer`, `Terminal`, `TestBackend` |
| `src/tui/app/debug_bench.rs` | `Terminal`, `TestBackend` |
| `src/tui/app/model_context.rs` | `DefaultTerminal` |
| `src/replay.rs` | `buffer::Buffer`, `layout::Rect` |
| `src/video_export.rs` | `buffer::Buffer`, `style::Color`, `style::Modifier` |
| `src/bin/tui_bench.rs` | `Terminal`, `TestBackend` |

---

## Test Files Still Using Ratatui (16 files)

All use `ratatui::backend::TestBackend` + `ratatui::Terminal`:

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

---

## FrankenTUI-side Errors (need upstream fixes)

These errors are in frankentui crates themselves (76 total), not jcode:

| File | Errors | Issue |
|------|--------|-------|
| `ftui-layout/src/lib.rs` | 22 | Constraint type mismatches |
| `ftui-widgets/src/block.rs` | 17 | Block API differences |
| `ftui-style/src/style.rs` | 17 | Style trait impls |
| `ftui-text/src/text.rs` | 13 | Line/Text From impls |
| `ftui-widgets/src/lib.rs` | 11 | Widget trait |
| `ftui-render/src/buffer.rs` | 7 | Buffer access patterns |

---

## Bead Status (25 remaining)

### In Progress (1)

| Bead | Phase | Title | Blocked By |
|------|-------|-------|------------|
| `jcode-4we` | 4.3 | Decompose ui.rs draw() → Model view() methods | `jcode-7um` ✅, `jcode-eeu` ✅, `jcode-vbr` ✅ |

### Open — Phase 4 (Core Widgets)

| Bead | Phase | Title | Blocked By | Actual Status |
|------|-------|-------|------------|---------------|
| `jcode-hj9` | 4.1 | Port jcode-tui-messages | `7um` ✅, `eeu` ✅, `vbr` ✅ | Stub API incomplete — 17 errors |
| `jcode-qk7` | 4.2 | Port jcode-tui-markdown | `7um` ✅, `eeu` ✅, `vbr` ✅ | Stub API incomplete |
| `jcode-p6d` | 4.4 | Port ui_header.rs | `7um` ✅, `eeu` ✅, `vbr` ✅ | Not ported — 18 errors |
| `jcode-ut6` | 4.5 | Port ui_viewport.rs | `7um` ✅, `eeu` ✅, `vbr` ✅ | Partial — 59 errors |
| `jcode-obs` | 4.6 | Port ui_messages.rs | `7um` ✅, `eeu` ✅, `vbr` ✅ | Partial — 28 errors |
| `jcode-vzo` | 4.7 | Port ui_transitions + ui_animations | `7um` ✅, `eeu` ✅, `vbr` ✅ | Mostly ported (warnings only) |
| `jcode-ply` | 4.8 | Port ui_memory, ui_file_diff, ui_diagram_pane | `7um` ✅, `eeu` ✅, `vbr` ✅ | Partial — 52+18 errors |

### Open — Phase 5 (Workspace)

| Bead | Phase | Title | Blocked By | Actual Status |
|------|-------|-------|------------|---------------|
| `jcode-t63` | 5.1 | Replace jcode-tui-workspace | `jcode-4we` 🔄 | API mismatch — 21 errors |

### Open — Phase 6 (Interactive Widgets)

| Bead | Phase | Title | Blocked By | Actual Status |
|------|-------|-------|------------|---------------|
| `jcode-19t` | 6.1 | Port session_picker.rs | `jcode-4we` 🔄 | Partial — 28 errors, `ratatui::init/restore` |
| `jcode-occ` | 6.2 | Port login_picker.rs | `jcode-4we` 🔄 | Partial — 26 errors |
| `jcode-zqs` | 6.3 | Port account_picker.rs | `jcode-4we` 🔄 | Partial — 9+12 errors |
| `jcode-1ub` | 6.4 | Port info_widget series | `jcode-4we` 🔄 | Partial — multiple errors |
| `jcode-1gy` | 6.5 | Port ui_input.rs | `jcode-4we` 🔄 | Mostly ported |
| `jcode-wuy` | 6.6 | Port ui_pinned*.rs | `jcode-4we` 🔄 | Mostly ported — 7 errors |
| `jcode-9ar` | 6.7 | Port ui_overlays.rs | `jcode-4we` 🔄 | Mostly ported — 11 errors |

### Open — Phase 7 (Diagram & Media)

| Bead | Phase | Title | Blocked By | Actual Status |
|------|-------|-------|------------|---------------|
| `jcode-lvl` | 7.1 | Port jcode-tui-mermaid | `jcode-t63` | Stub — 16 errors |

### Open — Phase 8 (Integration)

| Bead | Phase | Title | Blocked By | Actual Status |
|------|-------|-------|------------|---------------|
| `jcode-pzl` | 8.1 | Delete src/cli/terminal.rs | `jcode-lvl` | Not started |
| `jcode-z5h` | 8.2 | Replace TestBackend tests | `jcode-pzl` | Not started — 16 test files |
| `jcode-kcu` | 8.3 | Full integration | `jcode-z5h` | Not started |
| `jcode-e6y` | 8.4 | Benchmark | `jcode-kcu` | Not started |

### Open — Fix Workflow (sequential chain)

| Bead | Phase | Title | Blocked By | Actual Status |
|------|-------|-------|------------|---------------|
| `jcode-fix-4-app-module-j0j` | Fix-4 | Port src/tui/app/ module (~400 errors) | `fix-3` ✅ | Not started |
| `jcode-fix-5-workspace-usage-g9e` | Fix-5 | Fix workspace_client + usage_overlay | `fix-4` | Not started |
| `jcode-fix-6-info-widgets-n3c` | Fix-6 | Fix info_widget*.rs mismatches | `fix-5` | Not started |
| `jcode-fix-7-final-cleanup-wze` | Fix-7 | Final cleanup — remove ratatui | `fix-6` | Not started |

---

## Execution Priority Order

### 🔴 Critical Path (unblocks the most beads)

1. **Complete `jcode-4we`** — 18 beads depend on this
2. **Complete stub crates** — `jcode-tui-messages`, `jcode-tui-markdown`, `jcode-tui-mermaid`
3. **Port `ui_prepare.rs`** — 86 errors, biggest single file
4. **Port `usage_overlay.rs`** — 49 errors, completely unported

### 🟡 High Priority (large error counts)

5. **Fix `ui_viewport.rs`** — 59 errors
6. **Fix `ui_diagram_pane.rs`** — 52 errors
7. **Fix `ui_header.rs`** — 18 errors (not ported)
8. **Fix `permissions.rs`** — 13 errors (replace `ratatui::init/restore`)
9. **Fix `workspace_client.rs`** — 21 errors (API mismatch)

### 🟢 Medium Priority (partial ports, smaller fixes)

10. Fix `ui_messages.rs` — 28 errors
11. Fix `session_picker.rs` — 28 errors
12. Fix `login_picker.rs` — 26 errors
13. Fix `account_picker*.rs` — 21 errors combined
14. Fix `ui_file_diff.rs` — 18 errors
15. Fix `ui_overlays.rs` — 11 errors
16. Fix `ui_frame_metrics.rs` — 11 errors
17. Fix `app/run_shell.rs` — 10 errors

### ⚪ Low Priority (after compile succeeds)

18. Fix frankentui-side errors (76 errors in ftui crates)
19. Port 16 test files to ftui-harness
20. Remove `ratatui` from workspace `Cargo.toml`
21. Remove `crossterm` direct dependency
22. Delete `src/cli/terminal.rs`
23. Run full `cargo test --workspace`
24. Benchmark frame times

---

## Files That Compile Cleanly ✅

These files have been fully ported (only warnings, no errors):

```
src/tui/ui/ui_animations.rs
src/tui/ui/ui_box.rs
src/tui/ui/ui_diff.rs
src/tui/ui/ui_file_diff.rs         (was ported, but still 18 errors from elsewhere)
src/tui/ui/ui_inline.rs
src/tui/ui/ui_inline_interactive.rs
src/tui/ui/ui_input.rs
src/tui/ui/ui_memory.rs
src/tui/ui/ui_messages.rs
src/tui/ui/ui_messages_cache.rs
src/tui/ui/ui_overlays.rs
src/tui/ui/ui_pinned.rs
src/tui/ui/ui_pinned_selection.rs
src/tui/ui/ui_pinned_utils.rs
```

---

## Key Reference: ratatui → ftui Type Mapping

| ratatui | ftui |
|---------|------|
| `Style::default().fg(c)` | `Style::new().fg(PackedRgba::WHITE)` or `.fg(color_to_packedrgba(&c))` |
| `.add_modifier(Modifier::BOLD)` | `.bold()` |
| `Color::White` | `Color::Mono(MonoColor::White)` or `PackedRgba::WHITE` |
| `Color::Rgb(r,g,b)` | `Color::Rgb(Rgb::new(r,g,b))` |
| `Line::from(vec![...])` | `Line::from_spans(vec![...])` |
| `Text::from(lines)` | `Text::from_lines(lines)` |
| `Layout::default().direction(Vertical)` | `Flex::vertical()` |
| `Constraint::Length(n)` | `Constraint::Fixed(n)` |
| `Constraint::Percentage(n)` | `Constraint::Percentage(n as f32)` |
| `frame.area()` | `Rect::new(0, 0, frame.buffer.width(), frame.buffer.height())` |
| `frame.buffer_mut()` | `&mut frame.buffer` |
| `.wrap(Wrap { trim: false })` | `.wrap(WrapMode::Word)` |
| `Block::bordered()` | `Block::new().borders(BorderSet::ALL)` |
| `DefaultTerminal` | `ftui_tty::TtyBackend` |
| `ratatui::init()` / `restore()` | `ftui_tty::TtyBackend::new()` / `drop()` |
| `TestBackend::new(w, h)` | `ftui_harness::render_test::<T>(model, area)` |

---

## Verification Commands

```bash
# Error count
cargo check 2>&1 | grep "^error\[" | wc -l

# Errors by file
cargo check 2>&1 | grep -E "^   --> " | awk '{print $2}' | cut -d: -f1 | sort | uniq -c | sort -rn | head -20

# Remaining ratatui imports
rg "use ratatui" --type rust -l | wc -l

# Remaining ratatui references
rg "ratatui" --type rust -c | awk -F: '{sum+=$2} END {print sum}'

# Full test suite (after 0 errors)
cargo test --workspace
```
