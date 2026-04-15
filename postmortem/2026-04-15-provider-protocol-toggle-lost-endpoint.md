# Provider protocol toggle did not carry endpoint intent across transports

## What happened

For paired providers like MiniMax and Z.AI, switching between the OpenAI-compatible and Anthropic-compatible transports could drop the working endpoint selection.

Example:

- choose MiniMax
- set endpoint preset to `China`
- enable `Anthropic API`
- the UI switched to the Anthropic transport, but the endpoint fell back to the default target config instead of the matching `China` endpoint

That could immediately break requests with token or endpoint errors.

## Root cause

The transport toggle only changed the selected provider kind.

It did not seed the target transport variant from the current variant when the target variant had not been configured yet. That meant:

- base URL did not map across protocol variants
- shared fields like model, API key, and token limit could also be absent on the target side

The model was wrong: these are not unrelated providers. They are the same provider family with two transports.

## Fix applied

- when toggling protocol, persist the current variant first
- seed the target variant from the current variant when target fields are unset
- map endpoint presets by semantic label where possible
- if the target transport exposes only one valid endpoint preset, use it as the fallback

This preserves `China -> China` for MiniMax and gives Z.AI Anthropic its only valid endpoint instead of an empty/default one.

## What we learned

- Protocol toggles between paired providers need semantic state transfer, not just variant switching
- Endpoint presets should be mapped by intent, not by raw URL
- If a target transport has a single valid endpoint, defaulting to blank is not neutral; it is broken
