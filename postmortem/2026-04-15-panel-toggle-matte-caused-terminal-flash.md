# Panel toggle matte caused terminal flash

## What happened

Hiding or showing the bottom input bar and side agent panel caused the terminal area to flash.

## Root cause

Con used a short full-terminal layout matte during chrome transitions to hide earlier transparent-edge artifacts. That fixed one class of visual leak, but it also painted a temporary veil over the entire terminal surface during ordinary panel and input-bar toggles.

Because Ghostty renders underneath GPUI, that matte read as a terminal flash.

## Fix applied

Removed the full-terminal matte from agent-panel and input-bar hide/show transitions. Other layout transitions still keep their existing matte behavior for now.

## What we learned

Covering the whole terminal is too blunt for routine chrome animation. Edge artifacts should be solved at the moving seam or chrome surface, not by veiling the terminal content.
