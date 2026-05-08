# con-test

Logic test framework for `con-cli` — launches a real con process, resets state between
test files, and drives the session via plain-text `.test` files.

Inspired by [sqllogictest](https://www.sqlite.org/sqllogictest/doc/trunk/about.wiki): test
cases inline both the command and the expected output in a single file. No separate result
files, no test harness boilerplate.

## How it works

```
con-test
  │
  ├── launches one con process  (unique socket per run)
  │
  ├── for each .test file:
  │     ├── reset_state()       (close all tabs except tab 1)
  │     └── run each cmd step   (via con-cli --socket ...)
  │
  └── kills con process on exit (RAII — even on panic or Ctrl-C)
```

Each test file starts from a known baseline: exactly one tab, one pane, fresh shell.

## Quick start

```bash
# 1. Build everything
cargo build -p con -p con-cli -p con-test

# 2. Run all tests (con is launched automatically)
./target/debug/con-test crates/con-test/testdata/

# 3. Run a single file
./target/debug/con-test crates/con-test/testdata/tabs/basic.test

# 4. Baseline mode — write actual output into .test files as new expected values
./target/debug/con-test --rewrite crates/con-test/testdata/

# 5. Stop on first failure
./target/debug/con-test --fail-fast crates/con-test/testdata/

# 6. Verbose — show pass/skip per step
./target/debug/con-test --verbose crates/con-test/testdata/
```

## .test file format

```
# Lines starting with # are comments.

cmd <con-cli arguments...>   # optional inline label
match <mode>                 # optional, default: contains
----
expected output here
```

A blank line or the next `cmd` ends the expected block.

### Match modes

| Mode          | Behaviour |
|---------------|-----------|
| `contains`    | actual output contains the expected string (default) |
| `exact`       | full string equality (trailing whitespace trimmed per line) |
| `json-subset` | every key/value in expected JSON exists in actual JSON (ignores extra fields) |
| `ok`          | only checks exit code == 0; expected block is ignored |
| `error`       | checks exit code != 0; expected block matched against stderr |
| `regex`       | not yet implemented |

### Example

```
# tabs/basic.test

cmd --json tabs list  # at least one tab exists
match json-subset
----
{"tabs":[{"index":1}]}

cmd tabs new  # create a new tab
match ok
----

cmd --json tabs list  # now have 2 tabs
match contains
----
"index":2

cmd --json tabs close --tab 2
match ok
----
```

## CLI reference

```
con-test [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  .test files or directories (searched recursively)

Options:
  --con <PATH>              Path to con binary     (default: target/ sibling, then PATH)
  --con-cli <PATH>          Path to con-cli binary (default: target/ sibling, then PATH)
  --socket <PATH>           Socket path for the launched con process
                            (default: /tmp/con-test-<pid>.sock)
  --startup-timeout <SECS> Seconds to wait for con to start (default: 30)
  --rewrite                 Rewrite expected blocks from actual output (baseline mode)
  --fail-fast               Stop after the first failing file
  --verbose                 Show pass/skip results per step
```

Binary resolution order (for both `--con` and `--con-cli`):

1. Explicit flag
2. `CON` / `CON_CLI` environment variable
3. Sibling binary in the same `target/` directory as `con-test`
4. `PATH`

## Adding tests

1. Create a `.test` file under `testdata/` in the appropriate subdirectory.
2. Write `cmd` / `match` / `----` / expected blocks.
3. If unsure about exact output, run with `--rewrite` to generate the baseline,
   then `git diff` to review.

## State isolation

Before each `.test` file, con-test calls `reset_state()` which closes all tabs
except tab 1. This means:

- Every test file starts with exactly 1 tab and 1 pane.
- Tests that create tabs/panes do not need to clean up after themselves.
- Tab index 1 is always the baseline tab at the start of each file.

## Running in CI

```yaml
- name: Build
  run: cargo build -p con -p con-cli -p con-test

- name: Run con-test
  run: ./target/debug/con-test crates/con-test/testdata/
  # con is launched automatically by con-test.
  # On macOS CI runners a display is available so the con window appears briefly.
  # Set CON_LOG_FILE to capture con logs if a test fails.
  env:
    CON_LOG_FILE: /tmp/con-test.log
    RUST_LOG: error
```
