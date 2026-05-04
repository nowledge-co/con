# 2026-05-04 Sidebar Rename Blur + Terminal Refocus Design

## Goal

Make sidebar tab rename behave like horizontal tab-strip rename by saving on blur, while ensuring successful rename returns focus to the current tab’s terminal so the user can continue typing immediately.

## Scope

This design covers two behavior changes only:

1. Sidebar rename saves on blur, matching Enter behavior.
2. Successful rename returns focus to the current tab’s focused terminal.

Out of scope:

- Changing rename visuals
- Changing drag/reorder behavior
- Changing horizontal-tab rename semantics beyond terminal refocus
- Introducing different focus targets for different rename entry points

## Current State

The codebase already supports:

- Horizontal tab-strip inline rename in `crates/con-app/src/workspace.rs`
- Sidebar inline rename in `crates/con-app/src/sidebar.rs`
- Shared workspace-side persistence via `SidebarRename` events and `on_sidebar_rename`

The horizontal tab-strip rename already has:

- Enter-to-save
- Blur-to-save
- Escape-to-cancel protection via `tab_rename_cancelled_index`
- Shared normalization through `normalize_tab_user_label`

The sidebar rename currently has:

- Enter-to-save
- Escape-to-cancel
- No blur-save path
- No explicit protection against Escape followed by blur

## Requirements

### Functional

1. Starting sidebar rename should keep existing behavior for opening the inline input.
2. Pressing Enter in sidebar rename should save the label exactly as today.
3. Blurring sidebar rename should save using the same normalization and emitted event shape as Enter.
4. Pressing Escape in sidebar rename should cancel and must not later save due to blur.
5. Successful rename from either source:
   - horizontal tab strip
   - sidebar
   must return focus to the active tab’s focused terminal.
6. Empty or whitespace-only names must continue clearing the custom label.

### UX

1. Rename completion should feel terminal-first: the next keystroke should go to the active terminal.
2. Sidebar and horizontal rename should have aligned commit/cancel semantics.
3. Escape remains the dedicated cancel gesture.

## Recommended Approach

### Option A — Mirror horizontal rename semantics in sidebar and refocus in workspace (Recommended)

- Add blur-save handling to sidebar rename.
- Add Escape-vs-blur cancellation protection in sidebar state.
- Keep persistence centralized in workspace through `SidebarRename`.
- Move terminal refocus responsibility to workspace rename commit handlers, because workspace owns the active terminal.

Why this is best:

- Preserves current boundaries: sidebar emits rename intent; workspace persists tab state.
- Keeps the focus target authoritative in one place.
- Minimizes new surface area and follows the existing horizontal rename pattern.

### Option B — Make sidebar own more rename lifecycle and request refocus externally

- Sidebar would save on blur and then ask the workspace to refocus terminal through a new event or callback.

Why not recommended:

- Adds cross-component coordination for a behavior the workspace already controls.
- Makes ownership less clear.

### Option C — Generalize all rename flows into a shared abstraction first

- Extract common rename controller/shared helper for sidebar and tab strip.

Why not recommended now:

- More refactor than the feature requires.
- Higher risk for a small UX fix.

## Design

### 1. Sidebar rename lifecycle

`SessionSidebar::begin_rename` will treat `InputEvent::PressEnter` and `InputEvent::Blur` as the same commit path. That path will:

- read the input value
- trim it
- convert blank input to `None`
- emit `SidebarRename { session_id, label }`
- clear the local rename state

To preserve Escape semantics, the sidebar will store a small cancel marker for the active rename session. If Escape is pressed, the sidebar clears the inline editor and records that the session was canceled. If a subsequent blur arrives for the same rename session, that blur handler becomes a no-op.

This mirrors the protection already used in horizontal tab rename, but remains sidebar-local because the sidebar owns that input entity and its blur lifecycle.

### 2. Workspace persistence and focus restoration

The workspace remains the persistence authority.

- `on_sidebar_rename` continues to normalize and persist `user_label`
- `commit_tab_rename` continues to normalize and persist `user_label`

Both successful commit paths will explicitly focus the active tab’s focused terminal after the rename is committed. This keeps behavior consistent regardless of where rename started.

The focus target is always:

- `self.tabs[self.active_tab].pane_tree.focused_terminal()`

The refocus should happen only after a successful commit, not on cancel.

### 3. Normalization

No new normalization rules are introduced.

Both rename entry points continue using:

- trim surrounding whitespace
- blank => clear custom label (`None`)
- non-blank => save trimmed string

`normalize_tab_user_label` in `workspace.rs` remains the semantic source of truth for workspace-side persistence. Sidebar may continue local trimming before event emission, but the workspace remains tolerant and authoritative.

## Data Flow

### Sidebar rename success

1. User enters sidebar rename.
2. Input emits `PressEnter` or `Blur`.
3. Sidebar converts input to `Option<String>` and emits `SidebarRename`.
4. Workspace `on_sidebar_rename` updates the tab model.
5. Workspace syncs sidebar/session save.
6. Workspace focuses the active terminal.

### Sidebar rename cancel

1. User presses Escape.
2. Sidebar marks the active rename session canceled.
3. Sidebar removes local rename state.
4. Any later blur for that same session is ignored.
5. No model update, no focus restoration.

### Horizontal rename success

1. User enters tab-strip rename.
2. Input emits `PressEnter` or `Blur`.
3. Workspace `commit_tab_rename` updates the tab model.
4. Workspace syncs sidebar/session save.
5. Workspace focuses the active terminal.

## Error Handling

This feature has no new external failure modes.

Expected defensive behavior:

- If the target tab/session no longer exists by commit time, drop the rename gracefully.
- If blur arrives after cancel, ignore it.
- If focus restoration cannot resolve a terminal, do nothing rather than panic.

## Testing Strategy

### Unit-level / logic coverage

In `workspace.rs` tests:

- keep existing normalization tests
- add a small pure helper test if terminal-refocus gating or cancel-remap logic needs isolation

In `sidebar.rs` tests, if lightweight testing is practical:

- cover blur-save normalization helper
- cover cancel-marker behavior preventing blur save after Escape

If sidebar view testing is too heavy, extract a small pure helper for commit/cancel state transitions and test that helper instead.

### Verification

- `cargo test -p con workspace::tests`
- targeted sidebar tests if added
- `cargo build -p con`

### Manual checks

1. Sidebar rename → type new name → click elsewhere → name saves.
2. Sidebar rename → clear name → click elsewhere → custom label clears.
3. Sidebar rename → type text → press Escape → no save.
4. Horizontal rename → Enter/blur → name saves and terminal receives next keystroke.
5. Sidebar rename → Enter/blur → terminal receives next keystroke.

## Files Expected to Change

- `crates/con-app/src/sidebar.rs`
  - sidebar rename blur-save semantics
  - Escape-vs-blur cancel protection
- `crates/con-app/src/workspace.rs`
  - terminal refocus after successful rename commits
  - possible small helper for active-terminal refocus reuse

## Acceptance Criteria

1. Sidebar rename saves on blur.
2. Sidebar Escape cancels without later blur-save.
3. Horizontal rename still supports Enter/blur save and Escape cancel.
4. Successful rename from sidebar or horizontal strip restores focus to the active terminal.
5. Build and targeted tests pass.
