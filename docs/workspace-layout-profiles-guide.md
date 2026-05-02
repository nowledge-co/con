# Workspace Layout Profiles

Con restores your ordinary workspace automatically. You should not need a guide
for that. After rebooting, upgrading, or relaunching Con, your windows, tabs,
panes, surfaces, working directories, and private terminal text history should
come back as continuity.

This guide is for the explicit workflow: saving a reusable layout profile for a
project.

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

Production target:

1. Open a project in Con.
2. Arrange tabs, panes, and surfaces visually.
3. Rename tabs, panes, and surfaces so the layout is understandable.
4. Choose Export Current Layout.
5. Review the generated `.con/workspace.toml`.
6. Commit it only if the layout is useful to the project.

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

Production target:

```sh
cd ~/dev/app
con
```

or:

```sh
con ~/dev/app
```

Con should detect the project profile, open the layout under that project root,
and apply your private local memory on top when available.

If no profile exists, Con opens a normal fresh project shell and can later offer
to export the current layout.

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

## Process Continuity

Layout profiles do not resume running processes. Use tmux or zellij when you
want processes to survive app restarts:

```sh
tmux attach -t app || tmux new -s app
```

Con restores the layout and directory. tmux restores the running session.
