# Provider Protocol Endpoint Loss

## What Happened

Users configuring MiniMax, Moonshot, or Z.AI with a regional/domestic base URL
could switch the provider card into Anthropic protocol mode, save Settings, and
return to Con with the provider effectively back on the wrong protocol or endpoint.
The common failure mode was a China endpoint falling back to the global endpoint,
which made otherwise valid tokens fail.

## Root Cause

The Settings UI had two pieces of state for dual-protocol providers:

- The sidebar provider identity, such as MiniMax or Z.AI.
- The actual protocol variant, such as MiniMax Anthropic or Z.AI Anthropic.

When toggling protocol mode, Con seeded missing fields into the target variant
but preserved any stale non-empty target base URL. That meant a previously saved
global Anthropic endpoint could survive even when the user had just selected a
China endpoint on the OpenAI-compatible side.

The active provider could also remain on the sidebar/base variant after the
provider card switched to Anthropic mode. Reopening Settings then made the
configuration look lost because the provider controls were driven partly by the
global active provider and partly by the selected provider card.

## Fix Applied

- Protocol toggles now map named endpoint presets across variants and replace
  stale target endpoints when the source endpoint is a known preset.
- Custom target endpoints are preserved when the source endpoint is custom.
- The active provider and suggestion provider are normalized through the saved
  protocol preference before saving or rebuilding model controls.
- Provider Settings controls now rebuild from the selected provider card, not
  from the unrelated global active provider.

## What We Learned

For dual-protocol providers, the sidebar provider is only a grouping label. The
saved protocol variant is the runtime provider identity and must drive active
model selection, suggestion routing, endpoint presets, and save/reopen behavior.
