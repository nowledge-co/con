# Workspace Layout Profiles

Con restores your ordinary workspace automatically. You should not need a guide
for that. After rebooting, upgrading, or relaunching Con, your windows, tabs,
panes, surfaces, working directories, and private terminal text history should
come back as continuity.

This guide is for the explicit workflow: saving or opening a reusable layout
profile for a project.

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
session backup.

## Export A Profile

1. Open a project in Con.
2. Arrange tabs, panes, and surfaces visually.
3. Rename tabs, panes, and surfaces so the layout is understandable.
4. Choose **Export Current Layout** from Command Palette or the File menu.
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

Inside the app, use:

- **Add Layout Profile Tabs** to choose a project folder or profile file and add
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
- **Add Layout Profile Tabs** is the explicit "new tab(s) from this profile"
  flow. If the profile contains one tab, it behaves like a new tab. If it
  contains several tabs, Con adds all of them.
- **Open Layout Profile in New Window** is the explicit "new window from this
  profile" flow.
- **New Window** stays scratch by default. A global "default new-window layout"
  setting is intentionally deferred because it can fight with private restore
  and project memory.

## Process Continuity

Layout profiles do not resume running processes. Use tmux or zellij when you
want processes to survive app restarts:

```sh
tmux attach -t app || tmux new -s app
```

Con restores the layout and directory. tmux restores the running session.
