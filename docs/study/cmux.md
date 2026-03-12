# Study: cmux (Reference Implementation)

## Overview

cmux is a macOS terminal emulator (Swift/AppKit) built on libghostty that provides a socket-based control API for AI agents. It's the primary reference for con's agent integration architecture.

## Architecture

- **Language**: Swift (macOS only)
- **UI**: SwiftUI + AppKit + Bonsplit (tab management)
- **Terminal**: libghostty via C FFI
- **Browser**: WKWebView (built-in browser for agent workflows)
- **Agent comms**: Unix domain socket, dual protocol (V1 text + V2 JSON-RPC)

## Key Insight: Agent-Agnostic Design

cmux does NOT embed an LLM. It provides infrastructure for external agents:
- Socket API for control (notifications, input, focus, metadata)
- Agents (Claude Code, Codex) run inside the terminal as regular processes
- cmux enhances the UX (notification rings, sidebar metadata) without "taking over"

**This is the pattern con should follow.** Built-in agent via Rig is an addition, not the core.

## Socket API (V2 JSON-RPC)

Key commands we should implement:

| Command | Purpose |
|---------|---------|
| `system.identify` | Return window/workspace/surface IDs, capabilities |
| `notification.create` | Trigger notification (blue ring, sidebar badge) |
| `notification.create_for_surface` | Target specific terminal pane |
| `terminal.write` | Send input to terminal |
| `context.get` | Get cwd, git branch, last command, exit code |
| `browser.open_split` | Open URL in split (if we add browser) |

## Socket Access Control

```
off           — socket disabled
con_only      — only processes spawned inside con can connect
automation    — external local clients allowed
password      — socket requires auth token
allow_all     — fully open (unsafe, dev only)
```

Default: `con_only` (safe by default).

## Threading Model (from cmux CLAUDE.md)

Critical for performance:
- **High-frequency telemetry** (`report_*`, port scanning): parse off-main-thread, coalesce, then update UI
- **Focus-mutating commands** (`window.focus`, `surface.focus`): main thread only
- **Never** let socket I/O block the render loop

## Patterns to Adopt

1. **Socket API as foundation** — built-in agent is just the first socket client
2. **Off-main socket threading** — hot paths never block rendering
3. **Notification system** — blue rings, sidebar badges, per-pane notifications
4. **Capability-based access** — plugins declare what they need, con grants/denies
5. **Stable handle refs** — human-readable IDs for workspaces/panes, survive restarts
6. **Sidebar metadata** — git branch, PR status, listening ports shown automatically

## Socket Path Convention

```
/tmp/con.sock              # production
/tmp/con-debug.sock        # debug builds
/tmp/con-debug-<tag>.sock  # tagged builds
```
