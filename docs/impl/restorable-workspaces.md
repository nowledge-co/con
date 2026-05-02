# Restorable Workspaces

Issue: [#111](https://github.com/nowledge-co/con-terminal/issues/111)

## Product Position

Restorable workspaces are a continuity feature, not a process snapshot feature.
Con should bring the user back to the same working shape: windows, tabs, panes,
pane-local surfaces, cwd, history, focus, and agent context. It must not pretend
that a shell process, a TUI process, or arbitrary terminal scrollback can be
faithfully resumed.

The production design has one priority order:

1. **Local continuity first.** Restore the user's own windows and project memory
   from private app data.
2. **Project memory second.** Opening a repo should feel like returning to a
   familiar workspace without writing anything to the repo.
3. **Exported layout DSL third.** A versioned schema is useful as Con's own
   export/import format. It should be generated from a user-tuned workspace
   first, not treated as a hand-written boot script.

This ordering mirrors browser and IDE tacit behavior: New Window is fresh,
Open Folder can restore local project memory, and app launch can continue where
the user left off. Exported files describe layout intent; private runtime state
stays private.

## Current Production Slice

Implemented in the first issue #111 PR:

- private session restore now round-trips every pane-local surface in a pane
- each surface stores id, title, owner, cwd, and close-pane-when-last policy
- each pane stores its active surface id
- old session files that only stored a leaf cwd still load correctly
- New Window uses a fresh session seeded with global history, not a clone of
  the restored layout
- a second process that detects a live control endpoint opens a fresh
  history-backed session instead of restoring the same saved layout again

This slice is production-safe because it improves private restore fidelity and
introduces only a layout-only schema. It does not introduce command replay or a
repo trust model.

## First Principles

1. **A terminal session is not a document.**
   Con can restore placement and intent. It cannot honestly restore arbitrary
   process state. True process continuity belongs to tools such as tmux,
   zellij, shells, or the underlying application.

2. **Private state and shared intent are different products.**
   Conversations, command history, trust decisions, window geometry, scrollback,
   active focus, and credentials are private. Team-shared files must be
   reviewable, deterministic, and safe to clone.

3. **New Window means fresh.**
   Cmd+N, Dock reopen after all windows are closed, and summon-from-hotkey
   should create a clean shell unless the user explicitly opens a project or
   chooses to restore.

4. **Open Project can remember.**
   If the user opens the same folder again, Con may restore local project
   memory keyed by the canonical project root. This is the IDE-shaped behavior
   users already understand.

5. **No hidden replay.**
   Con must never run commands from a restored workspace without explicit user
   action. Future task files are picker inputs, not boot scripts. The layout
   schema deliberately has no `run` field.

6. **Stable IDs are for APIs; names are for humans.**
   `pane_id` and `surface_id` are the control-plane targets. The UI should show
   stable names such as "Server", "Tests", "Codex", or "Surface 2".

7. **Restore should optimize for flow, not exact pixels.**
   Rows, columns, pane ratios, cwd, active tab, and named surfaces matter.
   Pixel-perfect historical window bounds are secondary and platform-specific.

8. **Screen text history is a first-class continuity feature.**
   Users expect meaningful scrollback to survive restart, especially if they
   come from iTerm2. libghostty does not provide this as a product feature, so
   Con needs an app-owned scrollback/transcript snapshot later. It must be
   private by default and separate from exported layouts.

## User-Facing Flows

### Flow 1: Quit Tonight, Continue Tomorrow

The user has one Con window with three tabs:

- `Dev`: editor shell, server pane, tests pane
- `Agents`: two pane-local surfaces, `Planner` and `Worker`
- `Release`: one shell in `~/release`

When Con launches cold tomorrow:

- the same windows/tabs/panes/surfaces return
- each terminal opens at its remembered cwd
- the active tab/pane/surface is restored
- per-tab agent conversation resumes from private state
- commands are not automatically rerun
- global and project history are available for suggestions

Example UI copy for restored panes:

```text
Restored workspace from May 2, 22:41. Processes were not resumed.
```

The line should be quiet and optional. It exists to set correct expectations,
not to add noise to every shell forever.

### Flow 2: Cmd+N Scratch Window

The user presses Cmd+N while a large project workspace is open.

Expected result:

- one clean shell
- default cwd from config or launch context
- shared command/input history
- no copied agent conversation
- no copied tabs, panes, or surfaces

This is already the behavior direction of `fresh_window_session_with_history()`.

### Flow 3: Second Process Invocation

The user runs `con` while Con is already open.

Production target:

- the new process sends a control-plane request to the running app
- the running app opens the requested window/project
- the new process exits
- only the running app writes runtime state

Current mitigation:

- the second process detects the live endpoint and opens a fresh-history
  session instead of cloning the last restored session

The mitigation is safe, but not final. Single-instance forwarding is the
production target.

### Flow 4: Open Folder

The user runs:

```sh
con ~/dev/con
```

Production target:

- Con canonicalizes `~/dev/con`
- if local project memory exists, it restores that folder's last Con shape
- otherwise, it opens one fresh shell rooted at `~/dev/con`
- project memory remains in app data, not in the repo

Suggested private storage:

```text
<app-data>/
  launch.json
  windows/<window-uuid>.json
  projects/<hash-of-canonical-root>.json
  history.json
```

### Flow 5: Agent Orchestrator Surfaces

An external orchestrator creates a pane with multiple surfaces:

- surface `planner`: cwd `~/dev/app`
- surface `worker-1`: cwd `~/dev/app/crates/server`
- surface `worker-2`: cwd `~/dev/app/crates/ui`

After restart:

- all three surfaces still exist in the same pane
- titles and owners survive
- inactive surfaces are sized to their pane before focus
- the orchestrator can continue targeting stable surface IDs where possible
- humans can distinguish the surfaces by names

This is the most important shipped value of the current PR.

### Flow 6: tmux / SSH Continuity

If the user wants true process continuity, Con should make tmux or an explicit
SSH attach command easy, not fake process restore.

Recommended user pattern:

```sh
tmux attach -t con || tmux new -s con
```

Con's job is to restore the terminal shape around that command. Running the
command remains a user action unless a future trusted task system explicitly
supports it.

## State Model

### 1. Window Runtime State

Private app data, one file per window.

Purpose:

- tabs
- split layout
- focused pane
- active tab
- pane-local surfaces
- agent panel state
- input bar state
- vertical tabs state

Sketch:

```json
{
  "version": 1,
  "id": "win_01j...",
  "project_root": "/Users/me/dev/con",
  "active_tab": 0,
  "tabs": [],
  "chrome": {
    "agent_panel_open": true,
    "input_bar_visible": true,
    "vertical_tabs_visible": false
  }
}
```

### 2. Launch Manifest

Private app data that records which windows were open at last app quit.

Sketch:

```json
{
  "version": 1,
  "restore_windows_on_launch": true,
  "last_active_window_id": "win_01j...",
  "windows": ["win_01j...", "win_01k..."]
}
```

### 3. Project Memory

Private app data keyed by canonical root hash.

Purpose:

- last local workspace shape for a project
- project-scoped command/input history
- per-tab conversation IDs
- last active tab/pane/surface for that project

Sketch:

```json
{
  "version": 1,
  "root": "/Users/me/dev/con",
  "last_window_id": "win_01j...",
  "tabs": [],
  "history": {
    "commands": [],
    "inputs": []
  }
}
```

### 4. Exported Layout DSL

Implemented as `con-core::workspace_layout` for validation and future
import/export wiring.

Purpose:

- deterministic output from "Export Current Layout"
- stable schema for import/export tests
- reviewable layout intent if a user chooses to commit it
- future bridge for orchestrators that want to generate Con layouts

Non-purpose:

- not a shell history file
- not a process restore file
- not a credential store
- not a hand-written startup script

Default path for future export:

```text
.con/workspace.toml
```

Current schema constraints:

- `format = "con.workspace.layout"`
- `version = 1`
- tabs, panes, surfaces, split geometry, cwd, and optional agent defaults
- no `run`
- no `restore`
- no conversations
- no command history
- no scrollback
- no trust decisions

Future task files should be separate:

- `.con/tasks.toml`: explicit named commands users pick from a menu

Do not combine layout and command replay. It creates a trust model before the
product needs one.

### 5. Screen Text History

Terminal scrollback in Con is currently runtime-only. That is a meaningful gap:
screen text history is a practical continuity feature, and iTerm2 users expect
it.

Production direction:

- maintain an app-owned bounded transcript per pane/surface
- snapshot the visible scrollback privately with the window/project state
- restore it as text history before the new shell prompt is ready
- never write transcript history to exported workspace layouts
- cap disk and memory use aggressively
- avoid recording alternate-screen TUIs by default unless the user explicitly
  enables it

This should be implemented separately from layout restore. Layout tells Con
where terminals belong. Transcript history tells the user what happened there.

## Startup Semantics

| Gesture | Production Behavior |
| --- | --- |
| Cold app launch | Restore windows from `launch.json` if enabled. |
| Cmd+N | Fresh scratch window with shared history only. |
| Open Folder / `con <path>` | Restore private project memory if present; otherwise fresh root shell. |
| Dock click after all windows closed | Reopen one clean/default window, or last window if the user chose that setting. |
| Second process invocation | Forward to running app through the control socket, then exit. |
| Crash recovery | Restore last private shape, but never pretend processes resumed. |

## Control Plane Direction

Existing pane and surface APIs remain stable.

Near-term additive APIs:

- `windows.open`
- `windows.list`
- `windows.focus`
- `workspaces.open_project`
- `workspaces.current`
- `workspaces.save_local`
- `workspaces.clear_local`

Deferred APIs:

- `workspaces.export_layout`
- `workspaces.import_layout`
- `workspaces.validate_layout`

The schema exists now, but these APIs should wait until the product has a real
import/export UI and manual review flow.

## Implementation Roadmap

### Phase 1: Lossless Surface Restore

Status: implemented.

- Extend private `PaneLayoutState::Leaf` with `surfaces`.
- Persist every pane-local surface, not only the active cwd.
- Restore active surface, owner, title, cwd, and close policy.
- Keep old single-surface session files readable.

### Phase 2: AppState v1

Status: next production milestone.

- Split the current single `Session` into window runtime files.
- Add a launch manifest that lists windows open at last app quit.
- Migrate old `session.json` into one window state on first launch.
- Keep global history independent.
- Add "Restore windows on launch" in Settings.
- Ensure Cmd+N always creates a scratch window.
- Treat Dock reopen after all windows close as a fresh-window gesture unless
  the user explicitly chose last-window restore.

### Phase 3: Single-Instance Forwarding

Status: required before this feature is complete.

- When a second process starts, connect to the live control endpoint.
- Send a `windows.open` or `workspaces.open_project` request.
- Exit the second process.
- Add a write lock or generation stamp so stale processes cannot overwrite
  runtime state.

### Phase 4: Project Memory

Status: after AppState.

- Detect project roots from explicit paths and cwd.
- Store private memory under `projects/<hash(root)>.json`.
- Restore local project shape when opening that folder again.
- Add "Forget Local Workspace State" in Command Palette and Settings.

### Phase 5: Shared Tasks and Layouts

Status: schema foundation exists; UI and task files are deferred.

- Wire "Export Current Layout" only after AppState and project memory are solid.
- Export from the live workspace instead of asking users to hand-write the file.
- Start with `.con/tasks.toml` for named commands.
- Keep `.con/workspace.toml` layout-only.
- Never store secrets, conversations, command history, scrollback, active focus,
  or trust decisions in repo files.

### Phase 6: Screen Text History

Status: required for iTerm2-grade continuity, not part of this PR.

- Add bounded transcript capture per pane/surface.
- Persist private scrollback snapshots with AppState/project memory.
- Restore transcript text separately from the live PTY.
- Provide a setting for transcript retention size.
- Exclude alternate-screen content by default to avoid restoring stale TUI
  frames.

## Non-Goals

- No full PTY/process snapshot.
- No hidden command replay.
- No terminal scrollback persistence in exported layouts.
- No shell-history file rewriting.
- No cross-machine conversation sync.
- No repo-stored credentials, tokens, or private histories.
- No command-running workspace files in the first production slice.

## Review Checklist

Before a restorable-workspace PR is merge-ready, verify:

- Cmd+N opens a fresh scratch window.
- Reopening Con after quit restores the previous private shape.
- Pane-local surfaces survive restore with correct title/cwd/owner.
- No project file is written unless the user explicitly requests export.
- No command runs because of restore.
- Exported layout TOML contains no commands, conversations, history, or
  scrollback.
- A second process does not clone the restored layout and agent conversation.
- Old session files still load.
- Windows, Linux, and macOS keep the same semantics even if their storage paths
  differ.
