# Tab cwd restore fell back to home on macOS protected folders

## What happened

After the first restorable-workspace slice, terminal text and tab titles
could come back correctly while the live shell reopened in `~`. A common
repro was:

1. Open Con.
2. `cd ~/Documents` or `cd ~/Downloads`.
3. Quit and relaunch.
4. The old transcript still showed the protected directory, but the new
   prompt was back at `~`.

This made restore look like it had remembered the screen but not the shell.

## Root Cause

There were two separate state sources:

- **Grounded cwd:** Ghostty shell integration reports OSC 7/PWD. Con stores
  that as the terminal cwd.
- **Restored text:** Con seeds previous screen text into Ghostty's terminal
  parser before starting the new shell. That text is continuity, not state.

An attempted fallback parsed restored prompt text to infer cwd. That was wrong:
in an E2E test, `tab.cwd` was correctly `/Users/weyl/Documents`, but the
restored text still contained an older `/tmp` prompt. The prompt parser wrote
the older path into `layout.surfaces[].cwd`, and restore uses the layout
surface cwd. The guessed value beat the real PWD value.

After removing that guess, a deeper macOS issue remained. Launching a restored
pane in `/tmp` worked, but launching it in `/Users/weyl/Documents` or
`/Users/weyl/Downloads` still landed in `~`. Ghostty's embedded surface path
preflight called `openDirAbsolute`/`stat`, and its exec path called
`access`, before handing cwd to the child process. macOS privacy controls can
deny those directory read/metadata checks for Documents/Downloads even though
the shell can still `chdir` there and later report PWD. Ghostty rejected the
cwd during preflight and fell back to home.

## Fix

- `pane_tree::to_state` no longer infers cwd from restored transcript text.
  Surface cwd is now the reported terminal cwd, with only the deterministic
  initial cwd fallback already owned by the terminal view.
- `GHOSTTY_ACTION_PWD` remains a typed `GhosttySurfaceEvent::PwdChanged`, and
  the workspace persists immediately when it arrives.
- The embedded Ghostty build patch now treats macOS cwd preflight differently:
  for `apprt/embedded.zig` and `termio/Exec.zig`, Con trusts the app-provided
  cwd on Darwin and lets process spawn be the authoritative failure boundary.
  Non-macOS behaviour is unchanged.

## Validation

An isolated macOS E2E used private socket/session/history paths:

1. Start Con in `/tmp`.
2. Run `cd /Users/weyl/Documents` through `con-cli`.
3. Confirm `tree.get` reports `/Users/weyl/Documents`.
4. Quit Con.
5. Confirm `session.json` has `/Users/weyl/Documents` in `tab.cwd`,
   layout leaf cwd, and surface cwd.
6. Relaunch the same isolated session.
7. Confirm `tree.get` reports the live shell cwd as `/Users/weyl/Documents`.

The same direct-launch check (`con /Users/weyl/Documents`) now opens the live
shell in Documents.

## What We Learned

- Restored terminal text must never be used as a source of truth for cwd.
  It is visual continuity only.
- The authoritative cwd source is terminal protocol state, not prompt shape.
- macOS privacy-protected directories can make directory preflight stricter
  than the child shell's actual ability to start there. For app-captured shell
  cwd, process spawn is the correct boundary.
- When session restore stores both tab-level and layout-level cwd, the layout
  surface value must be as trustworthy as the tab value because restore uses
  the layout tree first.
