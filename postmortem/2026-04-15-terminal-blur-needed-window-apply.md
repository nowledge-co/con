# Terminal blur needed explicit window apply

## What happened

Con exposed a terminal blur setting and wrote `background-blur` into Ghostty's runtime config, but users could toggle the setting without seeing any visual change in existing windows.

## Root cause

On macOS, Ghostty does not rely on config reload alone for blur. It also applies blur through `ghostty_set_window_background_blur(...)` on the native `NSWindow`.

Con updated the Ghostty app and surface config, but never invoked that window-level blur API after settings changes.

## Fix applied

- Added the missing FFI for `ghostty_set_window_background_blur`
- Applied window blur when a Ghostty surface is first attached
- Reapplied window blur after runtime appearance updates across all live terminal panes

## What we learned

Some Ghostty appearance settings are config-backed but still require a macOS window-side apply step. For embedded Ghostty integration, runtime appearance work must cover both config mutation and native window effects.
