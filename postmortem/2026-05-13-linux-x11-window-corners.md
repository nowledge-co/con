# Linux X11 Window Corners

## What happened

On Xfce/X11, Con opened with a sharp rectangular outer silhouette while Xfce
Terminal on the same display had rounded window-manager corners.

## Root cause

Con requested GPUI client-side decorations on Linux and rendered rounded
workspace content into a transparent top-level surface. Runtime logs confirmed
the rounded GPUI branch was active, but on X11/Xfce the visible outer silhouette
is owned by the window manager's server-side frame. Removing that frame meant
there was no native rounded shape left for the compositor to show.

## Fix applied

Linux now chooses decorations by compositor: Wayland keeps client-side
transparent rendering, while X11 uses server-side decorations and an opaque
window/root. The Linux in-app caption buttons and drag titlebar behavior are
rendered only when GPUI reports client-side decorations.

## What we learned

Rounded GPUI content is not the same as a rounded top-level X11 window. On X11,
native server decorations are the performance-safe path for matching the desktop
environment's window shape.
