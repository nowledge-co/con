# Settings Unsaved Close

## What happened

The standalone Settings window could be closed with unsaved edits. Closing the
window reverted the live preview snapshot, so changes appeared to be accepted
while the window was open but disappeared when Settings closed.

## Root cause

Standalone Settings used the same preview rollback path for native window close,
Escape, and Cmd-W. There was no dirty-state check between the current control
draft and the preview snapshot captured when the Settings window opened.

## Fix applied

Settings now derives a current draft from the controls, compares it with the
standalone preview snapshot, and turns close attempts with dirty settings into an
inline confirmation strip inside Settings. The strip offers Save and Close or
Keep Editing, and the workspace only clears its settings-window handle after a
confirmed close.

## What we learned

Preview rollback is safe only as an explicit discard path. Window-close paths
need their own confirmation flow whenever UI controls can represent unsaved
state. Native close callbacks should only decide whether closing is allowed; the
visible response should live in the existing window so platform event
re-entrancy cannot swallow it.
