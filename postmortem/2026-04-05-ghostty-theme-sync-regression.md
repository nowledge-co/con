# What happened

After the Ghostty-only cleanup, changing the terminal theme in Settings could update con's own interface while the embedded Ghostty terminal kept an older palette.

The failure was easiest to see when font size or another runtime appearance update happened after the theme change. The terminal could fall back to a previous or default-looking Ghostty palette even though the selected theme in Settings was correct.

# Root cause

Ghostty runtime appearance updates were being treated as one-off patches instead of a merged source of truth.

Two details combined into the regression:

1. `GhosttyApp::update_config` built a fresh config from only the incoming patch, so an update that only mentioned `font-size` could drop previously applied colors.
2. Settings save pushed `font-size` separately and only reapplied the full theme when the theme name itself changed.

That meant the app-level Ghostty config could become incomplete over time. con's own UI theme stayed correct because it was driven by `TerminalTheme`, but Ghostty received a lossy runtime config.

# Fix applied

We made Ghostty appearance updates stateful and full-fidelity:

- `GhosttyApp` now keeps the last merged appearance state in memory
- every runtime config update merges into that state before serializing a Ghostty config
- Ghostty app creation now starts with both colors and font size, not colors alone
- Settings save now reapplies the full terminal appearance instead of sending a standalone font-size update first
- added regression tests for config-patch merge behavior

# What we learned

- Runtime config APIs must have a clear ownership model. If the underlying library accepts full config objects, our wrapper should preserve a merged source of truth instead of assuming partial updates are additive.
- The terminal theme is not just a UI concern. con chrome and Ghostty surfaces need one shared appearance model, or they will drift apart.
- Small convenience update paths such as "just update font size" are risky unless they are proven to preserve the rest of the runtime state.
