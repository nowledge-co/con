# Restorable Workspaces Guide

Con restores your workspace shape so you can return to work quickly. It restores
layout and context, not running processes.

## What Con Restores

Con restores:

- windows, tabs, panes, and pane splits
- pane-local surfaces and their names
- working directories
- active tab, focused pane, and active surface
- command and input history
- tab-owned agent conversations from your private app data

Con does not automatically restore:

- running shell jobs
- TUI process state
- terminal scrollback
- commands from a project file

If you need real process continuity, use tmux or zellij inside Con.

## Continue Where You Left Off

Use Con normally, then quit the app. On the next launch, Con restores your
private workspace shape.

Example:

```text
Window
  Dev tab
    left pane: ~/dev/app
    right pane: ~/dev/app/crates/server
  Agents tab
    surface: Planner
    surface: Worker
```

When Con opens again, those tabs, panes, directories, and surfaces return.
Commands are not rerun.

## Open a Scratch Window

Press Cmd+N on macOS, or use New Window from the menu.

Con opens one clean shell with your normal history and defaults. It does not
copy the current project's tabs, panes, surfaces, or agent conversation.

Use this for quick commands, temporary shells, or unrelated work.

## Open a Project

Production target:

```sh
con ~/dev/app
```

If Con has private memory for that project, it restores the local shape you used
last time. If not, it opens one shell at the project root.

This memory stays in your app data. Con does not write workspace files into the
repo unless you explicitly export something in a future workflow.

## Use Surfaces for Agents

Surfaces are tab-like sessions inside one pane. They are useful when an
orchestrator or coding agent needs several named terminal contexts in the same
visible pane.

Example:

```text
Pane: ~/dev/app
  Surface: Planner
  Surface: Worker 1
  Surface: Worker 2
```

After restart, Con restores those surfaces with their names and directories so
both humans and tools can target them again.

## Use tmux for Process Continuity

If you want processes to survive app restarts, use tmux:

```sh
tmux attach -t app || tmux new -s app
```

Con restores the pane and cwd. tmux restores the running terminal session.

## Privacy Rules

Con keeps these private:

- conversations
- command history
- window geometry
- active focus
- credentials and tokens
- trust decisions

Future project files, if added, should only describe safe shared intent such as
named tasks or layout shape.
