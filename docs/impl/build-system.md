# Implementation: Build System

## Overview

con is a pure Rust workspace compiled with Cargo. No external build tools are required beyond `rustc` (stable, edition 2024) and `cmake` (for GPUI shader compilation).

## Build Commands

```bash
cargo build                    # debug build
cargo build --release          # release build
cargo run -p con               # run the terminal
cargo test --workspace         # run all tests
```

## Workspace Structure

```toml
[workspace]
members = [
    "crates/con",           # main binary (GPUI app shell)
    "crates/con-core",      # shared logic (harness, config, session)
    "crates/con-terminal",  # terminal emulation (grid, pty, input)
    "crates/con-agent",     # AI agent harness (Rig 0.34, tools)
    "crates/con-cli",       # CLI client (stub)
]
resolver = "3"
```

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| gpui | mainline (git) | GPU-accelerated UI framework (from zed-industries/zed) |
| gpui-component | git | shadcn/ui-style component library |
| rig-core | 0.34 | Multi-provider AI agent framework |
| portable-pty | 0.8 | Cross-platform PTY management |
| vte | 0.15 | Pure Rust VT100/xterm parser |
| crossbeam-channel | 0.5 | Lock-free channels for event passing |
| tokio | 1.x | Async runtime for agent tasks |

## GPUI Shaders

GPUI compiles Metal shaders at runtime via the `runtime_shaders` feature flag. This means:
- No Xcode.app installation required for development
- No pre-compilation step for shaders
- cmake is needed for the shader compilation toolchain

## Platform Requirements

| Platform | Requirements |
|----------|-------------|
| macOS | Rust stable, cmake |
| Linux | Rust stable, cmake, libwayland-dev, libxkbcommon-dev |
| Windows | Rust stable, cmake |

## Dev Workflow

```bash
cargo watch -x 'run -p con'    # auto-rebuild on file changes
cargo nextest run               # faster parallel test runner
```
