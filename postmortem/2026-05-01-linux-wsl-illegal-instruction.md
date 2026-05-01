# Linux WSL Illegal Instruction at Startup

## What happened

A WSL Ubuntu 22.04.5 user reported that Con printed the initial startup
logs and then exited with `Illegal instruction (core dumped)`:

```text
[INFO con] con starting
[INFO con] config loaded
Illegal instruction (core dumped)
```

The crash happened before the next startup log after GPUI platform
construction, so there was no terminal-pane traceback or Rust panic to
symbolicate from the issue alone.

## Root cause

The Linux and Windows terminal backends build Ghostty's `libghostty-vt`
with Zig. We invoked `zig build` without an explicit `-Dtarget`, which
lets Ghostty's `standardTargetOptions` default to the build host's native
CPU.

That is wrong for release artifacts. The Rust binary can still be built
for generic `x86_64-unknown-linux-gnu`, but the linked Zig static archive
may contain instructions selected for the GitHub Actions runner CPU. A
WSL VM, older desktop CPU, or constrained virtualized environment can then
hit a SIGILL before the app opens a window.

## Fix applied

`con-ghostty` now derives a generic Zig target from Cargo's target triple
for the `libghostty-vt` build:

- `x86_64-unknown-linux-gnu` -> `x86_64-linux-gnu`
- `aarch64-unknown-linux-gnu` -> `aarch64-linux-gnu`
- `x86_64-pc-windows-msvc` -> `x86_64-windows-msvc`
- `aarch64-pc-windows-msvc` -> `aarch64-windows-msvc`

The macOS full-libghostty build remains unchanged because it uses the
native/fat macOS build path intentionally. An escape hatch,
`CON_GHOSTTY_VT_TARGET`, exists for explicit cross-target experiments.

## What we learned

Portable Rust release settings are not sufficient when a build script
links native static archives from another compiler. Every shipped native
sub-build needs an explicit target baseline; otherwise the weakest user
machine is silently determined by the strongest CI runner CPU.
