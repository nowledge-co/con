# Restorable Workspaces

Issue: [#111](https://github.com/nowledge-co/con-terminal/issues/111)

## Product Goal

Con should restore the user's working shape without pretending a terminal
process can be snapshotted. A restored workspace should feel like an IDE or
browser session:

- window/tab/pane/surface layout returns predictably
- each terminal starts in the right directory
- command/input history is available immediately
- per-tab agent conversations and routing stay attached to the tab that owns
  them
- project layouts can live in git as a normal dotfile
- opening a new window does not blindly clone the last window's agent/session

The important distinction is between **intent** and **runtime**:

- **Intent** is durable: project root, named tabs, split shape, pane cwd,
  surface names, optional startup commands, agent defaults.
- **Runtime** is ephemeral: process IDs, PTY handles, terminal scrollback,
  TUI internal state, shell integration readiness.

Con should restore intent automatically and expose runtime recovery as explicit
policy, not as a misleading "resume process" promise.

## Current State

Existing persistence already covers more than the issue title suggests:

- `con-core::session::Session` persists a single window's tabs, active tab,
  agent panel state, input bar state, global shell history, global input
  history, vertical-tabs state, and per-tab conversation IDs.
- `TabState` persists tab title, cwd, split layout, focused pane id,
  per-pane command history, per-tab agent routing, and user label.
- `PaneTree::to_state` / `from_state` persist split ratios and leaf cwd.
- `GlobalHistoryState` stores command/input history outside layout so a fresh
  window can still get history.

Gaps:

- The session file is a private app-runtime JSON shape, not a project-owned
  portable layout format.
- The root session is a single-window model. It cannot represent multiple
  independent windows cleanly.
- Pane-local surfaces are not persisted. `PaneLayoutState::Leaf` only stores
  the active surface cwd.
- Pane/surface names, owner, active surface, and surface cwd are runtime-only.
- New-window and startup semantics are mixed: first launch restores
  `Session::load()`, while New Window creates a fresh session with global
  history. Multiple app instances can still contend for the same session path.
- Terminal scrollback/process state is not recoverable and should not be
  silently promised.

## First Principles

1. **A terminal session is not a document.**
   We can restore shell placement and intent, but not a running TUI process.
   If a user wants `vim`, `claude`, `htop`, or `tmux` to reopen, that must be
   an explicit startup command or an external multiplexer state.

2. **Project layout and app recovery are different products.**
   Project layout belongs to the repo and should be reviewable in git.
   App recovery belongs to the user profile and can store private, noisy state.

3. **Stable IDs are for machines; names are for humans.**
   Pane/surface IDs should be stable within a restored workspace file where
   possible. UI can show names. CLI/agents should target IDs.

4. **Relative paths make layouts portable.**
   A committed workspace file should use paths relative to its root. Absolute
   paths are allowed only in private local state.

5. **No hidden replay.**
   Commands with side effects are never replayed implicitly. Startup commands
   require explicit `run = "..."` or `profile = "..."` in the layout.

6. **One authority owns live state.**
   The GPUI workspace remains the live authority. Persistence snapshots must
   be derived from workspace state, not maintained as a second mutable model.

## Proposed State Model

Use three layers.

### 1. User Profile Runtime State

Private app data. Replaces the current single `session.json` over time.

Purpose:

- crash/restart recovery
- last open windows
- active window/tab/pane/surface
- private local cwd values
- local command/input history
- per-tab conversation IDs
- UI chrome state

Path:

- existing `con_paths::app_data_dir()`
- migrate `session.json` to `app-state.v1.json`
- keep `history.json` separate or embed under the app state with a migration

Shape:

```json
{
  "version": 1,
  "last_active_window_id": "win-main",
  "windows": [
    {
      "id": "win-main",
      "workspace_ref": {
        "kind": "project",
        "root": "/Users/me/dev/con",
        "layout_file": ".con/workspace.toml"
      },
      "runtime": {
        "bounds": null,
        "active_tab": "server",
        "agent_panel_open": true,
        "agent_panel_width": 420,
        "input_bar_visible": true,
        "vertical_tabs_pinned": true,
        "vertical_tabs_width": 240
      }
    }
  ]
}
```

### 2. Project Workspace Layout

Git-compatible, human-readable, deterministic TOML.

Default file:

```text
.con/workspace.toml
```

Why TOML:

- Con config is already TOML
- good diff quality
- friendly for hand edits
- lower ceremony than JSON for a dotfile

Example:

```toml
version = 1
name = "con"
root = "."

[defaults]
shell = "login"
agent_provider = "openai"
agent_model = "gpt-5.2"

[[tabs]]
id = "dev"
title = "Dev"
cwd = "."
active_pane = "editor"

[tabs.agent]
conversation = "project"

[tabs.layout]
split = "horizontal"
ratio = 0.58
first = "editor"

[tabs.layout.second]
split = "vertical"
ratio = 0.50
first = "tests"
second = "server"

[[tabs.panes]]
id = "editor"
cwd = "."
active_surface = "shell"

[[tabs.panes.surfaces]]
id = "shell"
title = "Shell"
cwd = "."

[[tabs.panes.surfaces]]
id = "agent"
title = "Codex"
cwd = "."
run = "codex"
restore = "manual"

[[tabs.panes]]
id = "tests"
cwd = "."

[[tabs.panes.surfaces]]
id = "shell"
title = "Tests"
cwd = "."

[[tabs.panes]]
id = "server"
cwd = "crates/con-app"

[[tabs.panes.surfaces]]
id = "server"
title = "Server"
cwd = "crates/con-app"
run = "cargo run -p con"
restore = "ask"
```

Design notes:

- `tabs.layout` references pane IDs, so layout structure stays compact and
  pane metadata is not duplicated inside the tree.
- `cwd` is relative to `root` unless absolute.
- `run` is never replayed silently unless `restore = "auto"` and the command
  is marked safe by user policy. Initial implementation should support
  `manual` and `ask`; `auto` can come later.
- `surfaces` are pane-local terminal sessions. Persist all of them, not only
  the active one.

### 3. Local Project Overlay

Private user data keyed by canonical project root plus layout-file path.

Purpose:

- active tab/pane/surface for that project
- window bounds
- command/input history for that project
- conversation IDs if user does not want them committed
- last resolved absolute paths

Do not write noisy local state into `.con/workspace.toml`.

## Restore Semantics

### Pane

Restore:

- pane ID
- split geometry
- cwd
- human title
- focused pane
- command history used by Con suggestions

Do not restore:

- process PID
- shell job table
- arbitrary terminal scrollback by default

### Surface

Restore:

- surface ID
- title
- owner
- cwd
- active surface per pane
- close-pane-when-last policy for orchestrator-owned surfaces

Do not restore:

- live process unless explicit `run` policy exists

### Agent

Restore:

- tab-owned conversation ID from private runtime state by default
- tab routing provider/model
- panel UI state

Project layout may define defaults, but should not commit private
conversation IDs unless explicitly exported.

### History

Keep two history channels:

- command suggestion history: persisted privately and optionally scoped by
  project root
- project layout commands: explicit `run` entries in `.con/workspace.toml`

Do not write every command a user typed into the git layout file.

## Startup / Multi-Window Design

### Current Problem

`Session::load()` is single-window. Starting another process or opening a
window from a cold state can duplicate the same restored shape. That feels
wrong once agent sessions, surfaces, and long histories are involved.

### Target Behavior

1. **First process launch**
   Restore the last app state once: windows, tabs, panes, active tab, agent
   panel, input bar, and conversations.

2. **Dock click after closing all windows**
   Open a fresh window with global/project history, not a duplicate stale
   crashed workspace, unless the user enabled "Restore windows when reopening".

3. **Cmd+N / New Window**
   Open a fresh window. Inherit global history and defaults only.

4. **Open Project Workspace**
   Load `.con/workspace.toml` for that project, apply local overlay, and create
   a separate window bound to that workspace.

5. **Second app process**
   Prefer single-instance forwarding to the running app. If unsupported on a
   platform, use a per-process session lock so only one process writes the
   runtime state. A second process can open a fresh window but must not overwrite
   the first process's window state on exit.

## Compatibility With Existing Control Plane

No existing `panes.*` behavior should change.

Additive API surface:

- `workspaces.get`
- `workspaces.save`
- `workspaces.open`
- `workspaces.export`
- `workspaces.import`
- `workspaces.list`

Existing `tree.get` can become the base snapshot for export. The export layer
should convert live `PaneTree` into project layout TOML with stable pane/surface
IDs and relative paths.

Agents and external orchestrators should continue to prefer:

- `pane_id` for visible split panes
- `surface_id` for pane-local terminal sessions

## Implementation Plan

### Phase 1 — Lossless Internal Snapshot

Goal: make current private restore complete before adding a public file format.

- Extend `PaneLayoutState::Leaf` to store surfaces:
  - `surfaces: Vec<SurfaceState>`
  - `active_surface_id: Option<usize>`
  - `pane_title/user label if needed`
- Add `SurfaceState`:
  - `surface_id`
  - `title`
  - `owner`
  - `cwd`
  - `close_pane_when_last`
- Update `PaneTree::to_state` and `PaneTree::from_state` to round-trip all
  surfaces.
- Preserve backward compatibility with old leaves that only have `cwd`.
- Add tests for:
  - one pane with multiple surfaces round-trips
  - active surface survives restore
  - split ratios and focused pane survive restore

### Phase 2 — App-State v1

Goal: fix the single-window session model.

- Introduce `AppState` with `windows: Vec<WindowState>`.
- Migrate old `Session` into one `WindowState`.
- Save each window independently with a window UUID.
- Only restore saved windows on true app startup, not on New Window.
- Keep `GlobalHistoryState` independent.

### Phase 3 — Project Workspace TOML

Goal: make `.con/workspace.toml` useful and git-friendly.

- Add `WorkspaceLayout` in `con-core`, separate from `Session`.
- Implement import/export between `WorkspaceLayout` and live `Session`/`PaneTree`.
- Add command palette actions:
  - Save Workspace Layout
  - Open Workspace Layout
  - Export Current Layout
  - Reload Workspace Layout
- Add `con-cli workspaces export/import/open`.
- Start with manual file operations; later add UI.

### Phase 4 — Restore Policy

Goal: make startup commands safe and intentional.

- Introduce `restore = "manual" | "ask" | "auto"`.
- Default to `manual`.
- On workspace open, show a compact restore sheet listing commands to run.
- Never auto-run commands from a git file without user approval unless a
  user-local trust record exists for that file hash/root.

### Phase 5 — UX Polish

Goal: make the feature discoverable without becoming IDE-heavy.

- Surface workspace name in the title/tab sidebar.
- In settings, add "Restore windows on launch" and "Trust workspace startup
  commands".
- In command palette, include workspace actions and show their shortcuts.
- Right-click tab/sidebar menu can include "Save Layout to Project".

## Migration Rules

- Old `session.json` remains readable.
- Missing `layout` falls back to one pane using `tab.cwd`.
- Missing surfaces means create one surface from leaf cwd.
- Missing local overlay means use `.con/workspace.toml` defaults.
- Corrupt project workspace file should not block app startup; show a recoverable
  error and open a fresh shell.

## Non-Goals For First Implementation

- No full PTY/process snapshot.
- No hidden command replay.
- No terminal scrollback persistence by default.
- No shell-history file rewriting.
- No cross-machine agent conversation sync.

## Open Questions

- Should project workspace files live at `.con/workspace.toml` or
  `.config/con/workspace.toml` inside the repo? Recommendation:
  `.con/workspace.toml`, because the directory can later contain local README,
  scripts, or templates while app-private overlays stay outside the repo.
- Should committed layouts include agent model defaults? Recommendation:
  allow it, but never credentials; local user settings remain the source of
  provider auth.
- Should scrollback be exportable manually for debugging? Recommendation:
  separate "Export Terminal Transcript" action, not workspace restore.

