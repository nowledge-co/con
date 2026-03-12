# con — Development Guide

## What is con?

con is an open-source, cross-platform, GPU-accelerated terminal emulator with a built-in AI agent harness. Built in Rust.

## Stack

- **UI**: GPUI (gpui-ce, community edition of Zed's framework)
- **Terminal emulation**: libghostty-vt (Zig, linked via C FFI)
- **AI agent**: Rig (Rust agent framework, multi-provider)
- **PTY**: portable-pty crate
- **Socket API**: Unix domain sockets, JSON-RPC (cmux-inspired)

## Repository Layout

```
kingston/
├── DESIGN.md          # Architecture and design decisions
├── CLAUDE.md          # This file — development guide
├── docs/
│   ├── impl/          # Implementation notes per crate/subsystem
│   └── study/         # Research notes on 3pp dependencies
├── postmortem/        # Issue postmortems (YYYY-MM-DD-title.md)
├── crates/            # Rust workspace members (TODO: scaffold)
│   ├── con/           # Main binary (GPUI app)
│   ├── con-core/      # Shared logic
│   ├── con-terminal/  # Terminal emulation + PTY
│   ├── con-agent/     # AI harness (Rig)
│   └── con-cli/       # CLI + socket client
├── 3pp/               # Third-party sources (gitignored, submodules)
└── assets/            # Themes, fonts, icons
```

## Build

```bash
# Prerequisites: zig >= 0.15.2, rust (stable), cmake
cargo build            # debug
cargo build --release  # release
cargo run -p con       # run
cargo test --workspace # test
```

## Key Conventions

- **Crate boundaries matter.** con-terminal has zero UI deps. con-agent has zero terminal deps. con-core glues them.
- **FFI safety.** All ghostty C calls wrapped in safe Rust in con-terminal. No raw pointers leak upward.
- **Agent transparency.** When the built-in agent runs a command, it executes in a visible terminal pane. No hidden subprocesses.
- **Socket API first.** The built-in agent is a client of con's socket API. External agents (Claude Code, plugins) use the same API.
- **Config is TOML.** User config at `~/.config/con/config.toml`. Ghostty theme files are compatible.

## Branching

- `main` — stable
- Feature branches: `wey-gu/<short-name>`

## Postmortems

When solving a non-trivial bug or issue, create `postmortem/YYYY-MM-DD-title.md` with:
- What happened
- Root cause
- Fix applied
- What we learned
