---
name: con-cli-e2e
description: Validate Con's local socket control plane against a real running app session. Use when testing con-cli, the Unix socket API, pane control, tmux control, or in-session agent calls for automation and evaluation.
---

# con-cli E2E

Use this skill when the task is to verify that Con's CLI/control plane works against a live app window, not just that the code compiles.

Primary reference:

- Read [`docs/impl/con-cli-e2e.md`](../../docs/impl/con-cli-e2e.md) for the full workflow and current live limitations.

Default workflow:

1. Build the relevant crates.
2. Launch `cargo run -p con`.
3. Wait for `/tmp/con.sock`.
4. Use `con-cli --json identify`, `tabs list`, and `panes list` before acting.
5. Only use `panes exec` on panes that expose `exec_visible_shell`.
6. Use `agent ask` to verify the real in-tab built-in agent session.

Rules:

- Prefer `--json` for every command in automated evaluation.
- Prefer `pane_id` over `pane_index` for follow-up actions.
- After visible execution, confirm the pane still reports `shell_prompt` and keeps `exec_visible_shell`.
- If `agent ask` fails, check provider config/env before blaming the socket layer.

Known current limit:

- `panes create` returns promptly, but the new pane may not yet be ready for immediate shell control. Treat it as provisional until `panes list` shows `is_alive: true` and normal shell capabilities.
