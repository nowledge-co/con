# Hacking on con

Quick contributor map for `con`.

Read these first:
- `README.md` — public project overview
- `CLAUDE.md` — development conventions
- `DESIGN.md` — architecture and product direction
- `docs/README.md` — documentation index

## Workspace Map

Terminal crates do not depend on the UI. The agent crate does not depend on a specific terminal backend.

| Crate | Role |
|-------|------|
| `con` | GPUI shell: windows, tabs, panes, agent UI, settings, command surfaces |
| `con-core` | Config, sessions, shared app logic, harness wiring |
| `con-terminal` | Fallback terminal backend: `vte`, grid, PTY, input |
| `con-ghostty` | macOS Ghostty integration and FFI |
| `con-agent` | Rig integration, tools, hooks, conversation, skills |
| `con-cli` | CLI and future socket client |

macOS prefers `con-ghostty`; the fallback backend remains important for portability and testing.

## Agent Model

- Shared Tokio runtime per window
- Per-tab agent sessions
- Tool and model events flow to the UI over channels
- Rig `PromptHook` drives streaming, approvals, and lifecycle events

For the full breakdown, see `docs/impl/agent-harness.md`.

## Prerequisites

- Rust (stable, edition 2024)
- `cmake`

## Build

```bash
cargo build
```

## Run

```bash
cargo run -p con
```

## Test

```bash
cargo test --workspace
```

## Release Build

```bash
cargo build --release -p con
```

For signed macOS release artifacts, use:

```bash
./scripts/macos/release.sh
```

## Useful Paths

- `crates/con/src` — app shell and GPUI surfaces
- `crates/con-core/src` — shared app logic
- `crates/con-agent/src` — agent provider, hooks, tools, skills
- `docs/design` — design handoff set
- `docs/impl` — implementation notes
- `postmortem` — issue writeups and lessons learned
