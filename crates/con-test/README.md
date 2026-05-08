# con-test

Logic test framework for `con-cli` — launches a real con process, resets state between
test files, and drives the session via plain-text `.test` files.

Inspired by [sqllogictest](https://www.sqlite.org/sqllogictest/doc/trunk/about.wiki).

See [`docs/impl/con-test.md`](../../docs/impl/con-test.md) for the full implementation
guide.

## Quick start

```bash
# Build everything
cargo build -p con -p con-cli -p con-test

# Run all tests (con is launched automatically)
./target/debug/con-test crates/con-test/testdata/

# Run a single file
./target/debug/con-test crates/con-test/testdata/tabs/basic.test

# Baseline mode — write actual output as new expected values
./target/debug/con-test --rewrite crates/con-test/testdata/

# Stop on first failure
./target/debug/con-test --fail-fast crates/con-test/testdata/

# Verbose — show pass/skip per step
./target/debug/con-test --verbose crates/con-test/testdata/
```

## .test file format

```
# comment

con-cli <arguments...>       # preferred
# Legacy: old tests may still use `cmd ...`
---- <mode>
expected output here
```

### Match modes

| Mode          | Behaviour |
|---------------|-----------|
| `contains`    | actual output contains the expected string (default) |
| `exact`       | full string equality (trailing whitespace trimmed per line) |
| `json-subset` | every key/value in expected JSON exists in actual JSON |
| `ok`          | only checks exit code == 0; expected block ignored |
| `error`       | checks exit code != 0; expected matched against stderr |

### Example

```
# tabs/basic.test

con-cli --json tabs list  # at least one tab exists
---- json-subset
{"tabs":[{"index":1}]}

con-cli tabs new
---- ok

con-cli --json tabs list
---- contains
"index":2

con-cli --json tabs close --tab 2
---- ok
```

## CLI reference

```
con-test [OPTIONS] <PATHS>...

Arguments:
  <PATHS>...  .test files or directories (searched recursively)

Options:
  --con <PATH>              Path to con app binary (default: target/ sibling, then PATH)
                            Env override: CON
                            Binary name: con on Unix, con-app on Windows
  --con-cli <PATH>          Path to con-cli binary (default: target/ sibling, then PATH)
                            Env override: CON_CLI
  --socket <PATH>           Control endpoint for the launched con process
                            (default: temp Unix socket on Unix, named pipe on Windows)
  --startup-timeout <SECS> Seconds to wait for con to start (default: 30)
  --rewrite                 Rewrite expected blocks from actual output (baseline mode)
  --fail-fast               Stop after the first failing file
  --verbose                 Show pass/skip results per step
```

## State isolation

Before each `.test` file, con-test resets con to a known baseline:
- All tabs except tab 1 are closed
- All panes in tab 1 except the first are closed (via Ctrl-D)
- All pane-local surfaces in the surviving pane except the first are closed

Every test file starts with exactly 1 tab, 1 pane, and 1 surface.
