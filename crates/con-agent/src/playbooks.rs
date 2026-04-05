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

/// tmux navigation and control via send_keys.
pub const TMUX_PLAYBOOK: &str = "\
## tmux interaction via send_keys

The default tmux prefix key is Ctrl-B (\\x02). Some users remap it to Ctrl-A (\\x01) — \
if the default does not work, try \\x01 and read_pane to check.

### Step 0: Always read_pane FIRST
Before any tmux operation, read_pane to see:
- The tmux status bar (usually the last line) — shows session name, window list (* = active), hostname
- The main content area — what application is currently visible
Parse the status bar to know which window you are on and what other windows exist.

### Navigation (batch prefix + command in one send_keys call)
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
2. Navigate to a shell pane: look at the tmux status bar for a window with a shell, or create one with send_keys \"\\x02c\"
3. read_pane to verify you have a shell prompt (look for $, %, #, or similar)
4. Write the file using heredoc:
   send_keys \"cat > path/to/file << 'CONEOF'\\n\"
   send_keys \"file content line 1\\nline 2\\nline 3\\n\"
   send_keys \"CONEOF\\n\"
5. read_pane to verify the file was written (check for shell prompt return)
6. Optionally verify: send_keys \"cat path/to/file\\n\" then read_pane

### Running commands when a TUI (nvim, htop, etc.) is currently visible
1. read_pane to confirm current state (e.g., nvim is showing)
2. Navigate to a shell: switch to another tmux window with a shell prompt, \
or create a new window with send_keys \"\\x02c\"
3. read_pane to verify you reached a shell prompt
4. send_keys \"your_command\\n\" to execute
5. read_pane to see the output
6. Optionally switch back to the original window when done
";

/// vim/nvim editing via send_keys.
pub const VIM_PLAYBOOK: &str = "\
## vim/nvim interaction via send_keys

### Step 0: Always read_pane FIRST
Before any vim operation, read_pane to understand:
- Is vim actually visible right now? (if not, navigate to it first)
- What mode is vim in? (check the bottom status line)
- What file is open? (shown in the title bar or status line)

### Detecting mode
- If the bottom shows \"-- INSERT --\": vim is in insert mode
- If the bottom shows \":\" followed by text: vim is in command-line mode
- If no mode indicator: vim is in normal mode

### Always start from normal mode
Send \\x1b (Escape) first to ensure you are in normal mode before any operation.

### Writing content to the current buffer
To replace entire file content (turn-efficient approach):
1. send_keys \"\\x1b\" (ensure normal mode) — read_pane to verify
2. send_keys \"ggdG\" (go to top, delete everything) — read_pane to verify empty
3. send_keys \"i\" (enter insert mode) then immediately send content in the SAME call:
   send_keys \"i#!/bin/bash\\nline2\\nline3\\n...entire content...\"
   You can send up to ~50 lines in one send_keys call.
4. read_pane to verify content was entered correctly
5. send_keys \"\\x1b:w\\n\" (escape + save in one call)
6. read_pane to verify save succeeded (look for \"written\" message at bottom)

This uses ~6 tool calls instead of 10+. Batch keystrokes when they don't require verification between them.

To append content at the end:
1. send_keys \"\\x1bGo\" (normal mode, go to last line, open new line below in insert mode)
2. send_keys the content
3. send_keys \"\\x1b:w\\n\" (normal mode, save)

### Saving and quitting
- Save: send_keys \"\\x1b:w\\n\"
- Save and quit: send_keys \"\\x1b:wq\\n\"
- Quit without saving: send_keys \"\\x1b:q!\\n\"
- Open a different file: send_keys \"\\x1b:e path/to/file\\n\"

### Large content (more than 10 lines)
Send content in chunks of 5-10 lines. After each chunk:
1. read_pane to verify the chunk was received correctly
2. If corruption is detected, undo with send_keys \"\\x1bu\" and retry the chunk
3. Continue with the next chunk

### When vim is inside tmux
If you need to navigate away from vim to a tmux shell, use the tmux prefix (\\x02) — \
vim will ignore Ctrl-B in normal mode, and tmux will intercept it.
To return to the vim window, use tmux window switching (\\x02 then window number).
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
- To read remote files: use read_pane to see editor content, or navigate to a remote shell and send_keys \"cat file\\n\".
- To write remote files: use send_keys to operate the remote editor, or use heredoc in a remote shell.
- To run remote commands: use send_keys in a remote shell pane, NOT shell_exec (which is local).
- The cwd shown in context may be LOCAL — it does NOT reflect the remote working directory.
";
