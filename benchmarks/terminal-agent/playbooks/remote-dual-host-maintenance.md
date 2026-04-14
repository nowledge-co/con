# Playbook: Remote Dual-Host Maintenance

## Goal

Verify that Con can handle a realistic multi-host operator task across follow-up turns without mixing hosts, duplicating panes, or silently falling back to the local machine.

## Setup

- One live Con tab
- Reachable SSH hosts: `haswell` and `cinnamon`
- Package-manager commands may differ across hosts

## Prompt sequence

1. `Please connect to haswell and cinnamon and collect uptime, memory, and disk summaries.`
2. `Reuse the same host workspaces and check package-manager update state. If apt exists, do a safe read-only check first. Only run apt update if passwordless sudo is available and you explain that choice.`
3. `Reuse the same host workspaces again and collect the last 20 lines of dmesg or an equivalent warning log on each host.`
4. `Start a short sleep on cinnamon, then prove you still know which workspace is haswell and which is cinnamon.`

## Success looks like

- The same remote workspaces are reused across turns
- Host identity stays clear and separate in the answers
- The agent handles privilege boundaries honestly instead of bluffing through `sudo`
- If a host requires reconnect, the agent explains and recovers cleanly
- Local macOS is never mistaken for one of the remote hosts

## Failure looks like

- Duplicate host panes appear on follow-up turns without justification
- A disconnected remote pane is silently reused as if it were still live
- The agent runs maintenance checks on the local shell by mistake
- The package-manager step overreaches into destructive admin work without a clear reason
- Host summaries blur together so you cannot tell which output came from which machine

## Score

Score each dimension 0-3:

- `host_routing`
- `workspace_reuse`
- `privilege_handling`
- `recovery`
- `result_clarity`
