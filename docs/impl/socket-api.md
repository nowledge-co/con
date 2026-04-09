# Implementation: Socket API

## Overview

con exposes a Unix domain socket for external agents and plugins to control the terminal using a small JSON-RPC protocol.

## Socket Path

| Build | Path |
|-------|------|
| Production | `/tmp/con.sock` |
| Debug | `/tmp/con-debug.sock` |
| Tagged | `/tmp/con-debug-<tag>.sock` |

Env override: `CON_SOCKET_PATH`

## Protocol

JSON-RPC 2.0 over Unix domain socket. Newline-delimited messages.

### Request
```json
{"jsonrpc": "2.0", "method": "notification.create", "params": {"title": "Build done", "body": "All tests pass"}, "id": 1}
```

### Response
```json
{"jsonrpc": "2.0", "result": {"notification_id": "abc123"}, "id": 1}
```

## Methods

### System
| Method | Description |
|--------|-------------|
| `system.identify` | Get window/workspace/pane IDs, con version |
| `system.capabilities` | List all available methods |

### Notifications
| Method | Description |
|--------|-------------|
| `notification.create` | Global notification |
| `notification.create_for_pane` | Target specific pane (blue ring) |
| `notification.dismiss` | Dismiss notification by ID |

### Terminal
| Method | Description |
|--------|-------------|
| `terminal.write` | Send input to active (or specified) pane |
| `terminal.read` | Get last N lines of output |
| `terminal.resize` | Resize pane |

### Context
| Method | Description |
|--------|-------------|
| `context.get` | Get cwd, git branch, last command, exit code |
| `context.subscribe` | Stream context changes |

### Focus
| Method | Description |
|--------|-------------|
| `focus.pane` | Focus a specific pane |
| `focus.window` | Bring window to front |

## Access Control

```toml
# config.toml
[socket]
access = "con_only"  # default: only child processes can connect
# access = "automation"  # local clients
# access = "password"    # require auth
# access = "off"         # disabled
```

## Threading

- Socket listener: dedicated thread
- Message parsing + validation: off-main-thread
- Coalesce high-frequency messages (e.g. repeated context.get)
- Focus-mutating commands dispatch to main/UI thread
- Never block the render loop

## Plugin Usage

```javascript
// Node.js plugin example
const { ConClient } = require('@con/sdk');

const con = new ConClient(process.env.CON_SOCKET_PATH);
await con.notification.create({ title: 'Deploy complete' });
const ctx = await con.context.get();
console.log(`User is in ${ctx.cwd} on branch ${ctx.git_branch}`);
```
