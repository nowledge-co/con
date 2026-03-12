# con — Development Guide

## What is con?

con is an open-source, cross-platform, GPU-accelerated terminal emulator with a built-in AI agent harness. Built in Rust.

## Stack

- **UI**: GPUI-CE v0.3.3 (community edition of Zed's framework, Apache 2.0)
- **Terminal emulation**: vte v0.15 (pure Rust VT parser) + portable-pty
- **AI agent**: Rig v0.32.0 (from crates.io, 13 providers, Tool trait)
- **PTY**: portable-pty crate (cross-platform)
- **Socket API**: planned — Unix domain sockets, JSON-RPC (cmux-inspired)

## Repository Layout

```
kingston/
├── DESIGN.md          # Architecture and design decisions
├── CLAUDE.md          # This file — development guide
├── docs/
│   ├── impl/          # Implementation notes per crate/subsystem
│   └── study/         # Research notes on 3pp dependencies
├── postmortem/        # Issue postmortems (YYYY-MM-DD-title.md)
├── crates/
│   ├── con/           # Main binary (GPUI app shell)
│   ├── con-core/      # Shared logic (harness, config, session)
│   ├── con-terminal/  # Terminal emulation (grid, pty, input encoding)
│   ├── con-agent/     # AI harness (Rig 0.32, tools, conversation)
│   └── con-cli/       # CLI + socket client (stub)
├── postmortem/        # Integration & incident postmortems
└── assets/            # Themes, fonts, icons
```

## Build

```bash
# Prerequisites: rust (stable, edition 2024), cmake
cargo build            # debug
cargo build --release  # release
cargo run -p con       # run the terminal
cargo test --workspace # test

# GPUI needs runtime_shaders feature (already set) — no Xcode.app needed for dev
```

## Key Conventions

- **Crate boundaries matter.** con-terminal has zero UI deps. con-agent has zero terminal deps. con-core glues them.
- **Real Rig integration.** Tools implement `rig::tool::Tool` trait. Agent built via `client.agent(model).tool(T).build()`. Chat via `Chat::chat()` trait.
- **Agent transparency.** When the built-in agent runs a command, it executes visibly. No hidden subprocesses.
- **Shared tokio runtime.** The harness owns a single multi-thread tokio runtime — no thread-per-message.
- **Config is TOML.** User config at `~/.config/con/config.toml`.
- **GPUI patterns.** Use `cx.spawn(async move |this, cx| { ... })` for async work. Use if/else for conditional UI (FluentBuilder::when() is not re-exported).

## Branching

- `main` — stable
- Feature branches: `wey-gu/<short-name>`

## Postmortems

When solving a non-trivial bug or issue, create `postmortem/YYYY-MM-DD-title.md` with:
- What happened
- Root cause
- Fix applied
- What we learned
