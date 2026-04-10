# Conversation Trace Persistence Loss

## What happened

After restarting Con, the agent panel restored message text but lost the visible execution trace for prior assistant turns.

That made restored sessions feel much emptier than the live session:

- tool activity appeared to have never happened
- model and duration metadata were partially lost
- the agent's reasoning trail disappeared even though the conversation itself still existed

## Root cause

Two write/read paths were both lossy.

1. The harness only persisted the final assistant text. It did not save streamed thinking text or the tool-call/tool-result trace into the stored conversation message.
2. The panel restore path rebuilt messages from plain content only, so even a richer stored conversation would not have reconstructed the assistant timeline.

## Fix applied

- Added persisted assistant-trace fields to stored conversation messages:
  - `thinking`
  - `steps`, with optional `call_id` on tool call/result steps for pairing
- Captured streamed thinking and tool events inside the harness request lifecycle and attached them to the final saved assistant message
- Rebuilt restored panel state from persisted assistant traces, including:
  - thinking block
  - structured tool steps
  - model label
  - duration
- Added tests for message serialization round-trip and panel restoration

## What we learned

- Session persistence is not complete if it only saves message text. In an agent product, the execution trace is part of the session.
- Live UI state and persisted conversation state must share the same conceptual model. If the live panel is richer than the stored conversation, restart will always feel broken.
- Benchmark and scoring history are useful here too: continuity regressions should become visible as part of the product loop, not only through ad hoc manual discovery.
