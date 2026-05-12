# Quick controls

con keeps the terminal in front. The main controls help you move between the
terminal, command input, and the agent panel without changing context.

## Two-minute demo

Watch the short flow once, then use the table below as a reference.

<video controls muted playsinline preload="metadata" width="100%" aria-label="Two-minute con quick controls demo" src="https://github.com/user-attachments/assets/2b6f6145-e400-4a74-a951-cd8221493a17"></video>

## Shortcuts

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Switch focus between terminal and input | <kbd>⌘</kbd> <kbd>I</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>I</kbd> |
| Show or hide the bottom input bar | <kbd>⌃</kbd> <kbd>\`</kbd> | <kbd>⌃</kbd> <kbd>\`</kbd> |
| Show or hide the agent panel | <kbd>⌘</kbd> <kbd>L</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>L</kbd> |
| Show or hide vertical tabs | <kbd>⌘</kbd> <kbd>B</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>B</kbd> |
| Focus Files | <kbd>⌘</kbd> <kbd>⇧</kbd> <kbd>E</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>E</kbd> |
| Search Files | <kbd>⌘</kbd> <kbd>⇧</kbd> <kbd>F</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>F</kbd> |
| Cycle bottom-bar mode | <kbd>⌘</kbd> <kbd>;</kbd> | <kbd>⌃</kbd> <kbd>;</kbd> |
| Show or hide Quick Terminal | <kbd>⌘</kbd> <kbd>Backslash</kbd> after enabling | Not available |

## Input modes

### Smart

Smart mode decides whether your text should run as a shell command or go to the
agent. Use it when you want con to choose the obvious path without making you
switch surfaces first.

### Command

Command mode sends text to the shell. In a multi-pane workspace, the pane picker
lets you choose the focused pane, all panes, or a selected set.

### Agent

Agent mode sends text to the built-in agent. Use it when you want explanation,
planning, code help, or a careful change made with your approval.

These modes are why the input bar exists. It is not a second terminal prompt and
not a chat box glued to the side. It is a short path from what you are looking at
to the next action.

## Quick Terminal on macOS

Quick Terminal is an optional macOS-only drop-down terminal. Enable it in
Settings -> Keys, then use the shortcut to slide down a dedicated Con window
from the top of the active screen, even when another app is frontmost.

While Con is frontmost, you can also open Quick Terminal from the command
palette or **View -> Quick Terminal** without enabling the global shortcut.

It is separate from the main window. Hiding it keeps its live tabs, panes, cwd,
and scrollback. Closing its last tab or exiting its last shell destroys it, and
the next shortcut creates a fresh one.

For setup, behavior, and the difference from Summon / Hide Con, see
[Quick Terminal](quick-terminal.md).

## A good default flow

1. Work normally in the terminal.
2. Open the input bar when you need a command or a request.
3. Use Command mode for direct shell work.
4. Use Agent mode when you want con to reason over the visible pane and nearby
   context.
5. Review tool actions before approving them.

## Next

When you want to tune providers, suggestions, themes, skills, or shortcuts, open
[Settings](settings.md).

For tabs, panes, broadcast, links, and pane-local surfaces, see
[Terminal workflows](terminal-workflows.md).
