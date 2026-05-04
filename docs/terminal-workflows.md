# Terminal workflows

con is still a terminal when every AI feature is hidden. Start there. Use the
agent panel, input bar, surfaces, and automation only when they fit the work in
front of you.

## The workspace model

con uses a few names consistently:

| Name | Meaning | Use it for |
| --- | --- | --- |
| Window | A top-level app window | Separate projects or monitors |
| Tab | A workspace inside a window | One project, task, or long-running shell set |
| Pane | A visible split region inside a tab | Side-by-side shells, servers, logs, editors |
| Surface | A terminal session inside one pane | Multiple agent or worker sessions without adding more visible splits |

Most terminal work only needs tabs and panes. Surfaces are for the cases where a
single visible pane should host several terminal sessions.

## Focus without reaching for the mouse

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Switch focus between terminal and input | <kbd>⌘</kbd> <kbd>I</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>I</kbd> |
| Show or hide the input bar | <kbd>⌃</kbd> <kbd>`</kbd> | <kbd>⌃</kbd> <kbd>`</kbd> |
| Show or hide the agent panel | <kbd>⌘</kbd> <kbd>L</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>L</kbd> |
| Show or hide vertical tabs | <kbd>⌘</kbd> <kbd>B</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>B</kbd> |
| Open Command Palette | <kbd>⌘</kbd> <kbd>⇧</kbd> <kbd>P</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>P</kbd> |

When in doubt, open Command Palette and search by the action name.

## Tabs and windows

Use a new tab when the work belongs to the same window. Use a new window when
you want a separate space, usually for another project or display.

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| New window | <kbd>⌘</kbd> <kbd>N</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>N</kbd> |
| New tab | <kbd>⌘</kbd> <kbd>T</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>T</kbd> |
| Next tab | <kbd>⌃</kbd> <kbd>Tab</kbd> | <kbd>⌃</kbd> <kbd>Tab</kbd> |
| Previous tab | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>Tab</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>Tab</kbd> |
| Select tab 1-9 | <kbd>⌘</kbd> <kbd>1</kbd> ... <kbd>9</kbd> | <kbd>⌃</kbd> <kbd>1</kbd> ... <kbd>9</kbd> |

Vertical tabs are useful once tab titles matter more than tab position. Rename a
tab when its purpose is stable.

## Panes

Panes are visible splits. They are the right tool when you need to see more than
one terminal at once.

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Split right | <kbd>⌘</kbd> <kbd>D</kbd> | <kbd>Alt</kbd> <kbd>D</kbd> |
| Split down | <kbd>⌘</kbd> <kbd>⇧</kbd> <kbd>D</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>D</kbd> |
| Toggle pane zoom | <kbd>⌘</kbd> <kbd>⇧</kbd> <kbd>Enter</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>Enter</kbd> |
| Close pane | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>W</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>W</kbd> |

Pane zoom is for short bursts of focus. It does not stop sibling panes; it only
gives the active pane the full terminal area.

## Command input and broadcast

The bottom input bar has three modes:

- **Smart** chooses between shell command and agent request.
- **Command** sends text to the terminal.
- **Agent** sends text to the built-in agent.

Command mode can target more than one pane. Use the pane picker to send a
command to the focused pane, every pane, or a selected set. The picker mirrors
the current pane layout, so you can choose by position instead of remembering
pane numbers.

## Surfaces

A surface is a terminal session inside one pane. Think of it as a pane-local
tab.

Use surfaces when you want several live terminal sessions but do not want to
keep splitting the screen smaller. This is especially useful for coding agents,
subagents, or task-specific shells that should share one visible worker area.

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| New surface in focused pane | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>T</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>T</kbd> |
| New surface pane right | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>D</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>→</kbd> |
| New surface pane down | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>⇧</kbd> <kbd>D</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>↓</kbd> |
| Next surface | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>]</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>]</kbd> |
| Previous surface | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>[</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>[</kbd> |
| Rename surface | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>R</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>R</kbd> |
| Close surface | <kbd>⌘</kbd> <kbd>⌥</kbd> <kbd>⇧</kbd> <kbd>W</kbd> | <kbd>Alt</kbd> <kbd>⇧</kbd> <kbd>X</kbd> |

Only panes with multiple surfaces, owned orchestrator sessions, or explicit
surface names show the in-pane surface strip. Ordinary panes stay clean.

## Terminal menu

Right-click the terminal for common actions:

- copy and paste
- clear terminal
- split or close panes
- zoom the pane
- create, rename, switch, or close surfaces
- focus the input bar
- open Settings or Command Palette

The menu uses the same actions as shortcuts and Command Palette. If an action
has a shortcut, the menu shows it.

## Links, paste, and files

Use the platform modifier to open links from terminal output:

- macOS: hold <kbd>⌘</kbd>, then click a URL.
- Windows and Linux: hold <kbd>⌃</kbd>, then click a URL.

Paste text normally. Dragging files into the terminal sends their paths. When a
TUI supports image/file paste protocols, con forwards compatible clipboard and
drop payloads through the terminal path.

## Restore and profiles

Normal relaunch restore is automatic: con brings back windows, tabs, panes,
working directories, and bounded private terminal text.

Layout profiles are different. They are project files for recreating a workspace
shape later or sharing it with a team. See [Workspace profiles](workspace-layout-profiles-guide.md).
