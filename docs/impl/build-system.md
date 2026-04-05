# Implementation: Build System

## Overview

con is a Cargo workspace with one terminal runtime target: embedded Ghostty on macOS.

The build no longer includes the old `vte` and `portable-pty` pipeline.

## Build commands

```bash
cargo build
cargo build --release
cargo run -p con
cargo test --workspace
```

## Workspace shape

```toml
[workspace]
members = [
    "crates/con",
    "crates/con-core",
    "crates/con-terminal",
    "crates/con-ghostty",
    "crates/con-agent",
    "crates/con-cli",
]
```

## Key crates

| Crate | Purpose |
|-------|---------|
| `con` | GPUI app shell, tabs, splits, settings, agent panel |
| `con-core` | harness, config, session persistence |
| `con-ghostty` | Rust wrapper around libghostty C API |
| `con-terminal` | terminal theme data and Ghostty palette translation helpers |
| `con-agent` | built-in AI harness and tools |

## Key dependencies

| Dependency | Purpose |
|------------|---------|
| `gpui` | native GPU UI framework |
| `gpui-component` | reusable UI controls |
| `rig-core` | multi-provider agent runtime |
| `tokio` | async runtime for agent work |
| `crossbeam-channel` | UI and harness event routing |
| `reqwest` | live model list fetch from models.dev |

## Platform boundary

con currently requires macOS because the product depends on embedded Ghostty.

That is enforced in the binary crate with a compile-time error on non-macOS targets.

## Ghostty build boundary

`con-ghostty` is intentionally thin:

- FFI bindings live in `ffi.rs`
- surface/app lifecycle lives in `terminal.rs`
- product logic stays out of the wrapper

If we need stronger pane observability in the future, the preferred path is to upstream or expose new libghostty C API surface area instead of growing another terminal runtime in this repo.
