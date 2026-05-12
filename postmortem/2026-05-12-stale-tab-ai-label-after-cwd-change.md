# Stale AI Tab Label After Directory Change

## What Happened

Vertical tab titles could keep showing an AI-generated label from the previous
working directory after the user ran `cd`. This was easiest to reproduce when
the suggestion model was disabled, because no new AI summary arrived to replace
the stale one. With the suggestion model enabled, the tab could also briefly
show the old label until a fresh summary completed.

## Root Cause

`smart_tab_presentation` intentionally gives AI labels higher priority than the
current directory basename. `on_terminal_cwd_changed` synced the sidebar before
clearing the tab's AI label, so the old label still won. Clearing the label
fixed the immediate display, but it was not enough: a summary request already
in flight for the previous directory could complete later and reapply the old
label.

## Fix Applied

On CWD changes, Con now clears the affected tab's AI label, invalidates that
tab's summary-engine state, and bumps a tab-local summary epoch. Summary
callbacks carry the epoch they were requested under, and `apply_tab_summary`
drops callbacks whose epoch no longer matches the tab. This preserves immediate
CWD fallback, allows a fresh request to dispatch without the normal summary
budget delay, and prevents late stale AI responses from overwriting it.

## What We Learned

For async UI decoration, clearing visible state is only half the fix. Any
in-flight producer that can write the same state must carry enough identity to
prove it still belongs to the current user-visible context.
