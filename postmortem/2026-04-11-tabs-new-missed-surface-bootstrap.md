# tabs.new created a tab whose first pane never became live

## What happened

The benchmark runner started isolating live-socket operator runs in a fresh tab via `tabs.new`.

That immediately exposed a real runtime defect:

- `tabs.new` returned successfully
- `panes.list` on the new tab showed one pane
- but that pane stayed `surface_ready=false` and `is_alive=false`
- operator benchmarks could not start because the tab never exposed a live shell surface

## Root cause

`Workspace::new_tab()` did not bootstrap the new tab the same way `Workspace::activate_tab()` bootstraps an existing tab.

`activate_tab()` explicitly:

- marks all terminals in the target tab visible
- calls `ensure_surface(window, cx)` on them
- focuses the active terminal
- schedules the bootstrap reassert loop

`new_tab()` only focused the new terminal and scheduled the later reassert loop. That was not strong enough for control-plane automation, where the new tab must become live immediately after the control request completes.

## Fix applied

- Updated `Workspace::new_tab()` to mark the new tab's terminals visible and call `ensure_surface(window, cx)` immediately, matching the existing activation path.
- Revalidated against a live benchmark run using a fresh tab on `/tmp/con.sock`.

## What we learned

- Control-plane creation paths must use the same bootstrap rules as interactive UI paths. If they diverge, automation hits dead panes even though the UI path looks fine.
- Benchmark isolation was worth doing. It exposed a real product bug instead of just making the benchmark cleaner.
