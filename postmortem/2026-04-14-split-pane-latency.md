## What happened

Pane splits in Con began to feel materially slower than Ghostty, especially in two-pane mode. The visible symptom was that invoking a split could take close to a second before the new pane felt present and responsive.

## Root cause

Two design mistakes were compounding on the interactive split path:

1. App-level split shortcuts were routed through `ghostty_surface_split`, then back into Con through Ghostty's pending-event loop. That added an unnecessary round-trip for a UI action Con already knows how to perform locally.
2. `save_session()` was synchronous and wrote the full session JSON to disk directly on the UI path after a split.

Neither issue was catastrophic alone, but together they made pane creation feel much heavier than it needed to.

## Fix applied

- App-level `SplitRight` / `SplitDown` now call Con's local `split_pane(...)` path directly.
- Session persistence is now written on the background executor instead of blocking the interactive path.

Ghostty-originated split requests are still supported through the existing event bridge, but Con no longer uses that bridge for its own top-level split shortcuts.

## What we learned

- UI actions should not round-trip through terminal action bridges when the workspace already owns the layout model.
- Persisting session state synchronously on interactive paths is the wrong tradeoff, even when the data format is small.
- Responsiveness regressions often come from several "reasonable" steps layered together rather than one dramatic mistake.
