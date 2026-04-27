# Windows Terminal Tab Completion

## What Happened

Pressing Tab inside the Windows terminal did not trigger shell completion, even though the same shell completed normally in Windows Terminal.

## Root Cause

The Windows terminal view relied on `on_key_down` to forward keys into ConPTY, but Tab is a focus-navigation key in GPUI Root. macOS already bound Tab and Shift+Tab inside the `GhosttyTerminal` key context so the terminal consumes them before root focus cycling. The Windows view declared the same action names but left `init()` as a no-op and did not attach the `GhosttyTerminal` key context to the rendered terminal element.

## Fix Applied

The Windows terminal view now mirrors the macOS terminal contract:

- `init()` binds `tab` and `shift-tab` in the `GhosttyTerminal` context.
- The rendered terminal element declares `.key_context("GhosttyTerminal")`.
- The action handlers forward Tab (`\t`) and Shift+Tab (`ESC [ Z`) directly to the Windows terminal session.

## What We Learned

Special terminal keys are not ordinary keydown events in a GPUI app. Any key used by root focus navigation or application shortcuts must have an explicit terminal-context binding, otherwise it can be consumed before the terminal's byte encoder sees it.
