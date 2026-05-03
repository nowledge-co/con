# Workspace Layout Profiles

Con has two workspace promises.

The first is invisible: after rebooting, upgrading, or relaunching Con, your
ordinary workspace should come back. Windows, tabs, panes, surfaces, working
directories, and private terminal text history are continuity. You should not
need a guide for that.

The second is deliberate: when a workspace shape is worth keeping for a project,
you can save it as a layout profile. This guide is for that explicit workflow.

## What A Layout Profile Is

A layout profile is a Con-generated `.con/workspace.toml` file.

It describes workspace shape:

- tab names
- pane split structure
- pane and surface names
- relative working directories
- optional agent provider/model defaults

It does not contain private runtime state:

- no commands to run
- no conversations
- no command history
- no terminal text history
- no credentials
- no trust decisions

Think of it as a profile for recreating a tuned workspace, not as a terminal
session backup. It is safe to review because it describes shape, not activity.

## The Aha Flow

1. Tune the workspace visually until it feels right.
2. Name the tabs, panes, and surfaces so the intent is obvious.
3. Save the layout profile.
4. Review the generated `.con/workspace.toml`.
5. Reopen the project later with `con ~/dev/app`, or commit the file so a
   teammate can get the same starting shape.

The profile captures the workspace you designed. It does not capture what you
typed, what the agent said, or what processes were running.

Private terminal text restore can be disabled in **Settings -> General ->
Continuity**. Use **Clear Restored Terminal History** from Command Palette when
you want to turn it off and wipe the saved terminal text already stored by Con.
New installs enable continuity by default; existing config files created before
this setting existed stay off until you turn it on.

## Save A Profile

1. Open a project in Con.
2. Arrange tabs, panes, and surfaces visually.
3. Rename tabs, panes, and surfaces so the layout is understandable.
4. Choose **Save Layout Profile** from Command Palette or the Workspace menu.
5. Save as `.con/workspace.toml` under the project root.
6. Review the generated file.
7. Commit it only if the layout is useful to the project.

Example:

```text
Project: ~/dev/app
  Tab: Dev
    Pane: Shell      cwd .
    Pane: Server     cwd crates/server
    Pane: Tests      cwd crates/server
  Tab: Agents
    Surface: Planner cwd .
    Surface: Worker  cwd crates/ui
```

The exported file should recreate that shape, not the processes that happened
to be running inside it.

Path values are written as repo-relative slash paths, even on Windows:

```toml
cwd = "crates/server"
```

That keeps the file stable in git diffs and usable across machines.

## Open A Project Profile

Open a project profile explicitly:

```sh
con ~/dev/app
```

Con opens `~/dev/app/.con/workspace.toml` when it exists. If no profile exists,
Con opens a fresh shell rooted at `~/dev/app`.

Open a profile file directly:

```sh
con ~/dev/app/.con/workspace.toml
```

If the requested profile is malformed, Con opens a fresh shell and shows the
profile error in the terminal. It does not silently restore an unrelated
workspace.

Inside the app, use:

- **Add Tabs from Layout Profile** to choose a project folder or profile file and add
  its tabs to the current window. If the folder has no profile, Con adds one
  fresh tab rooted there.
- **Open Layout Profile in New Window** to choose a project folder or profile
  file and open it separately.

Plain `con` without a path still favors private session restore. That keeps
normal relaunch behavior predictable: quitting, upgrading, or rebooting should
bring back the last private workspace, not silently replace it with a project
profile because the process happened to start from a repo directory.

The more aggressive `cd ~/dev/app && con` project-detection flow is deferred
until Con has full single-instance forwarding and project-memory state. At that
point Con can distinguish "open this project" from "restore my last app".

## Share A Profile

Commit `.con/workspace.toml` when the layout is useful to other people on the
project.

Good shared profile content:

- "Dev", "Server", "Tests", "Release" tab names
- repo-relative cwd values
- panes and surfaces with clear names
- no machine-specific absolute paths unless unavoidable

Bad shared profile content:

- personal history
- output transcripts
- secrets
- commands that run automatically
- private agent conversations

## New Tab, New Window, And Defaults

Use these rules:

- **New Tab** stays scratch by default. It should not surprise you by expanding
  into a multi-pane project layout.
- **Add Tabs from Layout Profile** is the explicit "new tab(s) from this profile"
  flow. If the profile contains one tab, it behaves like a new tab. If it
  contains several tabs, Con adds all of them.
- **Open Layout Profile in New Window** is the explicit "new window from this
  profile" flow.
- **New Window** stays scratch by default. A global "default new-window layout"
  setting is intentionally deferred because it can fight with private restore
  and project memory.

## Gesture Semantics

| Gesture | Result |
| --- | --- |
| Launch Con normally | Restore the private workspace you left behind. |
| Cmd+N / New Window | Open one clean scratch shell with shared history. |
| `con ~/dev/app` | Open the project's profile if present; otherwise one shell rooted there. |
| `con ~/dev/app/.con/workspace.toml` | Open that profile directly. |
| Add Tabs from Layout Profile | Add the selected project/profile into the current window. |
| Open Layout Profile in New Window | Open the selected project/profile separately. |

This keeps every entry point legible: restore is automatic, scratch is fresh,
and project layout is explicit.

## Process Continuity

Layout profiles do not resume running processes. Use tmux or zellij when you
want processes to survive app restarts:

```sh
tmux attach -t app || tmux new -s app
```

Con restores the layout and directory. tmux restores the running session.
