# Agent Tool Surface — Design

## Mental Model

The agent is an **orchestrator**, not a keyboard user. It has direct API access to terminal management, command execution, and content observation. It should never simulate human keyboard shortcuts (Cmd+T, Cmd+W, Ctrl+B+t) — if a capability isn't exposed as a tool, it doesn't exist for the agent.

## Three Abstraction Levels

```
Level 1: Terminal Management          create_pane, close_pane
         (orchestrator)               The agent controls the terminal layout.

Level 2: Command Execution            terminal_exec, batch_exec, wait_for
         (shell)                      Run commands and get structured output.

Level 3: Interactive TUI              send_keys + read_pane
         (keystrokes)                 Drive vim, tmux, interactive programs.
```

The agent should **prefer higher levels**. Level 3 (send_keys) is the last resort — only for programs that require interactive keystroke input.

## Decision Framework

```
Is this a shell command on a visible pane?
  → terminal_exec (single) or batch_exec (parallel)

Need to run on a host not currently connected?
  → create_pane(command: "ssh hostname") → terminal_exec in new pane

Need to run on multiple hosts in parallel?
  → create_pane for each host → batch_exec across all

Need to interact with a TUI (vim, htop, etc.)?
  → send_keys + read_pane (follow playbooks)

Need to wait for a long-running command?
  → wait_for(pane, timeout, pattern)

Need file operations on the local machine?
  → file_read, file_write, edit_file, search, list_files

Need file operations on a remote host?
  → send_keys with heredoc (Level 3 — no direct remote file access)
```

## Tools

### Level 1: Terminal Management

#### `create_pane` (NEW)

Create a new terminal pane. Returns the pane index for subsequent tool calls.

```
Parameters:
  command: Option<String>    Start command (e.g., "ssh cinnamon"). None = local shell.

Returns:
  pane_index: usize          The new pane's index for addressing.
```

Implementation: dispatches `PaneRequest::CreatePane` to workspace, which calls `new_tab()` or equivalent, optionally writes the command to the PTY, and returns the assigned pane index.

Design notes:
- No split direction parameter for now. New pane = new tab. Splits are a visual layout concern the user controls.
- The `command` parameter is convenience sugar — equivalent to create_pane + send_keys, but saves a round-trip and ensures the command runs in a clean shell.
- The agent sees the new pane in subsequent `list_panes` calls.

#### `close_pane` (DEFERRED)

Not implementing initially. The agent should rarely need to close panes. The user can close them manually. If the agent creates temporary panes (e.g., for parallel SSH), it should leave them open so the user can inspect results.

### Level 2: Command Execution

#### `terminal_exec` (EXISTS — unchanged)

Run a shell command in a visible pane, wait for completion, return structured output.

Requires shell integration. 60-second timeout. Best for short commands.

#### `batch_exec` (EXISTS — unchanged)

Run multiple commands in parallel across panes. Returns all results.

#### `wait_for` (NEW)

Block until a pane becomes idle or matches a text pattern. Replaces the read_pane polling loop.

```
Parameters:
  pane_index: usize
  timeout_secs: Option<u64>      Default 120. Max 600.
  pattern: Option<String>        Wait for this text to appear in output.

Returns:
  status: "idle" | "matched" | "timeout"
  output: String                 Recent output (last 50 lines).
```

Implementation: polls the pane's `is_busy` flag and/or searches for `pattern` in recent output. Polls internally (e.g., every 500ms) — the agent doesn't need to know about the polling. Returns when condition is met or timeout expires.

Design notes:
- `idle` means the shell prompt returned (is_busy went false).
- `matched` means the pattern appeared in scrollback.
- `timeout` means neither happened within the timeout period.
- The 500ms internal poll interval is an implementation detail, not exposed.
- This is NOT a streaming tool — it returns once at the end. For progress monitoring, the agent uses `read_pane` between `wait_for` calls.

#### `shell_exec` (EXISTS — minor improvement)

Run a command on the local machine (invisible to user). Currently has no timeout parameter.

Improvement: add `timeout_secs: Option<u64>` (default 30).

### Level 3: Interactive TUI

#### `send_keys` (EXISTS — unchanged)

Send keystrokes to a pane. For TUI interaction only (vim, tmux, interactive programs).

The agent should NEVER use send_keys for:
- Running shell commands (use terminal_exec)
- Creating panes (use create_pane)
- Application shortcuts (Cmd+T, Cmd+W — not the agent's domain)

#### `read_pane` (EXISTS — minor improvement)

Read recent terminal output. Default 50 lines.

Current `lines` parameter works well. No changes needed — the agent already controls how many lines to read. The issue was the agent reading too many lines, which is a prompt/guidance problem, not a tool problem.

### Observation Tools

#### `list_panes` (EXISTS — unchanged)

Enumerate all panes with full metadata.

#### `search_panes` (EXISTS — unchanged)

Search scrollback content across panes.

#### `tmux_inspect` (EXISTS — unchanged)

Query tmux state within a pane.

### File Tools

All existing file tools (`file_read`, `file_write`, `edit_file`, `search`, `list_files`) remain unchanged. They are local-only by design.

## System Prompt Changes

### Remove from decision framework:
- Any mention of "hotkeys" or "keyboard shortcuts"
- Any implication that the agent can control the application UI

### Add to decision framework:
```
You control the terminal through tools, not keystrokes.
To create a new terminal: use create_pane, not keyboard shortcuts.
To run a shell command: use terminal_exec, not send_keys.
To wait for a command to finish: use wait_for, not repeated read_pane.
send_keys is ONLY for interactive TUI programs (vim, tmux, htop).

For parallel work across multiple hosts:
1. create_pane for each host (with ssh command)
2. batch_exec to run commands in parallel
3. Report results
```

### Efficiency guidance:
```
read_pane default (50 lines) is usually sufficient.
Only increase lines if you need to see earlier output.
Never read more than you need — scrollback is expensive.
Use search_panes instead of reading full scrollback to find specific output.
```

## Implementation Plan

### Phase 1: create_pane + wait_for (critical path)
1. Add `PaneQuery::CreatePane { command }` variant to tools.rs
2. Handle in workspace.rs — call `new_tab()`, optionally write command, return pane index
3. Add `PaneQuery::WaitFor { pane_index, timeout_secs, pattern }` variant
4. Handle in workspace.rs — async poll loop on pane's is_busy/scrollback
5. Register both tools in Rig agent builder
6. Update system prompt in context.rs

### Phase 2: System prompt overhaul
1. Rewrite decision framework to reflect the three levels
2. Remove keystroke-centric language
3. Add clear "never simulate shortcuts" rule
4. Update playbooks to reference create_pane for multi-pane scenarios

### Phase 3: Efficiency
1. Add timeout_secs to shell_exec
2. Optimize list_panes to skip full forensics when only metadata is needed
3. Consider "lightweight" list_panes that returns just indices + titles
