# macOS Bundle Missing Ghostty Terminfo

## What happened

`just macos-install` produced a `/Applications/con.app` bundle whose embedded
terminal launched with `TERM=xterm-ghostty`, but the app bundle did not include
Ghostty's compiled terminfo database. Shells and terminal-aware programs could
not resolve the terminal capabilities, which showed up as missing colors,
broken prompt rendering, and cursor/input display glitches.

## Root cause

`scripts/macos/build-app.sh` copied `zig-out/share/ghostty` into
`Contents/Resources/ghostty`, but did not copy sibling
`zig-out/share/terminfo` into `Contents/Resources/terminfo`. Ghostty's macOS
runtime expects the resources directory to be `Contents/Resources/ghostty` and
sets `TERMINFO` to the sibling `Contents/Resources/terminfo`.

Con also set `GHOSTTY_RESOURCES_DIR` whenever `Contents/Resources/ghostty`
existed, even if the sibling terminfo directory was absent. That made the bad
bundle look valid to Ghostty instead of allowing a safer fallback path.

## Fix applied

- Copy Ghostty's `share/terminfo` into the macOS app bundle.
- Make macOS verification fail when no compiled `xterm-ghostty` entry exists
  anywhere under `Contents/Resources/terminfo`.
- Make runtime app-bundle resource discovery require the terminfo sentinel
  before setting `GHOSTTY_RESOURCES_DIR`.
- Added a regression test for a bundle with `Resources/ghostty` but no
  `Resources/terminfo`, plus coverage for alternate compiled terminfo buckets.

## What we learned

Ghostty's embedded resource directory and terminfo database are coupled by
layout, not by a single copied directory. Future packaging changes should treat
`Contents/Resources/ghostty` and `Contents/Resources/terminfo` as one runtime
contract and verify both before release or local install.
