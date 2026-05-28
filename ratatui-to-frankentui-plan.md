# Porting Plan: jcode — Ratatui 0.30 → FrankenTUI (100% Migration)

## Executive Summary

**Goal**: Migrate jcode's entire TUI layer from `ratatui 0.30` to `frankentui`, replacing all 100+ files across 8 TUI crates. This is a full framework swap — not an adapter layer — native frankentui throughout.

**Approach**: Incremental phases starting from the dependency leaves and working toward the render core.

**Effort**: 7–12 weeks, depending on team size and parallelization.

---

## 1. Repository & Architecture Analysis

### 1.1 jcode TUI Surface Area

**Workspace ratatui dependency:**
```toml
# /data/projects/jcode/Cargo.toml:185
ratatui = "0.30"
crossterm = { version = "0.29", features = ["event-stream"] }
```

**8 TUI crates with ratatui dependencies:**

| Crate | Purpose | Ratatui Types Used |
|-------|---------|-------------------|
| `jcode-tui-style` | Color system, themes | `Color`, `Style`, `Modifier` |
| `jcode-tui-messages` | Message rendering, prepared frames | `Line`, `Span`, `Alignment`, `Rect` |
| `jcode-tui-render` | Chrome, layout utils, buffer ops | `Frame`, `Rect`, `Block`, `Borders`, `Buffer` |
| `jcode-tui-workspace` | Pane workspace, color map | `Buffer`, `Rect`, `Style`, `Color`, `Modifier` |
| `jcode-tui-mermaid` | Mermaid diagram rendering | `StatefulWidget` (via `ratatui_image`) |
| `jcode-tui-markdown` | Markdown rendering | `ratatui::prelude::*` |
| `jcode-tui-usage-overlay` | Usage overlay | `Style`, `Color`, `Paragraph` |
| `jcode-tui-session-picker` | Session picker | `Layout`, `Constraint`, `Direction`, `Style` |
| `jcode-tui-tool-display` | Tool display rendering | `Style`, `Color`, `Line`, `Span` |

**Core module files** (`src/tui/`):

| File | Purpose | Key ratatui types |
|------|---------|-------------------|
| `mod.rs` | TUI module hub, `TuiState` trait (~60 methods) | `Frame`, `Line` |
| `app.rs` | `App` struct (200+ fields), run loop | `DefaultTerminal` |
| `ui.rs` | Main `draw()` function (2400+ lines), render pipeline | `Frame`, `Paragraph`, `Style`, `Rect`, `Buffer` |
| `terminal.rs` | Terminal init/cleanup using `CrosstermBackend` | `Terminal`, `CrosstermBackend`, `DefaultTerminal` |
| `ui_header.rs` | Header rendering | `Color::Rgb`, `Style::default().fg()` |
| `ui_input.rs` | Input widget | `Modifier::BOLD`, `Style` chaining |
| `ui_messages.rs` | Message rendering | `Span`, `Line`, `Style` |
| `session_picker.rs` | Session picker UI | `Layout`, `Constraint`, `Direction`, `Paragraph` |
| `login_picker.rs` | Login picker | `Layout`, `Color`, `Style` |
| `info_widget.rs` | Info widget entry | Widget rendering entry |
| `info_widget_*.rs` | Git, model, usage, layout, todos widgets | Various |
| `account_picker.rs` | Account picker | `TestBackend`, `Terminal` |
| `ui_viewport.rs`, `ui_pinned.rs`, `ui_overlays.rs` | Viewport, pinned, overlays | Rendering contexts |
| `ui_test*.rs` | Tests | `TestBackend` |

**Total files with ratatui imports: 100+**

### 1.2 FrankenTUI Architecture

**20-crate workspace** at `/data/projects/frankentui/`:

| Crate | Purpose | Key API |
|-------|---------|---------|
| `ftui-core` | Terminal lifecycle, events, input | `TerminalSession`, `InputParser`, `Event` |
| `ftui-render` | Buffer, diff, presenter, Frame | `Frame { buffer, hit_grid, cursor }`, `BufferDiff` |
| `ftui-runtime` | Elm runtime, model, cmds, subs | `Model`, `update() → Cmd`, `view()` |
| `ftui-widgets` | 80+ widgets | `Widget::draw(&self, ctx, area)`, `StatefulWidget` |
| `ftui-layout` | Flex/Grid layout solver | `FlexLayout`, `Constraint` (Fixed, Percent, Flex, Min, Max) |
| `ftui-text` | Rope editor, Span, Line | `Span`, `Line`, `Segment`, `Rope` |
| `ftui-style` | Style, Color, Theme | `Style { fg, bg, modifiers }`, `Color` |
| `ftui-backend` | Backend abstraction | Backend trait |
| `ftui-tty` | Native TTY backend | Unix escape sequences |
| `ftui-web` | Web/WASM backend | browser WebSocket |

**Widget trait signature:**
```rust
pub trait Widget {
    fn draw(&self, ctx: &mut Fruictx, area: Rect);
}
pub trait StatefulWidget {
    type State;
    fn draw(&self, ctx: &mut Fruictx, area: Rect, state: &mut Self::State);
}
```

**Frame vs Ratatui Frame:**
```rust
// frankentui Frame (ftui-render/src/frame.rs)
pub struct Frame {
    pub buffer: Buffer,      // cell grid
    hit_grid: HitGrid,      // clickable regions
    cursor: CellAnchor,     // cursor position
    pub clip: Rect,         // clipping region
    // ...
}

// Ratatui Frame wraps buf + provides render_widget()
```

**Layout vs Ratatui Layout:**
```rust
// frankentui (ftui-layout)
FlexLayout::new()
    .direction(Direction::Row)
    .items([...])
    .gap(Gap::Px(1))
    .align(Align::Center);

// maps to ratatui:
Layout::default()
    .direction(Direction::Horizontal)
    .constraints([Constraint::Length(3), Constraint::Percentage(50)])
    .flex(Flex::Center)
```

**Style builder (ftui-style):**
```rust
Style::new()
    .foreground(Color::Red)
    .background(Color::Blue)
    .add_modifier(StyleModifier::Bold | StyleModifier::Italic)
```

---

## 2. Porting Strategy

### 2.1 Architectural Shift

jcode currently uses **immediate mode + buffer diffing** (ratatui pattern):
```
Widget::render(area, buf) → Buffer → BufferDiff → stdout
```

FrankenTUI uses an **Elm/Bubbletea reactive model**:
```
Model → view(frame) → Frame → BufferDiff → Presenter → stdout
```

**Key implication**: The entire `draw()` function in `ui.rs` must be decomposed into `view()` methods on frankentui `Model` types, with `update()` handlers for events that return `Cmd` (commands/subscriptions).

### 2.2 Migration Paths

| Path | Effort | Risk | Outcome |
|------|--------|------|---------|
| **A: Full Native Rewrite** | High | High | Pure frankentui — no ratatui surface left |
| **B: Adapter Layer** | Medium | Low | Thin shim using frankentui under the hood |
| **C: Hybrid (incremental)** | Medium | Medium | Rewrite widget by widget, gateway at Terminal |

**Recommendation**: Path A (Full Native Rewrite) — frankentui and ratatui models are too different for a transparent adapter. The Elm model is cleaner and more maintainable.

### 2.3 Phase Overview

```
Phase 1: Foundation Strip
  ├── Remove ratatui from Cargo.toml deps
  ├── Define frankentui Model/State types
  └── Establish frankentui runtime & Event loop

Phase 2: Style & Color Bridge
  ├── Port jcode-tui-style → frankentui Style/Color
  ├── Port theme system
  └── Validate color rendering

Phase 3: Layout & Geometry
  ├── Port jcode-tui-render layout utils
  ├── Convert Constraint/-direction usage to FlexLayout
  └── Validate rect/area operations

Phase 4: Core Widgets (Text)
  ├── Port Paragraph, Line, Span rendering
  ├── Port jcode-tui-messages
  ├── Port jcode-tui-markdown
  └── Validate text wrapping, alignment

Phase 5: Workspace & Pane System
  ├── Port jcode-tui-workspace
  ├── Map pane layout to frankentui pane workspace
  └── Validate resize/drag behavior

Phase 6: Interactive Widgets
  ├── Port ui_input (text input)
  ├── Port session_picker
  ├── Port login_picker
  ├── Port info_widget series
  └── Validate keyboard/mouse events

Phase 7: Diagram & Media
  ├── Port jcode-tui-mermaid via frankentui image widget
  ├── Integrate via frankentui image rendering pipeline
  └── Validate Mermaid output

Phase 8: Integration & Testing
  ├── Wire complete render pipeline
  ├── Run full test suite
  ├── Benchmark frame times
  └── Fix rendering edge cases
```

---

## 3. Phase-by-Phase Implementation Plan

### Phase 1: Foundation Strip (Week 1–2)

#### Step 1.1: Update Cargo Workspace Dependencies

**Remove from workspace** `/data/projects/jcode/Cargo.toml`:
```toml
# REMOVE:
ratatui = "0.30"

# ADD:
frankentui = { path = "../frankentui" }  # or git reference if frankentui is external
```

**Update each crate's Cargo.toml**:
```toml
# All 8 jcode-tui-* crates:
[dependencies]
- ratatui = "0.30"
- crossterm = { version = "0.29", features = ["event-stream"] }
+ frankentui = { path = "../../frankentui" }
+ ftui-tty = { path = "../../frankentui/crates/ftui-tty" }
```

#### Step 1.2: Define FrankenTUI Model Types

**Create `Model` in `src/tui/`** — replaces the current `TuiState` trait and most of the `App` struct:

```rust
// src/tui/model.rs
use ftui_runtime::{Model, Cmd, Subscription};
use ftui_render::Frame;
use ftui_layout::Rect;

pub struct Model {
    // From App struct: messages, scroll_state, streaming_buf, etc.
    pub messages: Vec<DisplayMessage>,
    pub scroll_state: ScrollState,
    pub input_buffer: String,
    pub session_picker: SessionPickerState,
    pub login_picker: LoginPickerState,
    // ... all 200+ fields from App
}

impl Model {
    pub fn new(/* ... */) -> Self { ... }
}

impl Update for Model {
    fn update(&mut self, msg: Msg) -> Cmd<Self> {
        match msg {
            Msg::UpdateMessages(m) => { ... Cmd::none() }
            Msg::Scroll(d) => { scroll(&mut self.scroll_state, d); Cmd::none() }
            Msg::InputSubmit => { submit_input(&self.input_buffer); Cmd::none() }
            Msg::Resize(w, h) => { /* update rects */ Cmd::none() }
            _ => Cmd::none()
        }
    }
}

impl View for Model {
    fn view(&self, frame: &mut Frame) {
        // This replaces ui::draw() — called each frame
    }
}
```

**Key Messages:**
```rust
enum Msg {
    UpdateMessages(Vec<DisplayMessage>),
    AppendStreamingChunk(String),
    Scroll(ScrollDelta),
    InputKey(KeyEvent),
    InputSubmit,
    ToggleSessionPicker,
    ToggleLoginPicker,
    Resize(u16, u16),
    // ... one variant per TuiState method
}
```

#### Step 1.3: Create Runtime Kernel

**Replace `app.rs` run loop** — from manual event polling + terminal.draw() to frankentui runtime:

```rust
// src/tui/app.rs (new)
use ftui_runtime::{program, Program};
use ftui_backend::Backend;
use ftui_tty::TtyBackend;

impl Application for Model {
    type Msg = Msg;
    type Dependencies = ();
}

#[tokio::main]
async fn main() -> Result<()> {
    let backend = TtyBackend::new()?;
    let model = Model::new(/* ... */)?;
    Program::new(backend, model).run().await
}
```

**Replaces: `/data/projects/jcode/src/cli/terminal.rs`** — frankentui's backend does raw mode, alternate screen, cleanup automatically.

#### Step 1.4: Stub All Views with Empty Render

Start with a minimal `view()` that renders nothing. Compile. Verify frankentui runtime boots. Then proceed.

**Deliverable**: Compiles with frankentui runtime kernel in place, App struct replaced with Model, run loop replaced with frankentui Program.

---

### Phase 2: Style & Color Bridge (Week 2–3)

#### Step 2.1: Port `jcode-tui-style`

**File**: `/data/projects/jcode/crates/jcode-tui-style/src/`

**Before (ratatui):**
```rust
use ratatui::style::{Color, Style, Modifier};
use ratatui::prelude::*;
```

**After (frankentui):**
```rust
use ftui_style::{Color, Style, ColorProfile};
use ftui_style::color::{rgb, ansi256};
```

**Key conversions:**

| jcode pattern | frankentui equivalent |
|--------------|----------------------|
| `Color::Rgb(r,g,b)` | `Color::Rgba(r, g, b, 255)` |
| `Color::Indexed(n)` | `Color::Index(n)` |
| `rgb(255, 213, 128)` | `Color::Rgba(255, 213, 128, 255)` |
| `Style::default().fg(c).add_modifier(MODIFIER_BOLD)` | `Style::new().foreground(c).add_modifier(StyleModifier::Bold)` |
| `blend_color(a, b, t)` | `Color::blend(a, b, ratio)` |
| `rainbow_prompt_color(i)` | `Color::rainbow(position)` |

**`jcode_tui_style::color_support()`** — frankentui auto-downgrades colors based on terminal capability (WCAG contrast check), so this may be simplified.

#### Step 2.2: Port Theme System

**File**: `/data/projects/jcode/crates/jcode-tui-style/src/theme.rs`

FrankenTUI's `ftui-style` includes WCAG contrast checking and auto-downgrade. Map jcode's theme constants to frankentui `ColorPalette` values.

#### Step 2.3: Update `jcode-tui-usage-overlay`

Uses `Paragraph`, `Block`, style helpers. These map directly to frankentui equivalents.

---

### Phase 3: Layout & Geometry (Week 3–4)

#### Step 3.1: Map Layout Patterns

**Common jcode pattern** in `session_picker.rs`, `login_picker.rs`:
```rust
// RATATUI:
let v_chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints(v_constraints)
    .split(frame.area());

let chunks = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
    .split(area);
```

**After** (frankentui):
```rust
use ftui_layout::{FlexLayout, Direction, Constraint, Align};

let v_chunks = FlexLayout::new()
    .direction(Direction::Vertical)
    .constraints(v_constraints.iter().map(|c| ftui_layout::Constraint::from(*c)))
    .split(area);

// frankentui constraint mapping:
Constraint::Percentage(40) → ftui_layout::Constraint::Percent(40)
Constraint::Length(n)      → ftui_layout::Constraint::Fixed(n)
Constraint::Fill(1)         → ftui_layout::Constraint::Flex(1)
Constraint::Min(n)         → ftui_layout::Constraint::Min(n)
```

**Edge case**: Ratatui's `Constraint::Fill` can fill remaining space with a weight. FrankenTUI's equivalent is `Constraint::Flex(weight)`.

#### Step 3.2: Geometry Utilities

**File**: `/data/projects/jcode/crates/jcode-tui-render/src/layout.rs`

jcode has `rect_contains`, `point_in_rect`, `rect_intersection`. FrankenTUI's `ftui-core` geometry module has equivalent functions. Replace jcode's utils with direct calls to `ftui_core::geometry::*`.

#### Step 3.3: Port Chrome/Buffer Operations

**File**: `/data/projects/jcode/crates/jcode-tui-render/src/chrome.rs`

Used for clearing areas, drawing rails, borders. FrankenTUI's `Block` widget handles box drawing with borders. `frame.buffer_mut()` direct buffer manipulation becomes `ctx.frame().buffer_mut()`.

---

### Phase 4: Core Widgets (Week 4–6)

#### Step 4.1: Message Rendering — `jcode-tui-messages`

**Files**: `prepared.rs` (290 lines), `cache.rs`, `message.rs`, `wrapped_line_map.rs`

This is the most complex crate. It:
- Pre-computes wrapped lines for messages
- Builds `PreparedChatFrame` with rect areas for each pane
- Uses `Line`, `Span`, `Alignment` extensively
- Has per-frame caching via `OnceLock`/`Mutex`

**Porting approach**:
1. Convert `DisplayMessage` and `PreparedMessages` types to frankentui-compatible
2. Replace `ratatui::layout::Alignment` → `ftui_layout::Align`
3. Replace `ratatui::text::Line` → `ftui_text::Line` (most direct translation)
4. Port `left_pad_lines_for_centered_mode()` and `centered_wrap_width()` to use frankentui text wrapping
5. `get_cached_message_lines()` caching pattern stays similar (`OnceLock` is stdlib)

#### Step 4.2: Markdown Rendering — `jcode-tui-markdown`

**File**: `/data/projects/jcode/crates/jcode-tui-markdown/src/lib.rs`

Uses `ratatui::prelude::*`. FrankenTUI's `Textarea` widget or `Paragraph` with markdown-style rendering. May need a custom widget adapter if frankentui doesn't have built-in markdown.

#### Step 4.3: UI Draw Function — `src/tui/ui.rs`

At **2400+ lines**, this is the centerpiece. Decompose into `view()` methods on `Model` types:

```rust
// BEFORE (ratatui):
pub(crate) fn draw(frame: &mut Frame, state: &dyn TuiState) {
    let chat_area = layout::compute_chat_area(...);
    let chunks = Layout::default()...split(chat_area);
    for chunk in chunks {
        frame.render_widget(Paragraph::new(...), chunk);
    }
}

// AFTER (frankentui):
impl View for Model {
    fn view(&self, frame: &mut Frame) {
        let chat_area = self.compute_chat_area();
        let v_chunks = FlexLayout::new()
            .direction(Direction::Vertical)
            .constraints([...])
            .split(chat_area);
        for chunk in v_chunks {
            if let Some(msg) = self.messages.get(chunk.index) {
                Paragraph::new(msg.lines.clone())
                    .alignment(ftui_layout::Align::Left)
                    .draw(ctx, chunk);
            }
        }
    }
}
```

**Sub-modules to migrate** from `src/tui/ui_*.rs`:
- `ui_header.rs` — rendered as `Block` with header content
- `ui_input.rs` — replace with frankentui `TextInput` widget
- `ui_messages.rs` — delegate to `jcode-tui-messages` crate
- `ui_transitions.rs` — frankentui handles some animations natively
- `ui_animations.rs` — frankentui has built-in animation system
- `ui_memory.rs` — info widget
- `ui_file_diff.rs` — diff pane
- `ui_pinned*.rs` — pinned items

---

### Phase 5: Workspace & Pane System (Week 6–7)

**Crate**: `jcode-tui-workspace`

FrankenTUI has its own **pane workspace system** built into `ftui-layout` and `ftui-core`:
- Drag-to-resize panes
- Magnetic docking
- Inertial throw
- Resizable workspace via pane indices

This replaces jcode's custom pane management, which used `Buffer` ops and manual `Rect` splitting.

**Action**: Delete `jcode-tui-workspace/src/workspace_map_widget.rs` and `workspace_map.rs`. Replace with frankentui's pane workspace API. The workspace is defined declaratively:

```rust
let workspace = PaneWorkspace::new()
    .split(Direction::Horizontal, [40, 60])
    .split(Direction::Vertical, ["chat", "pinned"])
    .resize("chat", 30)
```

---

### Phase 6: Interactive Widgets (Week 7–9)

#### Step 6.1: Session Picker — `session_picker.rs`

**Pattern**: `Layout`, `Constraint`, `Direction`, `Style`, `Color`, `Paragraph` for each session row.

FrankenTUI equivalent: `List` widget with custom row renderer. Keyboard navigation via frankentui subscriptions.

#### Step 6.2: Login Picker — `login_picker.rs`

Similar to session picker. Port to `List` + `Block` framing.

#### Step 6.3: Account Picker — `account_picker.rs`

Uses `TestBackend` for rendering tests. This test setup changes to use frankentui's test harness (`ftui-harness`).

#### Step 6.4: Info Widgets — `info_widget*.rs`

Each info widget (git, model, usage, layout, todos, swarm_background):

**Before**: `impl Widget for InfoWidgetGit` with `frame.render_widget(...)` calls

**After**: Each becomes a frankentui `Widget` implementation. frankentui's pane system makes positioning simpler.

---

### Phase 7: Diagram & Media (Week 9–10)

#### Step 7.1: Mermaid — `jcode-tui-mermaid`

**Current**: Uses `ratatui_image::StatefulImage` which implements `StatefulWidget`.

**Port**: FrankenTUI's `Image` widget supports image rendering. The `mermaid-rs-renderer` (jcode's custom Rust library) can be embedded in frankentui's render pipeline.

**Action**: Replace `ratatui_image::StatefulImage` with frankentui's `Image` widget, feeding it the rendered image data from the mermaid renderer.

---

### Phase 8: Integration & Testing (Week 10–12)

#### Step 8.1: Terminal Backend Cleanup

**Remove** `/data/projects/jcode/src/cli/terminal.rs` — frankentui handles raw mode, alternate screen, cleanup automatically.

Ratatui's `Terminal::new(CrosstermBackend::new(stdout))` → frankentui's `TtyBackend::new()`.

#### Step 8.2: Test Infrastructure

**Before**: Uses `TestBackend` from ratatui for snapshot tests.

**After**: Use `ftui-harness` for snapshot testing with frankentui's shadow-run framework.

#### Step 8.3: Run Full Test Suite

```bash
cd /data/projects/jcode
cargo test --workspace
```

Fix any rendering regressions. FrankenTUI's deterministic rendering should reduce flakes.

#### Step 8.4: Benchmark

Compare frame times before/after migration. FrankenTUI's optimized rendering pipeline should maintain or improve jcode's current 1000+ FPS baseline.

---

## 4. File-by-File Migration Table

| File | Phase | Action |
|---   |------ |--------|
| `Cargo.toml` | 1 | Remove ratatui dep, add frankentui deps |
| `src/cli/terminal.rs` | 1 | Delete entire file (frankentui backend handles) |
| `src/tui/mod.rs` | 1 | Update TuiState trait signatures for frankentui types |
| `src/tui/app.rs` | 1 | Replace App struct with Model, run loop with Program |
| `src/tui/ui.rs` | 4 | Decompose draw() → view() methods on Model |
| `src/tui/app/input.rs` | 6 | Port to frankentui TextInput widget |
| `src/tui/app/replay.rs` | 6 | Update replay to use frankentui Backend |
| `src/tui/app/remote.rs` | 6 | Remote event handling via frankentui subscriptions |
| `src/tui/ui_header.rs` | 4 | Port to Block + styled spans |
| `src/tui/ui_input.rs` | 6 | Port to frankentui TextInput |
| `src/tui/ui_messages.rs` | 4 | Port to jcode-tui-messages crate (updated) |
| `src/tui/ui_viewport.rs` | 4 | Viewport scroll via frankentui scrollable |
| `src/tui/ui_pinned*.rs` | 6 | Port all pinned widget variants |
| `src/tui/ui_overlays.rs` | 6 | Overlay system |
| `src/tui/session_picker.rs` | 6 | Port to List widget |
| `src/tui/login_picker.rs` | 6 | Port to List widget |
| `src/tui/account_picker.rs` | 6 | Port to List, update tests |
| `src/tui/info_widget*.rs` | 6 | Port all 8 info widget types |
| `crates/jcode-tui-style/src/lib.rs` | 2 | Re-export from frankentui Style |
| `crates/jcode-tui-style/src/color.rs` | 2 | Map to ftui_style::Color |
| `crates/jcode-tui-style/src/theme.rs` | 2 | Map to ftui_style theme system |
| `crates/jcode-tui-messages/src/lib.rs` | 4 | Update exports |
| `crates/jcode-tui-messages/src/cache.rs` | 4 | Use ftui_layout::Align |
| `crates/jcode-tui-messages/src/prepared.rs` | 4 | Use ftui_text::{Line, Span} |
| `crates/jcode-tui-messages/src/message.rs` | 4 | Text types updated |
| `crates/jcode-tui-render/src/lib.rs` | 3 | Update chrome/buffer utils |
| `crates/jcode-tui-render/src/chrome.rs` | 3 | Port to frankentui Block |
| `crates/jcode-tui-render/src/layout.rs` | 3 | Use ftui_core::geometry |
| `crates/jcode-tui-workspace/src/lib.rs` | 5 | Replace with frankentui pane system |
| `crates/jcode-tui-workspace/src/workspace_map_widget.rs` | 5 | Delete (frankentui pane handles) |
| `crates/jcode-tui-workspace/src/workspace_map.rs` | 5 | Delete |
| `crates/jcode-tui-workspace/src/color_support.rs` | 2 | Port to ftui_style color |
| `crates/jcode-tui-mermaid/src/lib.rs` | 7 | Update StatefulWidget impl |
| `crates/jcode-tui-mermaid/src/mermaid_widget.rs` | 7 | Port to frankentui Image |
| `crates/jcode-tui-markdown/src/lib.rs` | 4 | Port markdown rendering |
| `crates/jcode-tui-usage-overlay/src/lib.rs` | 2 | Port style to frankentui |
| `crates/jcode-tui-session-picker/src/lib.rs` | 6 | Port to List + flex layout |
| `crates/jcode-tui-tool-display/src/lib.rs` | 4 | Line/Span rendering |

---

## 5. Effort Estimation

| Phase | Scope | Estimated Weeks |
|-------|-------|----------------|
| 1. Foundation Strip | Workspace deps, Model, runtime kernel | 1–2 |
| 2. Style & Color Bridge | jcode-tui-style, 2 sub-crates | 1–2 |
| 3. Layout & Geometry | jcode-tui-render, layout utils | 1–2 |
| 4. Core Widgets (Text) | jcode-tui-messages, ui.rs, markdown | 2–3 |
| 5.  Workspace & Panes | jcode-tui-workspace → frankentui pane | 1–2 |
| 6. Interactive Widgets | Session picker, login picker, info widgets | 2–3 |
| 7. Diagram & Media | jcode-tui-mermaid | 1–2 |
| 8. Integration & Testing | Full pipeline, tests, benchmarks | 1–2 |
| **Total** | | **9–14 weeks** |

**Note**: Phases 4 and 6 are the largest — they contain the most rendering code. Parallelization across 2 engineers can cut 4–6 weeks off the total.

---

## 6. Key Technical Decisions

### 6.1 Ratatui → FrankenTUI Type Mapping

| Ratatui Type | FrankenTUI Type |
|-------------|----------------|
| `Frame<'_>` | `Frame` (buffer + hit_grid + cursor + clip) |
| `Buffer` | `Buffer` (16-byte cells, grapheme-aware) |
| `Cell` | `Cell` (`CellContent` + `PackedRgba` × 2 + `CellAttrs` + link_id) |
| `Rect` | `Rect` (`{ x, y, width, height }`) |
| `Layout` + `Constraint::Length/Percentage/Fill` | `FlexLayout` + `Constraint::Fixed/Percent/Flex` |
| `Direction::Vertical` | `Direction::Col` |
| `Direction::Horizontal` | `Direction::Row` |
| `Style::default().fg(c).bg(c2).add_modifier(M::BOLD)` | `Style::new().foreground(c).background(c2).add_modifier(StyleModifier::Bold)` |
| `Color::Rgb(r,g,b)` | `Color::Rgba(r,g,b,255)` |
| `Color::Indexed(n)` | `Color::Index(n)` |
| `Modifier` | `StyleModifier` |
| `Line` + `Span` | `ftui_text::Line` + `ftui_text::Span` |
| `Paragraph` | `Paragraph` (same name, different crate) |
| `Block` | `Block` (same name, different crate) |
| `Borders` | `BorderSet` + `BorderType` |
| `Paragraph::new(text).block(Block::bordered())` | `Paragraph::new(text).block(Block::new().borders(BorderSet::ALL))` |
| `frame.render_widget(Paragraph::new(), area)` | `widget.draw(ctx, area)` |
| `DefaultTerminal` = `Terminal< CrosstermBackend<Stdout>>` | `TtyBackend` |

### 6.2 Frame Access Patterns

**Before**:
```rust
frame.render_widget(Paragraph::new(text), area);
frame.buffer_mut().get_mut(...).set_char(...);
frame.buffer().cell(...);
```

**After** (frankentui uses `Fruictx`):
```rust
Paragraph::new(text)
    .block(Block::new().borders(BorderSet::ALL))
    .draw(ctx, area);
// Direct buffer access via ctx.frame().buffer_mut()
```

### 6.3 Backend

**Before**: `CrosstermBackend` wrapping `Stdout`. Raw mode via `crossterm::terminal`.

**After**: `TtyBackend` from `ftui-tty` — no external crossterm dep. FrankenTUI's ftui-tty handles all escape sequences natively.

### 6.4 Event Handling

**Before**: `crossterm::event::Event` passed to app manually.

**After**: FrankenTUI's `Event` type flows through `Subscription` into `update()` as `Msg` variants (Elm pattern).

### 6.5 Testing

**Before**: `let backend = TestBackend::new(40, 12); let mut terminal = Terminal::new(backend)?;`

**After**: `ftui_harness::render_test::<T>(model, area)` — snapshot-based with deterministic output.

---

## 7. Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Elm model size** | `Model` may have 200+ fields initially — big bang change | Phase 1 stubs with empty views; incremental `view()` fills |
| **ratatui_image incompatibility** | Mermaid uses `StatefulImage` which won't exist | Port mermaid renderer to use frankentui Image widget |
| **Layout constraint expressiveness** | Some ratatui layouts may not map precisely | Document edge cases; use frankentui Flex for most layouts |
| **Text wrapping differences** | `ratatui::wrap`/`ftui_text::wrap` algorithms differ | Test all message render paths; may need custom wrapper |
| **TestBackend removal** | Many tests use `TestBackend` for snapshot testing | Replace with `ftui_harness` snapshot testing |
| **Frame rate regression** | FrankenTUI has more infrastructure (Bayesian diff, hit grid) | Benchmark early (bi-weekly check); optimize hot paths |
| **Self-dev loop** | FrankenTUI has its own self-dev mechanism | Coordinate jcode's self-dev with frankentui's hot reload |

---

## 8. Next Steps

1. **This session**: Confirm scope, authorize Phase 1 start
2. **Phase 1.1**: Update `Cargo.toml` — remove ratatui, add frankentui deps
3. **Phase 1.2**: Create `Model` type in `src/tui/model.rs`
4. **Phase 1.3**: Create frankentui `Program` kernel to replace `app.rs` run loop
5. **Phase 1.4**: Get empty frankentui app compiling (minimal draw stub)
6. **Iterate** through phases 2–8 validating at each step

---

## Appendix A: FrankenTUI Key Files

| File | Purpose |
|------|---------|
| `ftui-widgets/src/lib.rs` | Widget trait, State, 80+ widget implementations |
| `ftui-runtime/src/program.rs` | Elm runtime, Model trait, Cmd, Subscription |
| `ftui-render/src/frame.rs` | Frame struct (Buffer + hit_grid + cursor + clip) |
| `ftui-render/src/buffer.rs` | Buffer/Cell structure, 16-byte cells |
| `ftui-layout/src/flex.rs` | FlexLayout constraint solver |
| `ftui-style/src/style.rs` | Style builder, CSS-like cascading |
| `ftui-text/src/lib.rs` | Span, Line, Segment, Rope text types |
| `ftui-core/src/geometry.rs` | Rect, Size, Point |
| `ftui-tty/src/lib.rs` | TtyBackend (terminal/backend) |

## Appendix B: Ratatui Key Files (commit 4493742)

| File | Purpose |
|------|---------|
| `ratatui-core/src/widgets/widget.rs` | Widget, StatefulWidget traits |
| `ratatui-core/src/terminal.rs` | Terminal draw loop |
| `ratatui-core/src/terminal/frame.rs` | Frame struct |
| `ratatui-core/src/buffer/buffer.rs` | Buffer struct |
| `ratatui-core/src/layout/layout.rs` | Layout solver (kasuari) |
| `ratatui-core/src/style.rs` | Style, Color, Modifier |
