# con — Development Guide

## What is con?

con is an open-source, macOS-native, GPU-accelerated terminal emulator with a built-in AI agent harness. Built in Rust.

A Windows port is in preparation — the non-UI crates already build on `x86_64-pc-windows-*` targets. The staged plan lives in
`docs/impl/windows-port.md` and the first preparation PR is summarized in `postmortem/2026-04-16-prepare-windows-port.md`.

## Stack

- **UI**: upstream Zed GPUI (git dependency on `zed-industries/zed`, Apache 2.0). Windows backend is D3D11/DirectComposition; HWND child-embedding is the known gap for the Windows port.
- **Terminal runtime**: libghostty — full Ghostty terminal via C API, Metal GPU rendering, embedded as native NSView. macOS-only today; see `docs/impl/windows-port.md` for the Windows strategy (likely `libghostty-vt` + ConPTY + custom renderer).
- **Terminal FFI**: con-ghostty crate — thin Rust wrapper over libghostty C API (surface lifecycle, action callbacks, clipboard, key/mouse input). Cfg-gated to macOS; on other targets the crate compiles to an empty shell so the workspace resolves.
- **Terminal support crate**: con-terminal — theme and palette helpers only
- **AI agent**: Rig v0.34.0 (from crates.io, 13 providers, Tool trait)
- **Socket API**: JSON-RPC 2.0 with a platform-specific transport — Unix domain sockets on Unix, Windows Named Pipes (`\\.\pipe\con`) on Windows. Served by the app and consumed first by `con-cli`.

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
│   ├── con-terminal/  # Terminal themes and palette helpers
│   ├── con-ghostty/   # Ghostty FFI wrapper — primary macOS backend (libghostty C API)
│   ├── con-agent/     # AI harness (Rig 0.34, tools, conversation)
│   └── con-cli/       # CLI + socket client for the live local control plane
├── postmortem/        # Integration & incident postmortems
├── assets/            # Themes, fonts, icons
└── 3pp/               # Third-party source (READ-ONLY reference, .gitignored)
```

## 3pp Policy

The `3pp/` directory contains third-party source checkouts for **read-only reference only**. It is `.gitignored` — never modify, commit, or depend on files in `3pp/`.

- All third-party dependencies come from **crates.io** (or git URLs in Cargo.toml).
- If a 3pp library has a bug, **upstream the fix** to the library's GitHub repo. Do not patch locally.
- `3pp/` exists solely so you can read and study how dependencies work internally.

## Build

```bash
# Prerequisites: rust (stable, edition 2024), cmake, zig (for libghostty on macOS)
cargo build            # debug (macOS)
cargo build --release  # release (macOS)
cargo run -p con       # run the terminal (macOS)
cargo test --workspace # test

# GPUI needs runtime_shaders feature (already set)
# The `con` UI binary is currently macOS-only — a compile_error fires on
# other targets. Check the portable crates on Linux or Windows with:
cargo check -p con-core -p con-cli -p con-agent -p con-terminal -p con-ghostty
# (con-ghostty intentionally compiles to an empty shell on non-macOS.)
```

## Control Plane

- `con-cli` is a real client for Con's local control socket, not a stub.
- Implementation details live in `docs/impl/socket-api.md`.
- The current live E2E workflow lives in `docs/impl/con-cli-e2e.md`.

## Local Skill

- Project-local skill: `skills/con-cli-e2e/SKILL.md`
- Use it when validating the control plane from an external agent or when writing eval automation against a real running Con session.
- The skill expects agents to prefer `con-cli --json`, verify pane capabilities before acting, and treat `panes create` as provisional until the new pane reports as alive and shell-ready.

## Design Language

- **Font**: IoskeleyMono (embedded, all weights) for terminal chrome — tabs, sidebar, input bar. System font (.SystemUIFont / SF Pro) for AI panel prose text, settings panel, and any non-terminal UI. Code blocks and terminal previews use IoskeleyMono via `mono_font.family` in theme JSON.
- **Default theme**: Flexoki Light. Dark available as Flexoki Dark.
- **Icons**: Phosphor Icons only (phosphoricons.com). Copy SVGs from `3pp/phosphor-icons/SVGs/regular/` into `assets/icons/phosphor/`. Never draw icons manually — always use the Phosphor library. Reference as `"phosphor/icon-name.svg"` in code. **Critical**: always set `.text_color()` directly on every `svg()` element — parent container color does NOT propagate to SVG stroke colors in GPUI.
- **Borderless**: No `border_1()`, `border_r_1()`, etc. Use opacity-based fills for surface separation.
- **Shadowless**: No `shadow_sm()`, `shadow_lg()`, etc. Use bg opacity for elevation.
- **Color by meaning only**: Monochrome surfaces by default. Accent color for semantic states (focus, active, warning, error).
- **Typography as hierarchy**: Size, weight, and opacity create structure — not boxes and borders.

See `docs/design/con-design-language.md` for full design system.

## UI/UX Principles

These principles govern all UI iteration. Follow them proactively — don't wait for the user to point out violations.

### Use gpui-component library first

Before building custom UI, check `3pp/gpui-component/` for an existing component. The library has 60+ components including:

- **Select** (`select::Select`, `SelectState<SearchableVec<String>>`) — searchable dropdowns. Use for any list selection instead of hand-rolling clickable divs.
- **Button** (`button::Button`) — use `.ghost()`, `.primary()`, `.small()`, `.icon()` variants. Never hand-roll clickable divs for buttons.
- **Input** (`input::Input`, `InputState`) — text fields with `.appearance(false)` for inline, `.cleanable()`, `.placeholder()`.
- **Switch** (`switch::Switch`) — toggles. Never hand-roll toggle divs.
- **Icon** (`Icon::default().path("phosphor/name.svg")`) — use with Button via `.icon()`.
- **Clipboard** (`clipboard::Clipboard`) — copy-to-clipboard with auto check-icon feedback.
- **Sidebar** (`sidebar::Sidebar`) — collapsible sidebar with icon-only mode.
- **Settings** (`setting::SettingPage`) — native settings layouts.

Read the component's source in `3pp/gpui-component/crates/ui/src/` to understand its API before using it. The `CLAUDE.md` in `3pp/gpui-component/` documents the full architecture.

### Visual normalization

- **Rounding consistency**: If one surface is flat (e.g., embedded terminal NSView), adjacent surfaces should also be flat. Don't mix rounded bubbles next to sharp-edged content.
- **Uniform widths**: When multiple panels/cards share a container and the user switches between them, use the same width for all to prevent layout jumping.
- **Icons over text labels** for mode indicators and compact controls. Text labels are for headings and settings fields.
- **Font context-switching**: Use mono font (Ioskeley Mono) for shell/command contexts. Use system font (.SystemUIFont) for natural-language/agent contexts and settings UI.

### Focus behavior

- After submitting input from the input bar, keep focus on the input bar — the user is in a "command flow" and will likely type more. They can click the terminal to focus it.
- Modal dialogs (settings, command palette) capture and return focus cleanly.
- Use `FocusInput` action (keybinding) as the explicit "focus the input bar" gesture.

### Density and wording

- Prefer compact, terse UI labels. "Browse Themes" not "Open Theme Catalog". "Load from Clipboard" not "Load Clipboard Theme".
- Remove anything the user can derive from context (e.g., don't show CWD when it's visible in the terminal).
- Placeholders should be action-oriented and short: "Run a command…", "Ask anything…".
- Avoid numbered step wizards for simple 2-3 step flows — inline everything flat.

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
