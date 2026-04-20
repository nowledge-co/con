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
    "crates/con-app",
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
| `gpui` | native GPU UI framework (upstream Zed git source) |
| `gpui-component` | reusable UI controls (upstream Longbridge git source) |
| `rig-core` | multi-provider agent runtime (pinned fork revision) |
| `tokio` | async runtime for agent work |
| `crossbeam-channel` | UI and harness event routing |
| `reqwest` | live model list fetch from models.dev |

## Dependency sourcing

`con` does not build against live sources in `3pp/`.

- `3pp/` is read-only reference material only.
- Cargo dependencies resolve from crates.io or explicit git sources in the workspace manifest.
- Ghostty source is fetched by `con-ghostty/build.rs` when needed, unless an override source directory is provided for local development.

## Platform boundary

con currently requires macOS because the product depends on embedded Ghostty.

That is enforced in the binary crate with a compile-time error on non-macOS targets.

## Ghostty build boundary

`con-ghostty` is intentionally thin:

- FFI bindings live in `ffi.rs`
- surface/app lifecycle lives in `terminal.rs`
- product logic stays out of the wrapper

## Ghostty resources

Ghostty's runtime is not just the static library. The child shell environment also depends on the bundled Ghostty resources payload, especially:

- `terminfo/xterm-ghostty`
- shell integration scripts
- supporting share files under `Resources/ghostty`

Con now handles this in two places:

- `cargo run -p con` debug runs: `con-ghostty` seeds `GHOSTTY_RESOURCES_DIR` from the built Ghostty `zig-out/share/ghostty` directory when that directory exists locally.
- macOS app bundles: `scripts/macos/build-app.sh` copies Ghostty's built `share/ghostty` tree into `Contents/Resources/ghostty`.

Without that payload, Ghostty falls back to `TERM=xterm-256color` and disables parts of shell integration. That changes the behavior child processes see and can invalidate product comparisons against standalone Ghostty.

If we need stronger pane observability in the future, the preferred path is to upstream or expose new libghostty C API surface area instead of growing another terminal runtime in this repo.
