# Pending Control Request Tab-Index Reuse

## What happened

The new local session-resume benchmark exposed a false failure before the first operator step. A freshly created benchmark tab reported:

- `the target tab stayed busy with another agent request for too long`

even though the benchmark runner had just created that tab and started a fresh conversation.

## Root cause

Con tracked pending control-plane `agent.ask` requests in `pending_control_agent_requests`, keyed by the current tab index.

That is unsafe when tabs are closed or reset:

- if a benchmark run is interrupted while an `agent.ask` is still pending
- and the benchmark tab is later closed or reset
- the stale pending entry can remain under the old numeric tab index
- the next fresh tab that reuses that index inherits the stale busy state

So the bug was not in benchmark setup. The bug was that tab lifecycle and pending request lifecycle were not coupled.

## Fix applied

Con now cleans up pending control-plane agent requests during tab teardown:

- closing a tab removes any pending request for that tab and shifts higher tab indexes down
- resetting the last remaining tab clears any pending request for that tab before replacing its session

If a request is still pending when the tab disappears, Con now resolves that control request with an explicit error instead of silently leaking it.

## What we learned

- Tab index is not a stable lifetime key.
- Any request state keyed by tab index must be reindexed or invalidated during tab teardown.
- The session-resume benchmark was valuable because it found a control-plane lifecycle bug that simpler happy-path benchmarks would not hit.
