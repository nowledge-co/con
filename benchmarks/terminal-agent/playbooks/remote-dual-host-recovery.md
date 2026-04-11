# Remote Dual-Host Recovery

## Goal

Verify that Con can:

- keep `haswell` and `cinnamon` distinct
- disconnect only one remote workspace
- recover only the affected host
- avoid recreating the healthy host workspace

## Setup

- Start from a clean Con tab
- Ensure `haswell` and `cinnamon` are reachable over SSH

## Prompt sequence

1. `Please connect to haswell and cinnamon, confirm which pane belongs to which host, and collect a short hostname plus uptime summary from each.`
2. `Disconnect only the cinnamon workspace cleanly, keep haswell alive, and prove you still know which pane is which afterward.`
3. `Recover cinnamon only, without recreating or disturbing the haswell workspace, then collect a short free-memory summary from both hosts.`
4. `Summarize the final host-to-pane mapping and exactly which workspace had to be recreated or reused during recovery.`

## Success looks like

- The agent creates or reuses one pane for `haswell` and one for `cinnamon`
- The disconnect turn exits only `cinnamon`
- The follow-up turn recreates or reconnects only `cinnamon`
- `haswell` stays mapped to the same pane across the whole scenario
- The final summary clearly states which pane was reused and which was recovered

## Failure looks like

- The agent loses host identity after the disconnect
- The agent recreates both hosts instead of only `cinnamon`
- The agent runs the follow-up on local macOS or the wrong host
- The final mapping is vague or internally inconsistent

## Scoring focus

- host routing
- selective recovery
- workspace reuse
- recovery honesty
- result clarity
