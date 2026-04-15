# Provider protocol selection was not persisted per provider

## What happened

For providers that support both OpenAI-compatible and Anthropic-compatible APIs, the Settings panel could show the wrong protocol after closing and reopening Settings. MiniMax and Z.AI were reported specifically.

The visible symptom was:

- switch `Anthropic API` on for MiniMax or Z.AI
- leave Settings
- reopen Settings and return to that provider
- the provider page fell back to the OpenAI variant and the switch appeared off

## Root cause

The Settings UI only tracked the currently selected provider variant in panel state.

That worked while the panel stayed open, but reopening Settings reset `selected_provider` from the global active agent provider. When the user later clicked a sidebar provider like `MiniMax`, the sidebar selection path had no persisted per-provider transport preference to consult, so it defaulted back to the OpenAI variant.

The state that mattered was provider-specific transport choice, but it was being stored only as transient view state.

## Fix applied

- Added explicit `provider_protocols` to `AgentConfig`
- Persisted `openai` vs `anthropic` transport choice for MiniMax, Moonshot, and Z.AI
- Updated the Settings sidebar selection path to restore the saved transport preference for that provider
- Added compatibility inference in config migration so existing configs with anthropic variants keep the correct transport without manual repair

## What we learned

- If a settings surface exposes provider-local behavior, that state must live in config, not only in the panel
- Reconstructing settings UI from unrelated global state is brittle when one screen mixes app-global and provider-local concerns
- Variant providers need an explicit preference model; inferring from whichever config entry happens to be non-empty is not durable
