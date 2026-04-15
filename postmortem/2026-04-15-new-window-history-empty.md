# New Windows Lost Command History

## What happened

Fresh Con windows opened from Cmd+N, Dock reopen, global summon, or Cmd+T with no active window started with an empty command-bar history, even though relaunching the app restored persisted session state.

## Root cause

Those fresh-window paths intentionally used `Session::default()` to avoid cloning old tabs and pane layouts into every new window. That also discarded the global command histories because layout state and global input history live in the same session snapshot.

## Fix applied

Fresh-window creation now builds a default layout while copying persisted global shell and input histories from the saved session. If the newer input-history field is empty, it falls back to persisted shell command history so older session files still provide recall.

## What we learned

Session data has two different lifetimes: window layout is per-window, while command history is global app memory. New-window code should preserve global memory without restoring old window topology.
