# Origin Sync: Fork Sync Skill for jcode (quangdang46/jcode)

## Problem

This repo is a **fork** of `1jehuang/jcode`. Multiple modules have been extracted into separate repos under `github.com/quangdang46/*`. When syncing from upstream, conflicts inevitably arise where upstream changes collide with code that was replaced by external repo references.

The naive approach (`git merge` + resolve everything by hand) breaks because:
- Upstream may modify code that now lives in an external repo → taking upstream's change **reverts the extraction**
- Upstream may modify adapter code (e.g., `casr_adapter.rs`, `dcg_bridge.rs`) — need to check if the external repo already handles this
- Upstream may add features that **should** be extracted but aren't yet → need to redirect

## Core Principle

> **The fork's code is the decorated state. Upstream changes that touch extracted domains must be redirected to the external repos, not reverted.**

---

## Extracted Module Map

| # | Cargo Dep Name | External Repo | Local Adapter/Bridge Files | Domain |
|---|---|---|---|---|
| 1 | `casr` | `quangdang46/cross_agent_session_resumer` | `crates/jcode-base/src/casr_adapter.rs` (748 lines), `crates/jcode-base/src/import.rs` (1002 lines) | Session import/resume (replaces `jcode-import-core`) |
| 2 | `ffs-search`, `ffs-engine` | `quangdang46/fast_file_search` | `crates/jcode-tui/src/tui/app/at_picker.rs` (uses `ffs_engine::mention` and `ffs_search::mention`) | File search, @-mention autocomplete |
| 3 | `dcg-core` | `quangdang46/destructive_command_guard` | `crates/jcode-app-core/src/dcg_bridge.rs` (740 lines) | Permission guard, YOLO classifier |
| 4 | `hashline` | `quangdang46/hashline` | `crates/jcode-app-core/src/tool/hashline_edit.rs` | SHA-256 anchored hashing |
| 5 | `mempalace-core` | `quangdang46/mempalace_rust` | `crates/jcode-mempalace-adapter/` (entire crate) | Memory palace |
| 6 | `dynamic_context_pruning` | `quangdang46/dynamic_context_pruning` | `crates/jcode-app-core/src/dcp_bridge.rs` (197 lines), `dcp_plugin.rs` | Context pruning |
| 7 | `rtco-core` | `quangdang46/rust_token_cost_optimizer` | `crates/jcode-app-core/src/rtco_filter.rs` | Token cost optimization |

### Additional git dependencies

| Dep | Owner | Domain |
|---|---|---|
| `agentgrep` | `1jehuang/agentgrep` | Code search (not extracted, from upstream contributor) |

### Not-yet-extracted domains that commonly conflict

- `crates/jcode-tui/src/tui/app/inline_interactive.rs` — session picker, resume logic (uses `casr` through `import.rs`)
- `crates/jcode-session-types/src/lib.rs` — `ResumeTarget` enum (has `ForeignSession` variant added locally)
- `src/cli/tui_launch.rs` — terminal launch (uses `casr_adapter` heavily)
- `crates/jcode-tui/src/tui/session_picker/` — session picker UI (has `ForeignSession` arms added locally)
- `crates/jcode-app-core/src/yolo_classifier.rs` — DCG integration

---

## Sync Workflow

### Step 0: Prerequisites

```bash
# Add upstream remote if missing
git remote add upstream https://github.com/1jehuang/jcode.git

# Verify upstream
git remote -v
# Should show:
#   origin    https://github.com/quangdang46/jcode.git (fetch/push)
#   upstream  https://github.com/1jehuang/jcode.git (fetch)

# Ensure master is clean
git checkout master
git status  # should be clean
```

### Step 1: Fetch upstream

```bash
git fetch upstream
```

### Step 2: Review upstream changes (before merge)

```bash
# See what's new from upstream since our last sync
git log --oneline master..upstream/master

# Check which files changed
git diff --stat master..upstream/master

# *** CRITICAL ***: Check if upstream has changed extracted module files
# If ANY of these files changed upstream, there WILL be conflicts
git diff --stat master..upstream/master -- \
  crates/jcode-base/src/casr_adapter.rs \
  crates/jcode-base/src/import.rs \
  crates/jcode-app-core/src/dcg_bridge.rs \
  crates/jcode-app-core/src/dcp_bridge.rs \
  crates/jcode-app-core/src/tool/hashline_edit.rs \
  crates/jcode-tui/src/tui/app/at_picker.rs \
  crates/jcode-tui/src/tui/app/inline_interactive.rs \
  crates/jcode-session-types/src/lib.rs \
  crates/jcode-tui/src/tui/session_picker/ \
  src/cli/tui_launch.rs
```

### Step 3: Merge

```bash
git merge upstream/master
```

**Do NOT use `git rebase`** for upstream sync — it rewrites history for the entire fork, making it impossible for collaborators to sync. Use `git merge`.

### Step 4: Conflict Classification & Resolution

For EACH conflicted file, classify the conflict:

#### Category A: Extracted Code Conflict

**Symptom**: Upstream changed code that was replaced by an external repo (casr, ffs, dcg, hashline, mempalace, dcp, rtco).

**Resolution**: **KEEP OUR VERSION. Discard upstream changes entirely.**

```bash
git checkout --ours -- <file>
git add <file>
```

**Reasoning**: The upstream's inline implementation was superseded by the external crate. Taking upstream changes would reintroduce the old inline code and break the dep chain.

**Exception**: If the upstream change also modifies the local adapter/bridge code in a way that's compatible with the external repo, check if the external repo already has equivalent support. If yes → forward-port the change to the external repo as a separate PR. If no → ask user.

#### Category B: Local Extension Conflict

**Symptom**: Conflict in files that have local additions not present upstream (e.g., `ForeignSession` variant in `ResumeTarget`, extra match arms).

**Resolution**: **KEEP OUR VERSION** for the local additions, but **INCORPORATE UPSTREAM'S** changes to shared code where they don't conflict.

```bash
# For each conflict hunk:
# - If the conflict is entirely about our local additions → keep ours
# - If upstream added something non-overlapping → incorporate both
# - If upstream changed the same area → need manual merge
```

**Sub-types**:
- **New enum variants added both sides** → keep both. This requires manual editing.
- **New match arms both sides** → keep both. Manual editing.
- **Upstream refactored the module structure** → compare carefully. If upstream moved code that references extracted deps, our paths need to stay.

#### Category C: Upstream-Only Change (no conflict)

**Symptom**: Upstream added/modified code that doesn't touch any extracted domain.

**Resolution**: **ACCEPT UPSTREAM** changes as-is.

```bash
git add <file>  # after the auto-merge stage already handled this
```

#### Category D: Third-Party Dep Change

**Symptom**: Upstream changed `Cargo.toml` or `Cargo.lock` — adding, removing, or updating dependencies.

**Resolution**: **CAREFUL MERGE**. Our `Cargo.toml` has git deps that upstream doesn't have. Preserve all `[dependencies]` entries for extracted repos (casr, ffs-search, ffs-engine, dcg-core, hashline, mempalace-core, dynamic_context_pruning, rtco-core). For everything else, accept upstream's version.

**Always run `cargo check` after resolving Cargo.toml conflicts.**

#### Category E: New Upstream Feature That Should Be Extracted

**Symptom**: Upstream added a feature that semantically belongs in one of the extracted repos (e.g., new session import format → should be in casr; new file search feature → should be in fast_file_search).

**Resolution**: **DO NOT implement the feature inline**. Instead:
1. Add a `FIXME`/`TODO` comment in the merge commit marking the gap
2. Create an issue in the corresponding external repo
3. Implement it in the external repo
4. Bump the dependency revision
5. Wire it through the adapter layer

### Step 5: Verification

```bash
# After all conflicts resolved:
git diff --cached --stat  # review staged changes

# Build must pass
cargo check 2>&1

# Run tests
cargo test 2>&1 | tail -20

# Format check
cargo fmt --all --check 2>&1

# If CI is critical, also run:
# scripts/test_fast.sh
```

### Step 6: Commit & Push

```bash
# The merge creates a commit automatically after all conflicts resolved
# Verify the merge commit message and push
git log --oneline -3
git push origin master
```

---

## Quick Reference: Common Conflict Patterns

### Pattern 1: `ResumeTarget` enum (jcode-session-types)

**Upstream** has: `CodexSession, PiSession, OpenCodeSession`
**Our fork** also has: `ForeignSession { provider_slug, session_id }`

**Resolution**: Keep ForeignSession variant. Add any new variants upstream added.

### Pattern 2: `resolve_resume_target_to_jcode` / `imported_session_id_for_target`

**Upstream** uses inline import logic. **Our fork** delegates to `casr_adapter`.

**Resolution**: Keep our version entirely (`git checkout --ours`).

### Pattern 3: Session picker match arms

**Upstream** matches known providers. **Our fork** has additional `ForeignSession` arms.

**Resolution**: Keep our version with the extra arms, add any new upstream provider arms.

### Pattern 4: `Cargo.toml` dependency changes

**Resolution**: Keep all `github.com/quangdang46/*` deps. Accept upstream dep changes for everything else.

### Pattern 5: Worktree submodule changes

If `.worktrees/` directories show up in `git status` as modified, do NOT stage them. They are managed separately.

```bash
git checkout -- .worktrees/  # restore worktree submodule pointers
```

---

## Pre-Merge Review Checklist

Before starting the merge, complete this checklist:

- [ ] `git fetch upstream` successful
- [ ] `git log master..upstream/master` reviewed for scope
- [ ] No pending local changes (working tree clean)
- [ ] Upstream changes in extracted-domain files identified (see Step 2)
- [ ] External repos' latest status checked (do they have the feature upstream is modifying?)

---

## Post-Merge Audit

After push, verify:

- [ ] `cargo check` passes
- [ ] Local builds work (`cargo build`)
- [ ] Extracted features still functional (session resume, file picker, DCG mode)
- [ ] No files from extracted repos left inline (if upstream added new inline code in extracted domains, flag it)
- [ ] Worktrees unaffected (checkout each worktree and run cargo check there too)

---

## Troubleshooting

### "Upstream added a new provider to session picker"

This is the most common conflict. Upstream adds a new session provider → our `ResumeTarget`, `import.rs`, `casr_adapter.rs`, `inline_interactive.rs`, `tui_launch.rs`, `session_picker.rs` all need new arms.

**Process**:
1. Note which new provider upstream added
2. Check if `casr` (cross_agent_session_resumer) already supports this provider
   - If YES → add the provider to `ResumeTarget` and wire it through `import.rs` using `casr_adapter::*`
   - If NO → implement support in `casr` repo first, then bump dep rev, then wire it here
3. Add match arms in all relevant files mirroring upstream's pattern but using our adapter functions

### "Both sides added code to the same function"

Manual intervention needed. Open the file and:
1. Identify which parts are ours (adapter calls, ForeignSession, etc.)
2. Identify what upstream added (new features, refactors)
3. Merge the two, keeping our extraction logic, accepting non-overlapping upstream features

### "Cargo.lock conflict"

This is normal for any dep change. Accept ours for git deps, accept theirs for crates.io deps. If unsure, resolve by:
```bash
git checkout --ours Cargo.lock
# This is usually safe since cargo update will fix stale entries
```
Then run `cargo generate-lockfile` or just `cargo check`.

---

## Repo Status Snapshot (as of last update)

- **Fork**: quangdang46/jcode
- **Upstream**: 1jehuang/jcode
- **Status**: 21 ahead, 366 behind (diverged, as of 2026-06-07)
- **Extracted repos**: 7 (casr, ffs, dcg, hashline, mempalace, dcp, rtco)
- **Adapter code**: ~2687 lines across 4+ bridge files
