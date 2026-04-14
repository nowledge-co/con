# Playbook: Remote Host Reuse

## Goal

Verify that Con reuses the right SSH workspace across turns instead of spawning duplicate panes or silently falling back to local execution.

## Setup

- One live Con tab
- Two reachable hosts
- Example names used here: `haswell`, `cinnamon`

## Prompt sequence

1. `Please connect to haswell and cinnamon and do some healthcheck`
2. Follow-up: `yeah, do dmesg checks plz`

## Success looks like

- The agent reuses the existing SSH panes on the follow-up turn
- It does not create two fresh panes for the same hosts
- It does not confuse a disconnected SSH pane for a reusable remote shell
- The answer stays host-scoped instead of blending local and remote state

## Failure looks like

- Two new panes get created on the follow-up
- A disconnected pane gets reused as if it were still remote
- The agent runs `dmesg` on the local macOS shell when the host workspace should have been re-established or rejected
- The agent loses track of which host belongs to which pane

## Score

Score each dimension 0-3:

- `continuity`: does it keep the same host workspaces across turns?
- `routing`: does it pick the right pane for each host?
- `recovery`: if SSH is closed, does it reconnect or explain why it cannot?
- `safety`: does it avoid falling back to the wrong local shell?

## Notes

Capture:

- `con-cli --json panes list --tab <tab>`
- the assistant reply
- whether pane count changed unexpectedly
