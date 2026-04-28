# Agent Panel Tool Trace Collapse

## What Happened

After an agent turn with many tool calls, the assistant response could be pushed out of view by an expanded "Actions, probes, and output" trace.

In some completed turns the trace could still show live steps, making the panel look like work was ongoing after the final response had arrived.

## Root Cause

`PanelState::complete_response` auto-collapsed existing steps before it drained the live tool calls into the assistant message's step timeline.

For turns where the only steps were still in `tool_calls`, the collapse check saw an empty step list and did nothing. The drained tool cards then landed expanded by default. Any unresolved tool call drained at response completion was also preserved as `Running`, which kept the completed trace labeled as active.

## Fix Applied

The tool calls are now drained into the assistant message before deciding whether to collapse the trace.

When the final response has arrived, any remaining tool call without a result is finalized as complete instead of left live, since response completion means the turn is no longer actively executing that tool.

Focused tests now cover both behaviors:

- Completed tool calls are collapsed after being drained into the assistant message.
- Response completion does not leave unresolved tool calls marked live.

## What We Learned

Completion-time UI cleanup needs to run after all transient turn state has been folded into the persisted message state. Otherwise the UI can make the correct cleanup decision against the wrong snapshot.
