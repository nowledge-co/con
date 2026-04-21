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
- **macOS**: Zig 0.13+ (for `libghostty`)
- **Windows**: Zig 0.13+ (for `libghostty-vt`), Visual Studio 2022 Build Tools with the Windows 10/11 SDK. Run the build from a _Developer Command Prompt for VS 2022_ so `rc.exe` is on `PATH`.

## Build

```bash
# macOS / Linux
cargo build

# Windows — must use the `w*` aliases. The default `con` binary cannot
# exist on Windows because `CON` is a reserved DOS device name, so the
# Windows build ships as `con-app.exe` via a feature-gated alias bin
# target. `cargo wbuild` is `cargo build --no-default-features
# --features con/bin-con-app`; `wrun`, `wcheck`, `wtest` mirror it.
cargo wbuild -p con --release          # → target\release\con-app.exe
```

## Run

```bash
# macOS / Linux
cargo run -p con

# Windows
cargo wrun -p con

> With optional arguments:

```bash
RUST_LOG=con_agent::flow=info,con_agent=warn,con_core=warn,con::suggestions=debug,con_core::suggestions=debug cargo run -p con
```

## Test

```bash
cargo test --workspace            # macOS / Linux
cargo wtest -p con-core -p con-cli -p con-agent -p con-terminal   # Windows (portable crates only)
```

## Release Build

```bash
cargo build --release -p con                  # macOS / Linux
cargo wbuild -p con --release                 # Windows → target\release\con-app.exe
```

For signed macOS release artifacts, use:

```bash
./scripts/macos/release.sh
```

## Useful Paths

- `crates/con-app/src` — app shell and GPUI surfaces (the crate directory is `con-app` because `con` is a reserved DOS device name on Windows; the Cargo package and binary are still named `con` on macOS, so `cargo run -p con` works as before — see `docs/impl/windows-port.md`)
- `crates/con-core/src` — shared app logic
- `crates/con-agent/src` — agent provider, hooks, tools, skills
- `docs/design` — design handoff set
- `docs/impl` — implementation notes
- `postmortem` — issue writeups and lessons learned
