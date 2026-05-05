# 2026-05-04 Sidebar Rename Blur + Terminal Refocus

## Goal

Make sidebar tab rename behave like horizontal tab-strip rename by saving on blur, while ensuring successful rename returns focus to the current tab’s terminal so the user can continue typing immediately.

## Scope

This note covers two behavior changes only:

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
7. Entering rename should select the existing tab name so replacement is immediate.

### UX

1. Rename completion should feel terminal-first: the next keystroke should go to the active terminal.
2. Sidebar and horizontal rename should have aligned commit/cancel semantics.
3. Escape remains the dedicated cancel gesture.
4. Rename should begin in replace mode, not append mode.

## Recommended Approach

### Option A — Mirror horizontal rename semantics in sidebar and refocus in workspace (Recommended)

- Add blur-save handling to sidebar rename.
- Add Escape-vs-blur cancellation protection in sidebar state.
- Keep persistence centralized in workspace through `SidebarRename`.
- Move terminal refocus responsibility to workspace rename commit handlers, because workspace owns the active terminal.
- Trigger select-all from the input’s first `Focus` event rather than from rename setup time, so the input is already the active focus target.

Why this is best:

- Preserves current boundaries: sidebar emits rename intent; workspace persists tab state.
- Keeps the focus target authoritative in one place.
- Minimizes new surface area and follows the existing horizontal rename pattern.
- Avoids brittle timing assumptions around GPUI action dispatch.

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

`SessionSidebar::begin_rename` treats `InputEvent::PressEnter` and `InputEvent::Blur` as the same commit path. That path will:

- read the input value
- trim it
- convert blank input to `None`
- emit `SidebarRename { session_id, label }`
- clear the local rename state

To preserve Escape semantics, the sidebar stores a per-editor generation token. If Escape is pressed, the sidebar clears the inline editor and records that editor generation as canceled. If a delayed blur arrives from that same editor, the blur handler becomes a no-op. If the user immediately opens a new rename editor on the same tab, the new editor gets a new generation and can still commit normally.

This mirrors the protection already used in horizontal tab rename, but remains sidebar-local because the sidebar owns that input entity and its blur lifecycle.

### 2. Workspace persistence and focus restoration

The workspace remains the persistence authority.

- `on_sidebar_rename` continues to normalize and persist `user_label`
- `commit_tab_rename` continues to normalize and persist `user_label`

Both successful commit paths explicitly focus the active tab’s focused terminal after the rename is committed. This keeps behavior consistent regardless of where rename started.

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

### 4. Focus-time select-all

Rename should enter with the old label selected. In practice, dispatching `SelectAll` during rename setup was unreliable because the new input was not always the active focus target yet.

The stable pattern is:

- create the rename input
- focus it
- listen for the input’s first `InputEvent::Focus`
- dispatch `SelectAll` once from that focus event

This keeps selection behavior aligned for both sidebar and horizontal rename flows and avoids append-mode entry.

### 5. Rename-state remapping during close/reorder

Horizontal tab rename state is index-based in the render layer, so tab close/reorder flows must remap the active rename editor and cancel marker as tab indices move.

That remap logic should:

- drop rename state if the renamed tab is closed
- shift later indices left after a close
- track tab identity across reorder by stable `summary_id`

This avoids stale rename/editor state after drag-reorder or close actions.

## Data Flow

### Sidebar rename success

1. User enters sidebar rename.
2. Input emits `Focus`, causing one-time select-all.
3. Input emits `PressEnter` or `Blur`.
4. Sidebar converts input to `Option<String>` and emits `SidebarRename`.
5. Workspace `on_sidebar_rename` updates the tab model.
6. Workspace syncs sidebar/session save.
7. Workspace focuses the active terminal.

### Sidebar rename cancel

1. User presses Escape.
2. Sidebar marks the active rename session canceled.
3. Sidebar removes local rename state.
4. Any later blur for that same session is ignored.
5. No model update, no focus restoration.

### Horizontal rename success

1. User enters tab-strip rename.
2. Input emits `Focus`, causing one-time select-all.
3. Input emits `PressEnter` or `Blur`.
4. Workspace `commit_tab_rename` updates the tab model.
5. Workspace syncs sidebar/session save.
6. Workspace focuses the active terminal.

## File Map

- Modify: `crates/con-app/src/sidebar.rs`
  - add blur-save handling to sidebar rename
  - add Escape-vs-blur cancel protection for sidebar rename input
  - normalize sidebar rename labels before emission
  - select all on the first rename input focus event
- Modify: `crates/con-app/src/workspace.rs`
  - refocus active terminal after successful horizontal tab rename
  - refocus active terminal after successful sidebar rename persistence
  - normalize horizontal rename labels before persistence
  - keep rename state stable across close/reorder
  - select all on the first rename input focus event
- Test: `crates/con-app/src/workspace.rs` test module
  - tab-slot geometry tests for browser-style reorder semantics
  - rename-state remap helper tests for close/reorder behavior
  - rename normalization tests
- Test: `crates/con-app/src/sidebar.rs` test module
  - rename normalization helper tests

## Implementation Notes

### Sidebar rename changes

- Add `rename_generation: u64` and `rename_cancelled_generation: Option<u64>` to `SessionSidebar`.
- Add `normalize_sidebar_rename_label(value: &str) -> Option<String>`.
- In `begin_rename`, use one commit path for `PressEnter` and `Blur`.
- Ignore blur commits that belong to an Escape-canceled rename.
- Track whether the input changed, so focus-to-blur without edits restores terminal focus without freezing a smart label as an explicit user label.
- On the first `InputEvent::Focus`, dispatch `SelectAll` once.

### Workspace rename changes

- Keep horizontal rename persistence in `commit_tab_rename`.
- Refocus the active terminal after successful sidebar or horizontal rename commits.
- Keep rename display aligned with `smart_tab_presentation` so updated user labels show immediately.
- Keep `tab_rename` bound to stable tab identity after tab close/reorder, and protect cancel/blur races with `tab_rename_generation` plus `tab_rename_cancelled_generation`.

### Horizontal tab reorder semantics

- Use browser-style left/right half drop slots.
- Dropping on the left half inserts before the target tab.
- Dropping on the right half inserts after the target tab.
- Support dragging later tabs forward as well as earlier tabs backward.
- Avoid triggering whole-window drag while tab drag is active on macOS.

## Error Handling

This feature has no new external failure modes.

Expected defensive behavior:

- If the target tab/session no longer exists by commit time, drop the rename gracefully.
- If blur arrives after cancel, ignore it.
- If focus restoration cannot resolve a terminal, do nothing rather than panic.
- If a reordered/closed tab invalidates transient rename indices, remap or drop that state instead of keeping stale indices.

## Testing Strategy

### Unit-level / logic coverage

In `workspace.rs` tests:

- keep normalization tests
- cover tab-slot helper behavior for left/right-half insertion semantics
- cover rename-state remap helpers for close/reorder behavior

In `sidebar.rs` tests:

- cover blur-save normalization helper

If sidebar view testing is too heavy, keep coverage focused on pure helper behavior and verify focus-side effects manually.

### Verification

- `cargo test -p con`
- `cargo build -p con`

### Manual checks

1. Horizontal tab rename enters with the old name selected.
2. Sidebar rename enters with the old name selected.
3. Sidebar rename → type new name → click elsewhere → name saves.
4. Sidebar rename → clear name → click elsewhere → custom label clears.
5. Sidebar rename → type text → press Escape → no save.
6. Horizontal rename → Enter/blur → name saves and terminal receives next keystroke.
7. Sidebar rename → Enter/blur → terminal receives next keystroke.
8. Horizontal tab drag reorder works in both directions.
9. Dragging a tab does not drag the whole window on macOS.

## Files Changed

- `crates/con-app/src/sidebar.rs`
- `crates/con-app/src/workspace.rs`

## Acceptance Criteria

1. Sidebar rename saves on blur.
2. Sidebar Escape cancels without later blur-save.
3. Horizontal rename still supports Enter/blur save and Escape cancel.
4. Rename from sidebar or horizontal strip starts with the old name selected.
5. Successful rename from sidebar or horizontal strip restores focus to the active terminal.
6. Horizontal tab drag reorder works with browser-style before/after insertion semantics.
7. Dragging tabs does not move the whole window on macOS.
8. Build and tests pass.
