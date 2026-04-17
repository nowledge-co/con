# 2026-04-16 — Preparing for Windows support

## What happened

Con shipped its first 18 months as a macOS-only application. The runtime
stack — embedded libghostty rendered through a child NSView in a GPUI
window — assumes Cocoa, Metal, IOSurface, Carbon, Objective-C
trampolines, Sparkle, and Unix domain sockets at every layer.

A Windows port had been an open user request for several months. Before
attempting the port itself we did a structured study of the upstream
dependencies to confirm what is realistic in 2026, and used that to
write a staged plan and a first preparatory PR.

## What we found

Three upstream realities shaped the plan:

1. **libghostty is not portable.** The full embedded C API
   (`ghostty_app_*`, `ghostty_surface_*`) is macOS-only. The
   `ghostty_platform_e` enum has only `MACOS` and `IOS` variants. A
   community fork (`InsipidPoint/ghostty-windows`) has a Win32 apprt
   but ships a standalone .exe and does not re-export the embedded C
   API. Ghostty maintainers have shipped `libghostty-vt` (PR #8840) as
   the cross-platform answer — it gives you a parser and screen state
   machine, not a renderer or a PTY.

2. **GPUI on Windows works but doesn't embed HWNDs.** Zed runs on
   Windows in beta on a D3D11/DirectComposition backend. The PR that
   added `WindowKind::Child` for HWND embedding
   (zed-industries/zed#24330) was closed unmerged because maintainers
   wanted a cross-platform API first. The macOS pattern of "give the
   terminal library your view and let it render into it" has no Windows
   analog in upstream GPUI.

3. **Our docs were already drifting.** `CLAUDE.md` described the UI
   layer as "GPUI-CE v0.3.3", but the workspace `Cargo.toml` had been
   moved to upstream `zed-industries/zed` gpui at some point and the
   doc never followed. That mattered because gpui-ce is macOS-only and
   would have been a port blocker — discovering after a day of work
   that we were actually on upstream gpui (which has a Windows backend)
   would have been an unpleasant surprise.

The combination of (1) and (2) means Windows is **not** a "build
libghostty for Windows and reuse `con-ghostty`" port. It is a
"different terminal backend, different embedding strategy" port. The
plan accordingly splits into three independent hurdles (terminal
backend, GPUI embedding, host-side Windows-isms) and is documented in
`docs/impl/windows-port.md`.

## Root cause of the macOS-only state

Nothing surprising — the project was built quickly to ship a macOS
product. The macOS-specific calls (NSView creation, NSPasteboard, dock
icon, NSBundle Info.plist reads, Sparkle, Carbon global hotkey) live
behind `cfg(target_os = "macos")` gates already, but the `con` binary
crate has a hard `compile_error!("con currently requires macOS …")` in
the crate's `main.rs` and the non-UI crates assume Unix sockets at
`/tmp/con.sock` with `chmod 0600`.

## The reserved-name bite

Shortly after the first prep push we tried to clone the repo on a
Windows machine and hit:

```
git clone git@github.com:nowledge-co/con.git
fatal: could not create work tree dir 'con': Invalid argument

git clone git@github.com:nowledge-co/con.git con-terminal
error: invalid path 'crates/con/Cargo.toml'
fatal: unable to checkout working tree
```

`CON` (along with `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`) is
a DOS device name that Windows reserves globally across its Win32 file
APIs, regardless of extension. Git on Windows refuses to create any
path component called `con`, so the repository couldn't be checked out
at all. The `cargo check --target x86_64-pc-windows-gnu` validation we
ran from Linux didn't surface this because cross-compilation never
creates a directory named `con` on the host filesystem — it only reads
the paths inside the repo.

Mitigation applied in the same PR series: renamed the UI crate
directory from `crates/con/` to `crates/con-app/`. The Cargo package
name stays `con` and the `[[bin]] name = "con"` stays `con`, so
`cargo run -p con` and the produced `con` binary on macOS are
unchanged. The binary name will need a per-target rename when the
Windows backend actually builds (`con.exe` is also reserved); the
mechanism is documented in `docs/impl/windows-port.md` under
"Binary naming on Windows".

## Fix applied (this PR — Phase 0)

This PR is preparation, not the port. Goals:

- Capture what we learned in a permanent plan doc that future
  contributors can pick up cold.
- Make the non-UI crates (`con-core`, `con-agent`, `con-cli`,
  `con-terminal`) compile on Windows by abstracting the Unix-specific
  pieces (control socket transport, hard-coded `/tmp` fallbacks,
  `chmod 0600` permissions) behind `cfg(unix)` / `cfg(windows)` paths.
- Stop `con-ghostty`'s `build.rs` from invoking `zig build` on
  non-macOS targets so a Windows `cargo check` doesn't fail at the
  build script.
- Replace the macOS-only `compile_error!` in the `con` binary with a
  message that points at `docs/impl/windows-port.md`, so anyone hitting
  it knows where to read about the plan and what the blockers are.
- Fix the stale GPUI-CE wording in `CLAUDE.md`.

We deliberately did **not** attempt to make the `con` UI binary
compile on Windows. That requires Phases 2-3 (a backend trait + a
Windows backend implementation) which are real engineering work and
should be separate, reviewable PRs.

## What we learned

- Doing the upstream study **before** writing any code was worth it.
  Learning that GPUI doesn't have HWND child embedding upstream changes
  the architecture of the Windows backend completely (Hurdle 2 in the
  plan). Discovering that two months into implementation would have
  been costly.
- Documentation drift bites: the GPUI-CE wording in CLAUDE.md was
  three months stale. Worth a periodic check that CLAUDE.md still
  matches `Cargo.toml`.
- Splitting the port into three independent hurdles (backend, GPUI
  embedding, host-isms) means contributors with different skills can
  work in parallel. The first preparatory PR can land without anyone
  having committed to a particular backend strategy.
- Having `con-ghostty` already isolated as its own crate, with a
  narrow Rust-side API consumed by `terminal_pane.rs`, means a
  Windows backend can be a sibling crate with the same surface — the
  rest of the UI doesn't need to know which backend rendered the pane.
  This was good architectural luck, not foresight, but worth noting.
