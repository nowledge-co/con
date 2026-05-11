# Keybinding Runtime Lifecycle

## What happened

Keyboard shortcuts had three related failure modes:

- Pressing the command palette shortcut repeatedly while still holding the
  modifier keys could close the palette and leave the next press ignored until
  the full chord was released.
- Settings allowed two actions to use the same shortcut without warning.
- Shortcut edits were saved to config, but existing app-level keybindings stayed
  active until restart.

## Root cause

The command palette shortcut was modeled as a toggle even though summon-style
modal shortcuts should be idempotent while the modifier chord is still active.
The settings panel also treated recorded shortcuts as plain strings, so it never
canonicalized or compared them before save.

Runtime rebinding had a separate lifecycle bug: GPUI keybindings are appended
unless an explicit `Unbind` is installed. Re-applying the new config without
unbinding the old configurable shortcuts left stale chords in the app keymap.

## Fix applied

- The workspace action for the command palette now shows/refocuses the palette
  instead of toggling it closed. Dismissal stays explicit through Escape,
  backdrop click, or command execution.
- Keybinding config now exposes canonical conflict detection, including enabled
  global shortcuts and reserved app/system shortcuts.
- Settings shows a shortcut-conflict error immediately after recording a
  conflicting chord and blocks saving until the conflict is resolved.
- Runtime settings apply now unbinds the previous configurable app shortcuts
  before binding the new configurable shortcuts. Fixed/system shortcuts remain
  startup-only, so repeated settings saves do not keep growing duplicate entries.

## What we learned

Shortcut settings need the same lifecycle discipline as any other mutable app
resource: validate before save, remove old runtime state before installing new
state, and keep summon-style modal shortcuts idempotent while the key chord is
active.
