# Study: External Socket Control Patterns

## Overview

A terminal product that supports agent workflows benefits from a socket-based control plane that is independent from any built-in model runtime.

The important pattern is:

- terminal stays canonical
- automation clients talk to the app over a local socket
- UI enhancements help orientation and safety, without taking over terminal ownership

## Architecture

- **UI**: native macOS shell
- **Terminal**: embedded terminal runtime
- **Agent comms**: Unix domain socket, JSON-RPC style messaging

## Key Insight: Agent-Agnostic Design

The app should provide infrastructure for external agents:

- Socket API for control and notifications
- Agents run inside the terminal as regular processes when appropriate
- The app enhances workflow visibility without becoming the workflow owner

Built-in agent support can exist, but it should be an addition, not the foundation.

## Socket API

Key commands worth supporting:

| Command | Purpose |
|---------|---------|
| `system.identify` | Return window/workspace/surface IDs, capabilities |
| `notification.create` | Trigger notification |
| `notification.create_for_surface` | Target specific terminal pane |
| `terminal.write` | Send input to terminal |
| `context.get` | Get cwd, git branch, last command, exit code |
| `browser.open_split` | Open URL in split, if browser support exists |

## Socket Access Control

```text
off           — socket disabled
con_only      — only processes spawned inside con can connect
automation    — external local clients allowed
password      — socket requires auth token
allow_all     — fully open (unsafe, dev only)
```

Default: `con_only`

## Threading Model

Critical for performance:

- high-frequency telemetry should parse off-main-thread, coalesce, then update UI
- focus-mutating commands stay on the main thread
- socket I/O must never block rendering

## Patterns to Adopt

1. **Socket API as foundation** — built-in agent is just one client
2. **Off-main socket threading** — hot paths never block rendering
3. **Notification system** — per-pane and global notifications
4. **Capability-based access** — clients declare what they need
5. **Stable handle refs** — human-readable IDs for workspaces and panes
6. **Sidebar metadata** — contextual system state surfaced without terminal takeover

## Socket Path Convention

```text
/tmp/con.sock              # production
/tmp/con-debug.sock        # debug builds
/tmp/con-debug-<tag>.sock  # tagged builds
```
