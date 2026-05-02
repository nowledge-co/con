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
- `PaneTree::to_state` / `from_state` persist split ratios, leaf cwd, and
  every pane-local surface in each leaf.
- `GlobalHistoryState` stores command/input history outside layout so a fresh
  window can still get history.
- `con-core::workspace_layout` defines a validated `.con/workspace.toml`
  schema for git-compatible project layout files.

Gaps:

- The session file is a private app-runtime JSON shape, not a project-owned
  portable layout format.
- The root session is a single-window model. It cannot represent multiple
  independent windows cleanly.
- Project workspace files are typed and validated, but not yet wired to
  Command Palette, Settings, or `con-cli` import/export flows.
- New-window and startup semantics are partially separated: first launch
  restores `Session::load()`, New Window creates a fresh session with global
  history, and a second process that sees an already-live control endpoint now
  opens a fresh-history session instead of cloning the same saved
  layout/agent conversation.
- Multiple app instances can still contend for runtime-state writes until the
  AppState window model and single-instance forwarding are implemented.
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

7. **Restore should optimize for "back to flow", not "exact pixels".**
   A user cares that the repo, panes, roles, cwd, and agent context are back.
   Exact process state is only valuable when it is truthful. False precision is
   worse than a clean restore prompt.

8. **A committed layout is a contract for a team.**
   Anything in `.con/workspace.toml` should be understandable in code review,
   stable across machines, and safe to clone. Private state must stay in local
   overlays.

9. **A new window is a new workspace, not a clone.**
   Browser/IDE muscle memory says Cmd+N starts fresh. Restore is for app launch
   and explicit project open, not every new surface.

## Product Use Cases

### Daily Continuity

The user quits Con at night and reopens it in the morning.

Required behavior:

- restore last windows only once on true app startup
- restore tabs, panes, surfaces, cwd, active focus, input bar, agent panel, and
  per-tab agent routing/conversation
- do not rerun commands unless explicit restore policy asks/approves
- global and project-scoped history should be available immediately

Design implication:

- private `AppState` owns this; project layout is only an input.

### Crash Recovery

The app crashes or the OS restarts.

Required behavior:

- recover the last known UI shape
- clearly label any commands that did not restart
- never silently replay dangerous commands
- preserve enough history for suggestions and agent continuity

Design implication:

- private runtime snapshots should be frequent and cheap, but restore actions
  must remain user-controlled.

### Scratch Window

The user presses Cmd+N or summons Con from anywhere.

Required behavior:

- open one clean shell with shared history/defaults
- do not clone the last project layout, agent conversation, or surfaces

Design implication:

- New Window uses `fresh_window_session_with_history()`, not app restore state.

### Project Dotfile

A repo contains `.con/workspace.toml`.

Required behavior:

- opening the repo creates the expected tabs/panes/surfaces with relative cwd
- team members can review the layout like any other dotfile
- local active focus, window size, and history remain private
- missing paths degrade gracefully

Design implication:

- `.con/workspace.toml` stores intent; local overlay stores user/runtime state.

### Team Onboarding

A new contributor clones a repo and opens its Con workspace.

Required behavior:

- understand the workspace before it runs anything
- see "Install", "Server", "Tests", "Agent" roles immediately
- choose which startup commands to run
- avoid leaking maintainer-specific absolute paths or conversations

Design implication:

- default `restore = "manual"`; `restore = "ask"` opens a restore sheet.

### Agent Orchestrator / Subagents

An external orchestrator creates pane-local surfaces for worker agents.

Required behavior:

- owned surfaces survive app restart as named surfaces with correct cwd
- human can see which surface belongs to what role
- `surface_id` remains stable within restored local state where possible
- orchestrator-owned ephemeral panes can still close when their last surface
  closes

Design implication:

- private session restore must be lossless for surfaces before project import
  matters.

### Remote / SSH Workspace

The user works inside SSH panes.

Required behavior:

- restore local shell panes at the correct local cwd
- represent remote intent explicitly if Con started the SSH command
- do not pretend an arbitrary SSH session can be resumed
- support startup command such as `ssh prod` only with user approval

Design implication:

- remote continuity is runtime/private evidence; project files may define an
  approved startup command but should not store live SSH state.

### tmux Workspace

The user relies on tmux for live process/session recovery.

Required behavior:

- Con can restore panes that attach to named tmux sessions/windows
- Con should not duplicate tmux's responsibilities
- startup command examples like `tmux attach -t project || tmux new -s project`
  must be explicit and reviewable

Design implication:

- tmux is the recommended path for true process continuity; Con restores the
  terminal shell shape around it.

### Multi-Root / Monorepo

The user wants one workspace for a monorepo or several related repos.

Required behavior:

- support per-pane relative cwd
- allow root-relative and absolute paths
- avoid assuming every pane belongs to one Rust/Node/etc. project type

Design implication:

- workspace `root` is only path resolution base; panes define their own cwd.

### Privacy / Sensitive Work

The user runs production commands or chats with private agents.

Required behavior:

- do not commit command history, scrollback, conversations, tokens, SSH hosts
  unless explicitly exported
- make trust decisions local and revocable
- allow project layouts to omit agent model/provider choices

Design implication:

- committed layout cannot be the source of secrets or private histories.

### Cross-Platform Team

The same repo layout is used on macOS, Windows, and Linux.

Required behavior:

- path handling is relative when possible
- platform-specific startup commands are possible without forking the file
- unsupported panes/surfaces degrade with a clear message

Design implication:

- future schema should support per-platform command variants before `auto`
  restore can be trusted broadly.

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
provider = "openai"
model = "gpt-5.2"

[tabs.layout]
kind = "split"
direction = "horizontal"
ratio = 0.58
first = { kind = "pane", id = "editor" }

[tabs.layout.second]
kind = "split"
direction = "vertical"
ratio = 0.50
first = { kind = "pane", id = "tests" }
second = { kind = "pane", id = "server" }

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
- Agent provider/model in the project layout are defaults only. Credentials
  always come from user config. Conversation IDs stay private in local runtime
  state unless a user deliberately exports a transcript.
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
- trust decisions for startup commands, keyed by canonical root, layout path,
  and layout content hash
- last accepted restore choices, so the user can rerun only the panes they care
  about

Do not write noisy local state into `.con/workspace.toml`.

Suggested path:

```text
<app-data>/workspace-overlays/<hash-of-root-and-layout>.json
```

The overlay key must include the canonical root and layout file path so two
repos with the same name do not collide.

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

Project layout may define provider/model defaults, but should not commit
private conversation IDs unless explicitly exported as a transcript.

### History

Keep two history channels:

- command suggestion history: persisted privately and optionally scoped by
  project root
- project layout commands: explicit `run` entries in `.con/workspace.toml`

Do not write every command a user typed into the git layout file.

### Startup Commands

Startup commands are product-critical and dangerous.

Policies:

- `manual`: show the command as a suggestion; user starts it.
- `ask`: show the command in a restore sheet with a Run checkbox.
- `auto`: only after the user locally trusts this workspace layout hash.

The restore sheet should show:

- workspace name/root
- each command
- target tab/pane/surface
- cwd
- whether the command came from project layout or local overlay
- a "trust this workspace layout" control scoped to root + file + hash

The restore sheet is mandatory before any command from git is run.

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
   Current mitigation: if the live control endpoint is reachable, open a fresh
   history-backed session instead of restoring the saved layout again. Target
   behavior: prefer single-instance forwarding to the running app. If
   unsupported on a platform, use a per-process session lock so only one process
   writes runtime state. A second process can open a fresh window but must not
   overwrite the first process's window state on exit.

## Compatibility With Existing Control Plane

No existing `panes.*` behavior should change.

Additive API surface:

- `workspaces.get`
- `workspaces.save`
- `workspaces.open`
- `workspaces.export`
- `workspaces.import`
- `workspaces.list`
- `workspaces.validate`
- `workspaces.trust`

Existing `tree.get` can become the base snapshot for export. The export layer
should convert live `PaneTree` into project layout TOML with stable pane/surface
IDs and relative paths.

Agents and external orchestrators should continue to prefer:

- `pane_id` for visible split panes
- `surface_id` for pane-local terminal sessions

## Implementation Plan

### Phase 1 — Lossless Internal Snapshot

Goal: make current private restore complete before adding a public file format.

- Status: implemented in the first issue #111 slice.
- Extended `PaneLayoutState::Leaf` to store surfaces:
  - `surfaces: Vec<SurfaceState>`
  - `active_surface_id: Option<usize>`
- Added `SurfaceState`:
  - `surface_id`
  - `title`
  - `owner`
  - `cwd`
  - `close_pane_when_last`
- Updated `PaneTree::to_state` and `PaneTree::from_state` to round-trip all
  surfaces.
- Preserved backward compatibility with old leaves that only have `cwd`.
- Added con-core serialization tests for legacy leaf restore and multi-surface
  leaf state.

### Phase 1.5 — Project Layout Schema

Goal: make the git-compatible format concrete before wiring UI.

- Status: implemented as `con-core::workspace_layout`.
- Added typed TOML structs for workspace defaults, tabs, layout nodes, panes,
  surfaces, and restore policy.
- Added validation for duplicate IDs, dangling active pane/surface refs,
  dangling layout refs, and unsafe split ratios.
- Added TOML round-trip tests and validation-failure tests.

### Phase 2 — App-State v1

Goal: fix the single-window session model.

- Introduce `AppState` with `windows: Vec<WindowState>`.
- Migrate old `Session` into one `WindowState`.
- Save each window independently with a window UUID.
- Only restore saved windows on true app startup, not on New Window.
- Keep `GlobalHistoryState` independent.
- Add a writer lock or generation stamp so multiple processes cannot
  overwrite each other's runtime state.
- Add a user setting: "Restore windows on launch".
- Treat Dock click after all windows close as reopen semantics, not crash
  recovery.

### Phase 3 — Project Workspace TOML

Goal: make `.con/workspace.toml` useful and git-friendly.

- Status: schema implemented; import/export wiring pending.
- Implement import/export between `WorkspaceLayout` and live `Session`/`PaneTree`.
- Add command palette actions:
  - Save Workspace Layout
  - Open Workspace Layout
  - Export Current Layout
  - Reload Workspace Layout
- Add a validation surface before save, including warnings for absolute paths,
  commands, and provider/model defaults.
- Add `con-cli workspaces export/import/open`.
- Start with manual file operations; later add UI.

### Phase 4 — Restore Policy

Goal: make startup commands safe and intentional.

- `restore = "manual" | "ask" | "auto"` exists in schema.
- Default to `manual`.
- On workspace open, show a compact restore sheet listing commands to run.
- Never auto-run commands from a git file without user approval unless a
  user-local trust record exists for that file hash/root.
- Add per-platform command variants before encouraging `auto` in shared repos.

### Phase 5 — UX Polish

Goal: make the feature discoverable without becoming IDE-heavy.

- Surface workspace name in the title/tab sidebar.
- In settings, add "Restore windows on launch" and "Trust workspace startup
  commands".
- In command palette, include workspace actions and show their shortcuts.
- Right-click tab/sidebar menu can include "Save Layout to Project".
- Empty state: when a repo has no workspace file, offer "Save current layout
  to this project" only after a user is clearly inside a repo.
- Failure state: if a workspace file is invalid, open a normal shell and show a
  non-blocking repair action.

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
- No secrets in project workspace files.
- No automatic trust for files pulled from git.

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
- Do we need platform-specific startup commands in v1? Recommendation:
  yes before `restore = "auto"` is promoted. A cross-platform team needs this.
- Should `run` be one shell string or argv? Recommendation:
  keep one shell string for human dotfile ergonomics, but store the shell/profile
  that interprets it. CLI can later support argv export for generated layouts.
- Should local overlays be human-editable? Recommendation:
  no. They are app-private runtime state; project layout is the human-editable
  artifact.
