# create_pane startup command silently dropped

**Date**: 2026-04-06

## What happened

Agent called `create_pane` with `command: "ssh cinnamon"`. The tool returned `{"pane_index":2,"command":"ssh cinnamon"}` — success. But `read_pane` showed a local zsh prompt, not an SSH session. The agent then ran `apt update` via `send_keys`, which failed with `zsh: command not found: apt` because it was still on macOS, not the remote host.

The startup command was silently discarded. No error, no warning.

## Root cause

`GhosttyView` initializes its terminal surface **lazily**. The lifecycle:

1. `create_terminal()` → `GhosttyView::new()` sets `self.terminal = None`
2. `terminal.write("ssh cinnamon\n")` runs immediately in the same render frame
3. `TerminalPane::write()` does `if let Some(terminal) = self.entity.read(cx).terminal()` — **None** → silently drops the data
4. Later, `GhosttyView::on_layout()` → `ensure_initialized()` creates the ghostty surface and sets `self.terminal = Some(...)`
5. Shell starts, but the SSH command is gone

The `create_pane` handler runs at the **top** of `ConWorkspace::render()`, before child views have painted. The GhosttyView's `on_layout` (where surface creation happens) runs later in the same frame. So every write between creation and first paint is lost.

## Fix

Added a `pending_write: Option<Vec<u8>>` buffer to `GhosttyView`:

- `write_or_queue(&mut self, data: &[u8])` — writes to PTY if terminal exists, otherwise buffers
- `ensure_initialized()` flushes `pending_write` immediately after surface creation
- `TerminalPane::write()` now calls `write_or_queue()` instead of direct `write_to_pty()`

The command travels with the view and fires exactly once, at the right time.

Additionally, the `ensure_initialized()` error branch was hardened: if `new_surface()` fails, `pending_write` is cleared (no PTY to receive it) and `initialized` is set to `true` to prevent infinite retry on every layout cycle. Without this, a failed surface creation would cause the queued data to accumulate and the initialization attempt to repeat on every frame.

## What we learned

1. **Silent drops are dangerous.** `if let Some(x) = maybe { x.do_thing() }` with no `else` branch is a silent failure pattern. In infrastructure code (PTY writes), this should at minimum `log::warn!`.

2. **Lazy initialization + immediate use = race.** When an object is created but initialized lazily (on first paint, on first IO, etc.), any method called between creation and initialization must either queue or fail loudly. The "Option + silent skip" pattern makes this invisible.

3. **Test the full lifecycle, not just the happy path.** The `create_pane` tool returned success with the correct `pane_index` — the tool-level contract was met. The failure was in the plumbing between "tool responds" and "command actually executes." Integration tests that verify the command ran (checking terminal content after creation) would have caught this.
