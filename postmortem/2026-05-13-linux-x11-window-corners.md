# Linux X11 Window Corners

## What happened

On Xfce/X11, Con opened with a sharp rectangular outer silhouette while Xfce
Terminal on the same display had rounded window-manager corners.

## Root cause

Con requested GPUI client-side decorations on Linux and rendered rounded
workspace content into a transparent top-level surface. Runtime logs confirmed
the rounded GPUI branch was active, but on X11/Xfce the visible outer silhouette
is the top-level X window shape, not the inner GPUI clip. Removing the native
frame meant there was no window-manager rounded shape left for the compositor to
show.

## Fix applied

Linux keeps Con's client-side chrome. Wayland still uses the transparent GPUI
surface and rounded workspace clip; X11 additionally applies a server-side
SHAPE mask matching the outer rounded frame, updating only when window
size/maximize/tiling state changes. The Linux wrapper also reserves a small
transparent frame and paints an opacity-based halo/outline around the rounded
workspace body, giving the main window visible depth without adding a second
native titlebar.

## What we learned

Rounded GPUI content is not the same as a rounded top-level X11 window. On X11,
an explicit shape mask is the performance-safe path for matching the desktop
environment's rounded windows while preserving client-side chrome.
