# Implementation: macOS Terminal Profiling

This note exists for one specific class of bugs:

- a heavy TUI such as Claude Code resizes instantly in Ghostty or iTerm2
- the same TUI in Con appears to repaint from upper rows down to the bottom
- Con feels hung or many seconds behind during live resize
- the same lag can reproduce even with a minimal TUI such as an otherwise empty `tmux`, which means the issue is not Claude-specific

At that point, guessing is lower value than measuring.

## What we already know

- Con **does** use GPU rendering on macOS.
- The terminal runtime is embedded Ghostty.
- Ghostty renders into a native `NSView` with its Metal renderer.
- The remaining gap is therefore most likely in Con's host path:
  - AppKit view embedding
  - GPUI window composition
  - main-thread scheduling around Ghostty wakeups
  - native resize/composition behavior

## Lightweight app logs

Con now exposes an opt-in trace mode.

Fast path:

```bash
./scripts/macos/profile-terminal-resize.sh
```

Time Profiler capture:

```bash
./scripts/macos/profile-terminal-resize-xctrace.sh
```

If `cargo` is not on the non-interactive launch PATH that `xctrace` sees, set it explicitly:

```bash
CARGO_BIN="$(command -v cargo)" ./scripts/macos/profile-terminal-resize-xctrace.sh
```

Equivalent manual form:

```bash
CON_GHOSTTY_PROFILE=1 \
RUST_LOG=con::perf=info,con_ghostty::perf=info,con=warn,con_core=warn,con_agent=warn \
cargo run -p con
```

This emits two high-signal log streams:

- `con::perf`
  - every real `ghostty_surface_set_size` request from the host view
  - includes old pixel size, old grid size, new pixel size, logical pane size, and host call time
  - host-side `update_frame` timing for the embedded terminal view
  - workspace-level Ghostty wake drain timing
- `con_ghostty::perf`
  - every Ghostty wakeup-driven app tick
  - includes queue delay before the main-thread tick and tick execution time

Interpretation:

- high `queue_delay_ms`
  - main thread is busy before Ghostty even gets a chance to process the wakeup
- low `queue_delay_ms`, low `tick_ms`, but still visibly slow resize
  - likely AppKit/CA composition or host embedding cost after the terminal core updates
- many resize requests with tiny geometric deltas
  - host resize churn is still too high
- few resize requests, but each followed by visible multi-second lag
  - likely not `ghostty_surface_set_size` itself
- low resize call time, low tick time, low wake drain time, but still visible lag
  - likely AppKit / Core Animation / compositing around the embedded terminal view
- high `update_frame` elapsed time
  - the embedding host path itself is expensive before Ghostty work begins

## Instruments pass

When logs are not enough, capture one trace from Con and one from Ghostty with the same TUI and the same resize gesture.

Use Instruments:

1. `Time Profiler`
2. `Core Animation`
3. `Metal System Trace`

Suggested workflow:

1. Launch Con from Xcode Instruments or attach Instruments to the running process.
2. Start a heavy TUI, for example `claude --resume`.
3. Also capture the simpler control case: launch `tmux`, then resize for 3-5 seconds.
4. Repeat the same capture with Ghostty.

The helper script above automates the `Time Profiler` launch for Con by wrapping:

```bash
xcrun xctrace record --template 'Time Profiler' --launch -- /absolute/path/to/cargo run -p con
```

Look for:

- main-thread time in AppKit / Core Animation during Con resize
- time under GPUI window or layer composition
- time spent in `ghostty_app_tick`
- time spent in `ghostty_surface_set_size`
- whether Metal work itself is slow, or whether Con is late getting frames onto screen

## Current thesis

If Con remains much slower than Ghostty even when:

- Ghostty wake tick delay is low
- Ghostty tick time is low
- resize request count is reasonable

then the problem is probably structural, not a missing one-line fix:

- the transparent GPUI host/compositor path around the terminal
- the embedding model of the native terminal view inside the GPUI window

That would justify a larger architectural pass instead of more local tweaks in `ghostty_view.rs`.
