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
| 1 | `casr` | `quangdang46/cross_agent_session_resumer` | `crates/jcode-base/src/casr_adapter.rs`, `crates/jcode-base/src/import.rs` | Session import/resume (replaces `jcode-import-core`) |
| 2 | `ffs-search`, `ffs-engine`, `ffs-symbol` | `quangdang46/fast_file_search` | `crates/jcode-tui/src/tui/app/at_picker.rs` (uses `ffs_engine::mention` and `ffs_search::mention`) | File search, @-mention autocomplete, symbols |
| 3 | `dcg-core` | `quangdang46/destructive_command_guard` | `crates/jcode-app-core/src/dcg_bridge.rs` | Permission guard, YOLO classifier |
| 4 | `hashline` | `quangdang46/hashline` | `crates/jcode-app-core/src/tool/hashline_edit.rs` | SHA-256 anchored hashing |
| 5 | `mempalace-core` | `quangdang46/mempalace_rust` | `crates/jcode-mempalace-adapter/` (entire crate) | Memory palace |
| 6 | `dynamic_context_pruning` | `quangdang46/dynamic_context_pruning` | `crates/jcode-app-core/src/dcp_bridge.rs`, `dcp_plugin.rs` | Context pruning |
| 7 | `rtco-core` | `quangdang46/rust_token_cost_optimizer` | `crates/jcode-app-core/src/rtco_filter.rs` | Token cost optimization |

### Additional git dependencies

| Dep | Owner | Domain |
|---|---|---|
| `agentgrep` | `1jehuang/agentgrep` | Code search (not extracted, from upstream contributor) |
| `beads_rust` | `quangdang46/beads_rust` | Issue/bead tracking integration (preserve on `Cargo.toml` conflicts) |

### Not-yet-extracted domains that commonly conflict

- `crates/jcode-tui/src/tui/app/inline_interactive.rs` — session picker, resume logic (uses `casr` through `import.rs`)
- `crates/jcode-session-types/src/lib.rs` — `ResumeTarget` enum (has `ForeignSession` variant added locally)
- `src/cli/tui_launch.rs` — terminal launch (uses `casr_adapter` heavily)
- `crates/jcode-tui/src/tui/session_picker/` — session picker UI (has `ForeignSession` arms added locally)
- `crates/jcode-app-core/src/yolo_classifier.rs` — DCG integration
- `crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs` — slash commands (`/permissions`, `/models`), `$<skill>`, `@<file>` autocomplete, FFS tool rename
- `crates/jcode-tui/src/tui/app/state_ui.rs` — `/skills` report, `/permissions` handler
- `crates/jcode-tui/src/tui/ui_overlays.rs` — help overlay entries
- `crates/jcode-base/src/safety.rs` — AUTO_ALLOWED list with `ffs *` entries
- `crates/jcode-base/src/skill.rs` — `parse_invocation` with `$<name>` instead of `/<name>`
- `Cargo.toml` — `mempalace-backend` feature, `jcode-app-core` dep
- `crates/jcode-app-core/src/tool/mod.rs` — tool registration names + module declarations
- `crates/jcode-app-core/src/dcg_bridge.rs` — `READ_ONLY_ACTIONS` with FFS tools, `mode_to_str`
- `crates/jcode-tui/src/tui/app/at_picker.rs` — `@` mention picker
- `crates/jcode-base/src/prompt.rs` — system prompt with `$skillname`

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

# *** CRITICAL ***: Check if upstream has changed ANY file we've modified locally.
# Auto-merge can silently overwrite our changes if there's no textual conflict.
# This includes extracted-domain files AND all local customizations.
# Use git log to find all files we've modified on master since fork:
LOCAL_FILES=$(git log --all --since="2026-01-01" --diff-filter=M --format="" --name-only -- \
  '*.rs' '*.toml' '*.json' '*.sh' '*.md' \
  | sort -u | grep -v 'target/' | grep -v '.worktrees/' | head -200)
UPSTREAM_FILES=$(git diff --name-only master..upstream/master)
# Files that changed both locally AND upstream — these need attention
COMMON_FILES=$(comm -12 <(echo "$UPSTREAM_FILES" | sort) <(echo "$LOCAL_FILES" | sort))
if [ -n "$COMMON_FILES" ]; then
  echo "=== Files changed by upstream that we've also modified locally ==="
  echo "$COMMON_FILES"
  echo "=== These risk silent overwrite. Review each after merge. ==="
fi
# Also check the known extracted-domain files explicitly
git diff --stat master..upstream/master -- $UPSTREAM_FILES
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

**Resolution**: **CAREFUL MERGE**. Our `Cargo.toml` has git deps that upstream doesn't have. Preserve all `[dependencies]` entries for extracted repos (casr, ffs-search, ffs-engine, ffs-symbol, dcg-core, hashline, mempalace-core, dynamic_context_pruning, rtco-core, beads_rust). For everything else, accept upstream's version.

**Always run `cargo check` after resolving Cargo.toml conflicts.**

#### Category E: New Upstream Feature That Should Be Extracted

**Symptom**: Upstream added a feature that semantically belongs in one of the extracted repos (e.g., new session import format → should be in casr; new file search feature → should be in fast_file_search).

**Resolution**: **DO NOT implement the feature inline**. Instead:
1. Add a `FIXME`/`TODO` comment in the merge commit marking the gap
2. Create an issue in the corresponding external repo
3. Implement it in the external repo
4. Bump the dependency revision
5. Wire it through the adapter layer

#### Category F: Silent Overwrite Risk

**Symptom**: Upstream changed a file that we've modified locally, and the merge auto-resolved WITHOUT conflict. No conflict markers, but our local changes are gone.

**Why this happens**: Git's auto-merge only produces conflict markers when both sides modified the same region. If upstream modified lines far from our local changes, auto-merge silently accepts both — but on the next `cargo check` our changes may no longer apply correctly, or runtime behavior regresses.

**Resolution**: After merge, BEFORE verification:

```bash
# List files that upstream changed that we also modified
# These are the highest risk for silent overwrite
git diff --name-only HEAD..origin/master -- $(comm -12 \
  <(git diff --name-only master..upstream/master | sort) \
  <(git log --all --since="2026-01-01" --diff-filter=M --format="" --name-only -- '*.rs' '*.toml' | sort -u)
)

# Check each one. If a file has unexpected differences (our local
# additions missing), restore from origin/master and re-apply:
git show origin/master:<file> > <file>  # get our version
# Then manually merge in any upstream additions that don't conflict
```

**Common files with silent overwrite risk** (checked 2026-06):
- `crates/jcode-tui/src/tui/app/state_ui_input_helpers.rs` — slash commands, FFS rename, `$`/`@` autocomplete
- `crates/jcode-tui/src/tui/app/state_ui.rs` — `/permissions`, `/skills` report
- `crates/jcode-tui/src/tui/ui_overlays.rs` — help entries
- `crates/jcode-base/src/safety.rs` — AUTO_ALLOWED list (FFS tools)
- `crates/jcode-base/src/config.rs` — tool-profile allow lists
- `crates/jcode-base/src/prompt.rs` — system prompt with `$skillname`
- `crates/jcode-base/src/skill.rs` — `parse_invocation` using `$<name>`
- `Cargo.toml` — mempalace-backend feature, jcode-app-core dep
- `crates/jcode-app-core/src/dcg_bridge.rs` — READ_ONLY_ACTIONS, mode helpers
- `crates/jcode-app-core/src/tool/mod.rs` — tool registrations, module declarations
- `crates/jcode-tui/src/tui/app/at_picker.rs` — `@` mention picker
- `crates/jcode-tui/src/tui/app/input.rs` — lazy-init @ picker
- `crates/jcode-tui/src/tui/ui_tools.rs` — tool summary display arms
- `crates/jcode-tui/src/tui/app/tui_lifecycle.rs` — App constructor field
- `crates/jcode-desktop/src/single_session.rs` — tool name match arms
- `crates/jcode-provider-core/src/anthropic.rs` — tool name mapping
- `crates/jcode-base/src/provider/claude.rs` — tool name mapping
- `crates/jcode-usage-types/src/lib.rs` — telemetry category arms
- `crates/jcode-tui-tool-display/src/lib.rs` — resolve_display_tool_name
- `crates/jcode-tool-types/src/lib.rs` — resolve_tool_name

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
- [ ] Upstream changes in locally-modified files identified (see Step 2 — dynamic file list)
- [ ] External repos' latest status checked (do they have the feature upstream is modifying?)

---

## Post-Merge Audit

After push, verify:

- [ ] `cargo check` passes
- [ ] Local builds work (`cargo build`)
- [ ] Category F check done: all locally-modified files that upstream touched were audited for silent overwrite
- [ ] `ffs`/`$`/`@`/`/permissions` features still working
- [ ] `Cargo.toml` has our feature flags (`mempalace-backend`, `dcp`, `rtco`)
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
- **Status**: diverged (regenerate counts with `git rev-list --left-right --count master...upstream/master`)
- **Extracted repos**: 7 (casr, ffs, dcg, hashline, mempalace, dcp, rtco)
- **Adapter code**: ~2687 lines across 4+ bridge files
