# Quick controls

con keeps the terminal in front. The main controls help you move between the
terminal, command input, and the agent panel without changing context.

| Action | macOS | Windows / Linux |
| --- | --- | --- |
| Switch focus between terminal and input | <kbd>⌘</kbd> <kbd>I</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>I</kbd> |
| Show or hide the bottom input bar | <kbd>⌃</kbd> <kbd>\`</kbd> | <kbd>⌃</kbd> <kbd>\`</kbd> |
| Show or hide the agent panel | <kbd>⌘</kbd> <kbd>L</kbd> | <kbd>⌃</kbd> <kbd>⇧</kbd> <kbd>L</kbd> |
| Cycle bottom-bar mode | <kbd>⌘</kbd> <kbd>;</kbd> | <kbd>⌃</kbd> <kbd>;</kbd> |

## Input modes

### Smart

Smart mode decides whether your text should run as a shell command or go to the
agent. Use it when you want con to choose the obvious path.

### Command

Command mode sends text to the shell. In a multi-pane workspace, the pane picker
lets you choose the focused pane, all panes, or a selected set.

### Agent

Agent mode sends text to the built-in agent. Use it when you want explanation,
planning, code help, or a careful change made with your approval.

## A good default flow

1. Work normally in the terminal.
2. Open the input bar when you need a command or a request.
3. Use Command mode for direct shell work.
4. Use Agent mode when you want con to reason over the visible pane and nearby
   context.
5. Review tool actions before approving them.
