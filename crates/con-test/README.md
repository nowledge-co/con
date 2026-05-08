# con-test

Logic test framework for `con-cli` — drives a live con session via `.test` files.

Inspired by [sqllogictest](https://www.sqlite.org/sqllogictest/doc/trunk/about.wiki): test cases
are plain text files that inline both the command and the expected output. No separate result
files, no test harness boilerplate.

## Quick start

```bash
# 1. Build con-cli and con-test
cargo build -p con-cli -p con-test

# 2. Launch con (in another terminal or background)
cargo run -p con &

# 3. Run all tests against the live session
./target/debug/con-test crates/con-test/testdata/

# 4. Run a single file
./target/debug/con-test crates/con-test/testdata/system/identify.test

# 5. Baseline mode — write actual output into the .test files as new expected values
./target/debug/con-test --rewrite crates/con-test/testdata/
```

## .test file format

```
# Lines starting with # are comments and are ignored.

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
| `regex`       | expected is a regex pattern (not yet implemented) |

### Example

```
# system/identify.test

cmd --json identify  # version field present
match json-subset
----
{"version":"0.1.0"}

cmd --json capabilities
match contains
----
"system.identify"

cmd tabs new
match ok
----
```

## CLI flags

```
con-test [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  .test files or directories containing .test files

Options:
  --con-cli <PATH>   Path to con-cli binary (default: sibling in target/, then PATH)
  --socket <PATH>    Override con socket path (passed as --socket to con-cli)
  --rewrite          Rewrite expected blocks from actual output (baseline mode)
  --fail-fast        Stop after the first failing file
  --verbose          Show pass/skip results in addition to failures
```

## Adding tests

1. Create a `.test` file under `testdata/` in the appropriate subdirectory.
2. Write `cmd` / `match` / `----` / expected blocks.
3. If unsure about the exact output, use `--rewrite` to generate the baseline, then review the diff.

## Preconditions

- A running con session (debug build listens on `/tmp/con-debug.sock`, release on `/tmp/con.sock`).
- `con-cli` built and reachable (sibling binary in `target/debug/` when run from the workspace).

## Running in CI

```yaml
- name: Build
  run: cargo build -p con -p con-cli -p con-test

- name: Launch con
  run: ./target/debug/con &
  env:
    DISPLAY: :99   # Linux headless

- name: Wait for socket
  run: timeout 15 bash -c 'until test -S /tmp/con-debug.sock; do sleep 0.5; done'

- name: Run con-test
  run: ./target/debug/con-test crates/con-test/testdata/
```
