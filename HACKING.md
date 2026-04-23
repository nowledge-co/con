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
| `con-terminal` | Theme + palette helpers shared across backends |
| `con-ghostty` | Per-platform terminal backends: macOS embedded libghostty + Metal, Windows libghostty-vt + ConPTY + D3D11/DirectWrite, Linux Unix PTY + libghostty-vt + GPUI-owned `StyledText` paint |
| `con-agent` | Rig integration, tools, hooks, conversation, skills |
| `con-cli` | CLI + socket client for the live local control plane |

Each platform exposes the same `GhosttyApp` / `GhosttyTerminal` /
`TerminalColors` type names from `con-ghostty`, so the rest of the
workspace consumes the backend without per-call-site `cfg` gates.
See `docs/impl/{linux,windows}-port.md` for the per-platform plans
and the path to the long-term GPU-accelerated grid renderer.

## Agent Model

- Shared Tokio runtime per window
- Per-tab agent sessions
- Tool and model events flow to the UI over channels
- Rig `PromptHook` drives streaming, approvals, and lifecycle events

For the full breakdown, see `docs/impl/agent-harness.md`.

## Prerequisites

- Rust (stable, edition 2024)
- `cmake`
- **macOS**: Zig 0.15.2+ (ghostty's `build.zig.zon` pins `minimum_zig_version = "0.15.2"`)
- **Windows**: Zig 0.15.2+ (same reason), Visual Studio 2022 Build Tools with the Windows 10/11 SDK. Run the build from a _Developer Command Prompt for VS 2022_ so `rc.exe` is on `PATH`. If Windows Defender is on, either add an exclusion for the repo dir or disable real-time scanning — zig's sub-build exes get briefly locked by MpEngine and spawn with `FileNotFound`.
- **Linux**: Zig 0.15.2+ (con-ghostty builds `libghostty-vt` for the Linux backend the same way it does for Windows), plus the GPUI runtime apt deps the CI job already installs:
  ```sh
  sudo apt-get install -y --no-install-recommends \
    libxcb-composite0-dev libxcb-dri2-0-dev libxcb-glx0-dev \
    libxcb-present-dev libxcb-xfixes0-dev libxkbcommon-x11-dev \
    libwayland-dev libvulkan-dev libfreetype-dev libfontconfig1-dev \
    mesa-vulkan-drivers
  ```
  The `mesa-vulkan-drivers` line gives you a software ICD (llvmpipe) as a fallback for headless / VM environments; on a real desktop with a hardware GPU you can skip it.

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

For a Linux release tarball (un-signed; mirrors the Windows
preview's distribution shape), use:

```bash
CON_RELEASE_VERSION=0.1.0-beta.X CON_RELEASE_CHANNEL=beta \
  ./scripts/linux/release.sh
```

Output lands in `dist/con-<version>-linux-<arch>.tar.gz` with a
SHA256 sum next to it. The CI workflow at
`.github/workflows/release-linux.yml` runs the same script on every
`v*` tag, attaches the tarball to the shared GitHub release, and
updates the Sparkle-shaped appcast at
`https://con-releases.nowledge.co/appcast/<channel>-linux-x86_64.xml`
that the in-app notify-only updater polls.

## Useful Paths

- `crates/con-app/src` — app shell and GPUI surfaces (the crate directory is `con-app` because `con` is a reserved DOS device name on Windows; the Cargo package and binary are still named `con` on macOS, so `cargo run -p con` works as before — see `docs/impl/windows-port.md`)
- `crates/con-core/src` — shared app logic
- `crates/con-agent/src` — agent provider, hooks, tools, skills
- `docs/design` — design handoff set
- `docs/impl` — implementation notes
- `postmortem` — issue writeups and lessons learned
