# Agent Tool Surface — Design

## Mental Model

The agent is an **orchestrator**, not a keyboard user. It has direct API access to terminal management, command execution, and content observation. It should never simulate human application shortcuts (Cmd+T, Cmd+W) — if a capability isn't exposed as a tool, it doesn't exist for the agent.

### Keystroke categories

The agent must distinguish three types of keystrokes:

1. **con application shortcuts** (Cmd+T, Cmd+W, Cmd+N): Control the con app. The agent CANNOT send these. Use tools instead.
2. **Terminal protocol sequences** (\\x02c for tmux prefix+c, \\x1b for Escape, \\x03 for Ctrl-C): Travel through the PTY to the remote program. send_keys is the correct tool for these.
3. **Shell commands** (ls, apt update, git status): Use terminal_exec when exec_visible_shell is available. Otherwise, send_keys to type the command + Enter in a shell prompt.

## Three Abstraction Levels

```
Level 1: Terminal Management          create_pane
         (orchestrator)               The agent controls the terminal layout.

Level 2: Command Execution            terminal_exec, batch_exec, wait_for
         (shell)                      Run commands and get structured output.

Level 3: Interactive TUI              send_keys + read_pane
         (keystrokes)                 Drive vim, tmux, interactive programs.
```

The agent should **prefer higher levels**. Level 3 (send_keys) is the last resort — only for programs that require interactive keystroke input, or when exec_visible_shell is unavailable (SSH without shell integration, inside tmux).

## Decision Framework

```
Is this a shell command on a pane with exec_visible_shell?
  → terminal_exec (single) or batch_exec (parallel)

Is this a shell command on a pane WITHOUT exec_visible_shell?
  → send_keys "command\n" (after read_pane to confirm shell prompt)

Need to run on a host not currently connected?
  → create_pane(command: "ssh hostname") — output shows connection result

Need to run on multiple hosts in parallel?
  → create_pane for each host — each returns output showing connection state
  → terminal_exec (if exec_visible_shell) or send_keys for each

Need to interact with a TUI (vim, htop, etc.)?
  → send_keys + read_pane (follow playbooks)

Need to wait for a long-running command?
  → wait_for(pane, timeout) — works universally (see idle detection below)

Need file operations on the local machine?
  → file_read, file_write, edit_file, search, list_files

Need file operations on a remote host?
  → send_keys with heredoc (Level 3 — no direct remote file access)
```

## Tool Design Principles

These principles govern the design of every agent tool. They are derived from production failures — each one exists because violating it caused a real, observable breakdown in agent behavior.

### 1. Don't force the model to predict

If a tool requires the model to guess what will happen, the tool is broken. The model doesn't know what apt will print, whether SSH will ask for a password, or how long a build takes. When we force predictions, the model guesses wrong and the system hangs or fails silently.

**Test**: Can the model use this tool correctly with zero domain knowledge about the command it's running? If not, the tool needs to observe on the model's behalf.

Applied:
- `wait_for` idle mode: the system detects completion (via shell integration or quiescence) — the model doesn't guess output text
- `create_pane` returns initial output: the model sees "connected" or "Password:" — doesn't predict auth method
- Pattern mode is last resort, not default

### 2. If a parameter exists, the model will fill it

Every schema parameter is a decision point. Models are thorough — they fill optional parameters "to be safe." If the system knows the right value (timeouts, polling intervals, buffer sizes), don't expose it. The model should express **intent**, not **mechanics**.

**Test**: For each optional parameter, ask: "Is there ever a case where the model choosing a value produces a better outcome than the system's default?" If no, remove it from the schema. Keep it in the Rust struct for internal use.

Applied:
- `timeout_secs` removed from `wait_for` schema — system uses 30s default, model retries on timeout
- Polling intervals are internal — never exposed

### 3. Tool responses must be self-contained for the next decision

A tool response should contain everything the model needs to decide its next action. If the model must immediately call another tool to understand what happened, the first tool's response is incomplete.

**Test**: After receiving this response, can the model decide what to do next without another tool call? If not, include the missing information.

Applied:
- `create_pane` returns `output` alongside `pane_index` — no follow-up `read_pane` needed
- `wait_for` returns `output` (last 50 lines) alongside `status` — on timeout, the model can see what's happening
- `terminal_exec` returns stdout/stderr/exit_code — complete picture

### 4. Degrade gracefully, never refuse

When a capability isn't available (no shell integration, no OSC 133), fall back to something useful. Never return an error that says "can't help" when a less precise but functional alternative exists.

**Test**: Does this tool work on a bare SSH pane with no shell integration? If not, what's the fallback?

Applied:
- `wait_for` without shell integration → quiescence detection (output stability)
- `terminal_exec` without `exec_visible_shell` → clear error with suggested alternative (send_keys)

### 5. Normalize at the boundary

Data from external systems (terminal grid, PTY output, FFI returns) has quirks the model can't handle — trailing whitespace from cursor position, ANSI codes, encoding variations. Normalize at the system boundary, before the data reaches any comparison logic or the model.

**Test**: If I run the same command twice and `diff` the tool's output, do I get identical results? If not, normalize whatever varies.

Applied:
- `trim_end()` on each line before quiescence comparison — cursor position doesn't affect stability detection
- `recent_lines()` returns plain text (no ANSI) — model sees clean content
- Empty pattern `""` normalized to `None` — Rust's `"".contains("")` edge case handled at entry

## Observe Before Acting

The agent must never assume terminal state. Before acting on any pane:
- Before send_keys: read_pane to see what is on screen
- After send_keys: read_pane to verify the action took effect
- After create_pane: check the returned output (included in response)
- Never chain multiple actions without observing between them

## Tools

### Level 1: Terminal Management

#### `create_pane`

Create a new terminal pane (split in current tab). Optionally run a startup command.

```
Parameters:
  command: Option<String>    Start command (e.g., "ssh host"). Executes automatically.

Returns:
  pane_index: usize          The new pane's 1-indexed position.
  command: Option<String>    Echo of the startup command.
  output: String             Initial terminal output (settled via quiescence).
```

Implementation: pane creation is deferred to render cycle (ghostty surfaces need a window context). The command is written to the PTY via `write_or_queue()` which buffers data if the ghostty surface hasn't initialized yet, flushing when `ensure_initialized()` completes. After creation, the tool polls `read_pane` with quiescence detection (3 stable polls × 500ms = 1.5s, max 8s budget) and returns the settled output.

Design notes:
- The `command` executes automatically — the model must NOT re-send it via send_keys.
- Returns `CreatePaneOutput` struct (not a string) to avoid double-encoding.
- **Output is included in the response.** The model sees what happened (SSH connected with prompt, password prompt, connection refused) without a follow-up `read_pane`. This eliminates the "guess what SSH will do" anti-pattern — the model observes, doesn't predict.
- New pane is a horizontal split within the agent's tab.

### Level 2: Command Execution

#### `terminal_exec`

Run a shell command in a visible pane, wait for completion, return structured output.
Requires `exec_visible_shell` capability (shell integration). 60-second timeout with fallback polling.

#### `batch_exec`

Run multiple commands in parallel across panes. Same exec_visible_shell requirement.
Returns `serde_json::Value` (not String) to avoid double-encoding.

#### `wait_for`

Wait for a pane to become idle or match a text pattern.

```
Parameters:
  pane_index: usize
  timeout_secs: Option<u64>      Default 30. Max 120. Short timeouts + retry preferred.
  pattern: Option<String>        Wait for this text to appear in output.

Returns:
  status: "idle" | "matched" | "timeout"
  output: String                 Recent output (last 50 lines).
```

Implementation: delegates to workspace-side GPUI async task with direct terminal access (no per-tick channel roundtrips). Three detection modes:

| Mode | Condition | Polling | Detection |
|------|-----------|---------|-----------|
| Shell integration idle | no pattern + has SI | 100ms | `take_command_finished()` or `!is_busy()` |
| Quiescence | no pattern + no SI | 500ms | Normalized output unchanged for 4 consecutive polls (2s) |
| Pattern match | pattern provided | 500ms | Substring match in `recent_lines(50)` |

Design notes:
- **Idle mode works universally.** With shell integration, uses precise OSC 133 D signals. Without it (remote SSH panes), falls back to output quiescence — detects when normalized output (trailing whitespace trimmed) stops changing for 2 seconds. The model doesn't need to know which mode is active.
- **Prefer idle mode (no pattern).** Pattern mode requires guessing what the output will contain. Idle mode just waits for the command to finish.
- **Empty pattern normalized to idle.** `pattern: Some("")` is treated as `None` — Rust's `"".contains("")` is always true, which would cause instant false match. The handler normalizes at entry.
- Quiescence baseline is captured inside the async task to avoid race between snapshot and first poll. Lines are `trim_end()`'d before comparison to neutralize cursor-position-dependent trailing whitespace from `ghostty_surface_read_text`. Empty baselines are deferred — counting starts only after the first non-empty snapshot.
- The tool sends a single `PaneQuery::WaitFor` and blocks on `response_rx.recv_timeout()`.

#### `shell_exec`

Run a command on the local machine (invisible to user). For git, file searches, package lookups.

### Level 3: Interactive TUI

#### `send_keys`

Send keystrokes to a pane. For TUI interaction (vim, tmux, interactive programs) and shell commands on panes without exec_visible_shell.

Implementation: decodes escape sequences (\\x1b, \\x03, \\n, etc. and bare hex fallback for weaker models), splits at standalone ESC boundaries with 10ms inter-segment delay to avoid VT parser ambiguity.

#### `read_pane`

Read recent terminal output. Default 50 lines. Returns plain text from ghostty's grid (no ANSI codes).

### Observation Tools

- `list_panes`: Enumerate all panes with full metadata. Returns `serde_json::Value`.
- `search_panes`: Search scrollback content across panes by regex.
- `tmux_inspect`: Query tmux state within a pane. Returns `serde_json::Value`.

### File Tools

All file tools (`file_read`, `file_write`, `edit_file`, `search`, `list_files`) are local-only by design. Cannot access remote SSH hosts.

## System Prompt Architecture

The system prompt is constructed dynamically by `TerminalContext::to_system_prompt()`:

1. **Decision framework** — keystroke categories, tool selection matrix
2. **Tool descriptions** — brief summaries (model also sees full tool schemas from rig)
3. **Safety rules** — destructive command guards, addressing rules
4. **Verify-before-act** — universal observe-before-acting principle
5. **TUI guide** (conditional) — emitted only when TUI panes are detected:
   - VERIFY_AFTER_ACT — detailed send_keys procedure
   - REMOTE_WORK — when focused pane has a remote host
   - TMUX_PLAYBOOK — when tmux is in any target stack
   - VIM_PLAYBOOK — when vim/nvim is visible
   - GENERAL_TUI — fallback for unknown TUIs
6. **Terminal context** — focused pane state, other pane summaries

Token budget: ~7,750 tokens for TUI sessions, ~5,000 for shell-only sessions.
