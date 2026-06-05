# jcode-mempalace-adapter

Type-conversion layer between jcode's `MemoryEntry` and mempalace's `Drawer`.

## What's here

- **`convert` module** — bidirectional 1:1 conversions between:
  - `MemoryCategory` ↔ `DrawerKind` (including `Entity`, `Correction`, `Custom`)
  - `MemoryEntry` ↔ `Drawer` (all fields mapped)
  - `MemoryScope` ↔ `MemoryScope` (Project→Local, Global→Global, All→All)
  - `TrustLevel` ↔ String
  - `Reinforcement` ↔ `MpReinforcement`

- **Mirror types** (`Drawer`, `DrawerKind`, `DrawerId`, `MemoryScope`) — local
  definitions that match mempalace's public surface exactly, exported for
  downstream crates that need to construct mempalace-shaped values without
  pulling in the full `mempalace-core` crate.

## Why no mempalace-core dependency?

mempalace-core depends on `rusqlite 0.32` while jcode uses `rusqlite 0.33`
(via `cross_agent_session_resumer`). Both versions link to the native `sqlite3`
library, which cargo's resolver disallows. The mirror-type approach avoids this
entirely — zero conflict, always compiles.

