## What happened

Resizing a Con window that contained a heavy TUI such as Claude Code, or even a simple `tmux` session, felt dramatically slower than native Ghostty.

The visual symptom was consistent:

- one pane
- resize or heavy reflow would appear to repaint from upper rows down to the bottom
- standalone Ghostty stayed effectively instant

The lag reproduced in the simplest case:

- one pane
- one active terminal surface
- no split management involved
- no Claude-specific behavior required

## Root cause

The final root cause was more basic than the earlier host-integration theories:

Con's macOS Rust app was being built in release mode, but the embedded Ghostty runtime was not explicitly being built as a release-class Zig library. `crates/con-ghostty/build.rs` invoked:

```text
zig build -Dapp-runtime=none -Dxcframework-target=native -Demit-macos-app=false
```

without `-Doptimize=ReleaseFast`.

That left Con embedding a debug-grade or non-release-grade Ghostty runtime while standalone Ghostty.app was running an optimized one. In traces, the hot path was dominated by Ghostty's own terminal-core work:

- `terminal.page.Page.verifyIntegrity`
- `terminal.formatter.PageFormatter.*`
- `terminal.render.RenderState.update`
- `renderer.generic.Renderer(renderer.Metal).rebuildRow`
- `renderer.generic.Renderer(renderer.Metal).addGlyph`

That is why Con looked existentially slower while standalone Ghostty did not: the embedded runtime itself was being built in the wrong mode.

## Fix applied

- `crates/con-ghostty/build.rs` now passes Zig `-Doptimize=ReleaseFast` by default for the macOS Ghostty build.
- The optimize mode is also overridable with `CON_GHOSTTY_OPTIMIZE` for controlled experiments.
- The profiling helper was corrected to trace the built `con` binary instead of accidentally profiling `cargo`.
- Documentation was updated so performance investigations first confirm two invariants:
  1. the trace targets `con`, not Cargo
  2. the embedded Ghostty runtime is built as a release-class library

## What we learned

- Before diagnosing architecture, validate the build mode. A release Rust binary can still embed a slow native dependency if that dependency's own build system defaults to a non-release profile.
- Trace correctness matters as much as trace content. Profiling `cargo run -p con` under `xctrace` produced believable but irrelevant results because it sampled Cargo and rustup startup code instead of the app under test.
- The first lightweight logs were still useful. They ruled out Rust-side resize calls, Ghostty wake queue delay, and `ghostty_app_tick()` as the primary bottleneck.
- The decisive trace was the first valid `xctrace` capture of the actual `con` binary. Once that trace showed Ghostty terminal-core symbols dominating the hot path, the problem became a build/runtime issue rather than a host-view issue.
- Earlier host-path fixes were not necessarily wasted, but they were not the main reason Con felt dramatically slower than Ghostty. The embedded runtime build mode was.
