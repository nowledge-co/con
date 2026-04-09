//! Contextual TUI interaction playbooks for the agent system prompt.
//!
//! These are injected into the system prompt only when the focused pane
//! (or any visible pane) contains a TUI target. Shell-only sessions
//! never see this content.

/// Mandatory pattern: every send_keys must be followed by read_pane.
pub const VERIFY_AFTER_ACT: &str = "\
## Verify-after-act (MANDATORY for all TUI interaction)

Every send_keys call MUST be preceded AND followed by read_pane.
1. read_pane FIRST to confirm what is on screen and where your keystrokes will go
2. send_keys to perform the action
3. read_pane AGAIN to verify the action took effect
4. If the screen does not show expected state, diagnose before retrying

Never chain multiple send_keys calls without reading the pane between them.
Do not assume a keystroke was received — always verify.
Do not assume you know what is on screen — always read first.
";

/// tmux navigation and control.
pub const TMUX_PLAYBOOK: &str = "\
## tmux interaction

Preferred path: if list_panes shows `query_tmux`, `exec_tmux_command`, or `send_tmux_keys`, use tmux-native tools first.
1. resolve_work_target when you need con to choose the best pane or tmux workspace first
2. tmux_find_targets or tmux_list_targets to discover exact tmux windows and panes
3. tmux_capture_pane to inspect the chosen tmux pane without confusing it with the outer con pane
4. tmux_ensure_shell_target when you need a safe shell pane for work
5. tmux_ensure_agent_target when you want to reuse or create a Codex CLI, Claude Code, or OpenCode tmux target
6. tmux_run_command when you need a fresh shell, a dedicated work window, or another long-running target
7. tmux_send_keys to a specific tmux pane target

Use outer-pane send_keys for tmux only as a fallback when tmux native control is unavailable.
tmux intercepts its prefix key from the PTY stream. Sending \\x02 (Ctrl-B) via send_keys \
reaches tmux, NOT a con application shortcut, but it is still a lower-fidelity path than tmux-native control.

The default tmux prefix key is Ctrl-B (\\x02). Some users remap it to Ctrl-A (\\x01) — \
if the default does not work, try \\x01 and read_pane to check.

### Step 0: Always read_pane FIRST
Before any tmux operation, read_pane to see:
- The tmux status bar (usually the last line) — shows session name, window list (* = active), hostname
- The main content area — what application is currently visible
Parse the status bar to know which window you are on and what other windows exist.

### Native tmux workflow
- Resolve best pane first: resolve_work_target(intent=\"tmux_shell\") or resolve_work_target(intent=\"agent_cli\", agent_name=\"codex\")
- Discover targets: tmux_list_targets(pane_index=...)
- Find a likely shell/agent pane: tmux_find_targets(pane_index=..., kind=\"shell\")
- Inspect a target: tmux_capture_pane(pane_index=..., target=\"%17\")
- Reuse or create a shell target: tmux_ensure_shell_target(pane_index=..., cwd=\"/repo\")
- Reuse or create an agent target: tmux_ensure_agent_target(pane_index=..., agent_name=\"codex\", cwd=\"/repo\")
- Launch a fresh target: tmux_run_command(pane_index=..., location=\"new_window\", command=\"bash\", window_name=\"scratch\")
- Act on a target: tmux_send_keys(pane_index=..., target=\"%17\", literal_text=\"htop\", append_enter=true)

### Fallback navigation (only when tmux native control is unavailable)
- Switch to window N: send_keys \"\\x020\" (prefix + 0, one call) — NOT two separate calls
- Next window: send_keys \"\\x02n\"
- Previous window: send_keys \"\\x02p\"
- List windows interactively: send_keys \"\\x02w\" then read_pane
- Create new window (with shell): send_keys \"\\x02c\"
- Switch tmux pane: send_keys \"\\x02o\" (next pane) or \"\\x02;\" (last active pane)
- Split horizontal: send_keys \"\\x02%\"
- Split vertical: send_keys \"\\x02\\\"\"

### Writing a file on a remote host via tmux
When you need to create or write a file on the remote machine:
1. read_pane to see current tmux state
2. Prefer resolve_work_target(intent=\"tmux_shell\") to choose the right tmux workspace first, then tmux_ensure_shell_target to reuse or create a shell pane. If native control is unavailable, navigate to a shell pane by status bar or create one with send_keys \"\\x02c\"
3. read_pane to verify you have a shell prompt (look for $, %, #, or similar)
4. If native control is available, send the heredoc through tmux_send_keys to the target shell pane. Otherwise use send_keys in the outer pane. Write the file using heredoc:
   send_keys \"cat > path/to/file << 'CONEOF'\\n\"
   send_keys \"file content line 1\\nline 2\\nline 3\\n\"
   send_keys \"CONEOF\\n\"
5. read_pane to verify the file was written (check for shell prompt return)
6. Optionally verify: send_keys \"cat path/to/file\\n\" then read_pane

### Running commands when a TUI is currently visible
1. read_pane to confirm current state
2. Prefer resolve_work_target(intent=\"tmux_shell\") to choose the right tmux workspace, then tmux_ensure_shell_target to identify or create a shell pane/window, then tmux_capture_pane to confirm it. \
If native control is unavailable, switch to another tmux window with a shell prompt, \
or create a new window with send_keys \"\\x02c\"
3. read_pane to verify you reached a shell prompt
4. Prefer tmux_send_keys to the specific shell pane target. Use outer-pane send_keys only when native control is unavailable
5. read_pane to see the output
6. Optionally switch back to the original window when done
";

/// Fallback guidance for unknown TUI applications.
pub const GENERAL_TUI: &str = "\
## General TUI interaction

When the visible target is an interactive app without a specific playbook:
1. read_pane FIRST to understand the current screen layout and any visible keybinding hints
2. Common keys: Enter (\\n), Escape (\\x1b), Tab (\\t)
3. Arrow keys: Up (\\x1b[A), Down (\\x1b[B), Right (\\x1b[C), Left (\\x1b[D)
4. Ctrl-C (\\x03) to interrupt or cancel
5. q or Q to quit many TUI apps
6. read_pane after each action to observe what changed
7. If stuck, try Escape (\\x1b) or Ctrl-C (\\x03) to return to a known state
";

/// Remote work strategy — emitted when the focused pane has a remote host.
pub const REMOTE_WORK: &str = "\
## Working on a remote host

The focused pane is connected to a remote host. Remember:
- file_read, file_write, edit_file, list_files, search are LOCAL-ONLY — they CANNOT access this remote host.
- To run remote commands: if the pane has `exec_visible_shell`, use terminal_exec. \
Otherwise, send_keys \"command\\n\" in a remote shell prompt.
- To read remote files: send_keys \"cat file\\n\" then read_pane.
- To write remote files: use heredoc via send_keys, or operate an open editor with send_keys.
- shell_exec runs on the LOCAL machine — never use it for remote work.
- The cwd shown in shell metadata may be LOCAL — it does NOT reflect the remote working directory.
";
