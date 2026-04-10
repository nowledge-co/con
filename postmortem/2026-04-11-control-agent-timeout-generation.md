# What happened

Operator benchmark runs started showing a strange pattern: a later `con-cli agent ask`
turn on the same tab would fail with the timeout budget from an earlier turn.

Example:

- step 1 asked for 90s
- step 2 asked for 120s
- step 2 failed with `agent.ask timed out after 90s`

That made the benchmark look much worse than the actual product state and hid the real
turn-level behavior.

# Root cause

Con's control-plane `agent ask` timeout was keyed only by tab index.

Each request spawned a timer task, but that timer did not carry any request identity.
When a request finished, the pending request entry was removed, but the old timer task
was still alive. If a new `agent ask` started on the same tab before the old timer
expired, the stale timer would wake up and remove the new pending request.

So the failure was:

1. request A starts on tab 1 with timeout 90
2. request A completes
3. request B starts on tab 1 with timeout 120
4. request A's old timer fires and aborts request B

# Fix applied

- Added a monotonic `request_id` for control-plane agent asks in
  [workspace.rs](/Users/weyl/conductor/workspaces/con/kingston/crates/con/src/workspace.rs)
- Stored that `request_id` in `PendingControlAgentRequest`
- Made the timeout task only remove the pending request if the request id still matches

That makes the timeout request-scoped instead of tab-scoped.

# What we learned

- Timeouts for multiplexed control flows must always be generation-scoped or request-scoped.
- Benchmark failures are useful when they reveal real control-plane bugs, but only if the
  benchmark itself is isolated enough to make those bugs legible.
- In an agentic system, stale asynchronous cleanup is a frequent source of false product
  regressions; benchmark automation is good at surfacing those races.
