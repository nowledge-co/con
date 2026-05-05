# Quick Terminal

Quick Terminal is a macOS-only drop-down Con window for short terminal work
from anywhere on the system.

It is off by default. Turn it on only if you want a global shortcut that can
open a terminal over the app you are using.

## Turn It On

1. Open **Settings**.
2. Go to **Keys**.
3. Enable **Quick Terminal**.
4. Use the default <kbd>⌘</kbd> <kbd>Backslash</kbd> shortcut, or record a
   different shortcut if that conflicts with your setup.

The shortcut is global. It works when another app is frontmost, as long as Con
is running.

You can also open it from **View -> Quick Terminal** or the command palette
while Con is active, even if the global shortcut is off.

## How It Feels

Press the shortcut and Con slides a dedicated terminal down from the top of the
active display. Press it again and the window hides. When you hide it with the
shortcut, Con returns focus to the app you were using before.

Clicking another app also hides Quick Terminal. In that case macOS keeps focus
on what you clicked.

The window is intentionally simple:

- it has no titlebar or traffic-light buttons
- it spans the visible width of the active screen
- it starts at about half the screen height
- you can drag the bottom edge to change its height while it is alive

## What It Keeps

Quick Terminal is a real Con workspace. As long as you hide it instead of
closing it, it keeps the things you expect a live terminal to keep:

- tabs
- panes
- working directories
- running shells and TUIs
- visible scrollback

That makes it useful for small loops: check a server, run a one-off command,
peek at logs, or keep a scratch shell close without moving your main Con
windows.

## What Ends It

Hiding is not the same as closing.

If you close the last tab or exit the last shell, the Quick Terminal window is
destroyed. The next shortcut creates a fresh Quick Terminal from your home
directory.

Quick Terminal does not keep a separate saved layout, remembered height, or
private workspace after it is destroyed. If a workspace matters, keep it in a
normal Con window or save a [workspace profile](workspace-layout-profiles-guide.md).

## Quick Terminal vs Summon / Hide Con

Con has two macOS global-window features. They solve different problems.

| Feature | What it does | Use it when |
| --- | --- | --- |
| Quick Terminal | Opens a dedicated top-pinned terminal window. | You want an iTerm-style scratch terminal from any app. |
| Summon / Hide Con | Shows or hides the normal Con app window. | You want to jump back to your main Con workspace. |

Both are off by default because global shortcuts can collide with launchers,
window managers, and other terminal apps.

## If the Shortcut Does Nothing

- Make sure Con is running.
- Make sure **Settings -> Keys -> Quick Terminal** is enabled.
- Try a different shortcut if another app already owns
  <kbd>⌘</kbd> <kbd>Backslash</kbd>.
- If you are recording a shortcut in Settings, Con temporarily suspends its
  global hotkeys so the shortcut can be captured instead of triggered.

## Platform Support

Quick Terminal is macOS-only. Windows and Linux keep the normal Con window
model for now.
