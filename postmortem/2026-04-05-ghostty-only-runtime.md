# What happened

con still carried two terminal stories at the same time:

- the real product path used embedded Ghostty
- a large internal VTE/PTTY fallback path still shaped workspace code, settings, docs, and agent execution logic

That split made the codebase harder to reason about and encouraged product behavior that was not actually true for the runtime users were running.

# Root cause

We kept the old backend alive as an escape hatch after the Ghostty migration.

Over time that created three problems:

1. workspace and harness code kept branching on abstractions that no longer matched the product
2. settings exposed controls such as backend choice and scrollback tuning that were not honest under Ghostty
3. design work around pane observability was being pulled toward a runtime we no longer wanted to ship

# Fix applied

We removed the legacy runtime path and made Ghostty the only terminal runtime:

- deleted the old `terminal_view.rs` path
- deleted the VTE/PTTY/grid code from `con-terminal`
- reduced `con-terminal` to theme and palette helpers
- made `TerminalPane` a Ghostty-backed wrapper instead of a backend enum
- simplified harness context building to snapshot-based Ghostty state only
- simplified `terminal_exec` completion handling to Ghostty command-finished plus bounded fallback
- removed backend selection and fake scrollback controls from config and settings
- wired Ghostty surface creation to inherit cwd and font size
- updated design and implementation docs to match the new boundary

# What we learned

- A fallback runtime is not free. If it is not exercised as a first-class product path, it quietly rots the architecture around it.
- Pane intelligence work must start from the real runtime's fact surface. Designing around a dead adapter leads to the wrong seams.
- Ghostty should stay the terminal engine. If con needs stronger observability, the correct move is to upstream or expose more libghostty API, not to rebuild a second terminal core beside it.
