---
name: con-cli-e2e
description: Validate Con's local socket control plane against a real running app session, and write/run con-test integration tests. Use when testing con-cli, the Unix socket API, pane control, tmux control, in-session agent calls, or when writing E2E test cases in crates/con-test/testdata/.
---

# con-cli E2E & con-test

Use this skill when the task is to verify that Con's CLI/control plane works against a live app window, or when writing/fixing integration tests in `crates/con-test/testdata/`.

Primary reference:

- Read [`docs/impl/con-cli-e2e.md`](../../docs/impl/con-cli-e2e.md) for the full workflow and current live limitations.

---

## con-cli manual E2E

Default workflow:

1. Build the relevant crates.
2. Launch `cargo run -p con`.
3. Wait for `/tmp/con.sock`.
4. Use `con-cli --json identify`, `tabs list`, and `panes list` before acting.
5. Only use `panes exec` on panes that expose `exec_visible_shell`.
6. Use `tree` / `surfaces list` only for pane-local surface validation.
7. After `surfaces create` or `surfaces split`, use `surfaces wait-ready --surface-id <id> --timeout 10` before sending input that assumes an initialized shell.
8. Use `agent ask` to verify the real in-tab built-in agent session.

Rules:

- Prefer `--json` for every command in automated evaluation.
- Prefer `pane_id` over `pane_index` for follow-up actions.
- Prefer `surface_id` for follow-up actions only when testing the explicit `surfaces.*` API.
- Keep existing pane and agent benchmarks on `panes.*`; surfaces are additive and must not change the built-in agent's pane model.
- After visible execution, confirm the pane still reports `shell_prompt` and keeps `exec_visible_shell`.
- If `agent ask` fails, check provider config/env before blaming the socket layer.

Known current limit:

- `panes create` now reports `surface_ready`, `is_alive`, and `has_shell_integration`, but startup-command panes can still be in a non-shell foreground state immediately after creation. Treat them as provisional until `panes list` confirms the capabilities you need for the next step.

---

## con-test integration tests

`con-test` is the E2E test runner for integration and interactive behavior. It launches a real con session, runs `.test` files against it via `con-cli`, and checks output.

### Running tests

```bash
# Build first
cargo build

# Run all tests
./target/debug/con-test crates/con-test/testdata/

# Run a single file
./target/debug/con-test crates/con-test/testdata/panes/split.test
```

### Test file format

Test files live in `crates/con-test/testdata/<group>/<name>.test`. Each step is a `con-cli` command followed by an assertion block:

```
# comment
con-cli --json <command>   # step description
---- <assertion>
<expected>
```

Assertion types:

| Assertion | Meaning |
|---|---|
| `---- ok` | Command exits 0 (any output accepted) |
| `---- contains` | stdout contains the literal string on the next line |
| `---- json-subset` | actual JSON is a superset of the expected JSON (subset match, deep) |

The `json-subset` assertion only checks the keys you specify — extra fields in the actual output are ignored. Use it to assert specific fields without coupling to the full response shape.

### Writing new tests

- **Unit-test functions** in Rust (`#[cfg(test)]`) for logic. Use `con-test` only for integration and interactive behavior that requires a live session.
- **No low-value tests** — don't write tests just to hit coverage. Every test should catch a real bug or document a real contract.
- Group tests by domain: `panes/`, `tabs/`, `agent/`, `system/`.
- After `panes create`, always add a `panes wait` step before asserting `is_alive` — the new pane's surface may not be ready immediately.
- For agent panel tests, use `agent open-panel-for-request` to drive the motion state, then `agent panel-state` to assert the result.

### Example test

```
# panes/split.test
con-cli --json panes list --tab 1  # start with 1 pane
---- json-subset
{"panes":[{"index":1}]}

con-cli --json panes create --tab 1 --location right  # split right
---- ok

con-cli --json panes wait --tab 1 --pane-index 2 --timeout 5  # wait for new pane
---- ok

con-cli --json panes list --tab 1  # both panes alive
---- json-subset
{"panes":[{"is_alive":true},{"is_alive":true}]}
```

### Fixing failures

When a test fails with `expected JSON is not a subset of actual JSON`, the actual output is printed in full. Check:

1. Is the command missing (unrecognized subcommand)? → Add the subcommand to `con-cli` and the `ControlCommand` handler.
2. Is a field wrong/missing? → Fix the handler's response shape.
3. Is the assertion racing (e.g. `is_alive: false` right after create)? → Add a `panes wait` or `surfaces wait-ready` step before the assertion.
