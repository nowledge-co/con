## What happened

Every fresh window could open with a blank first terminal pane until some later UI mutation occurred.

## Root cause

Ordinary Ghostty surfaces were being created during the very first layout pass. On startup, that pass can happen before GPUI/AppKit has fully settled the host window view hierarchy, so the embedded terminal starts from an unstable host view state.

## Fix applied

Normal pane initialization is now deferred by one GPUI frame after the first layout:

- first layout records the pane bounds
- the next frame creates the Ghostty surface
- the same deferred step reapplies the pane frame

Control-plane eager initialization remains separate.

## What we learned

The initial pane should not share the exact timing assumptions of later panes created after the window is already live. Startup needs a later creation point than steady-state pane splits.
