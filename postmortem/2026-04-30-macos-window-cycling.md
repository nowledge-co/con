# macOS Cmd-Backtick Window Cycling

## What happened

Cmd-Backtick and Cmd-Shift-Backtick were registered as GPUI actions, but they still failed when the embedded Ghostty terminal view was first responder. New workspace windows also opened centered at the exact same position, making multiple windows feel stacked instead of naturally cascading.

## Root cause

Window cycling is a native macOS window-management shortcut. Routing it only through GPUI is not sufficient when a native child NSView participates in key-equivalent handling before GPUI sees the event. The new-window placement issue came from Con overriding GPUI's default window placement with a fixed centered `WindowBounds`.

## Fix applied

Con now installs a macOS-only local AppKit key monitor for Cmd-Backtick. The monitor handles the shortcut before the terminal surface can consume it and cycles visible keyable app windows using a short-lived cycle-order snapshot, so repeated presses walk all windows instead of toggling only the top two.

Workspace window bounds now cascade from the active window and wrap inside the visible display area while preserving the first-window 1200x800 default.

## What we learned

App-wide macOS window-management shortcuts should be handled at the native window layer when Con embeds native platform views. GPUI actions are still useful for menu display and non-native focus paths, but they cannot be the only enforcement point for native app behavior.
