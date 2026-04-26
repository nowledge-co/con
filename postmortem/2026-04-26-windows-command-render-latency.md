# Windows command render latency

## What Happened

The Windows backend felt close for individual key echo after the first
renderer tuning pass, but real command output still arrived in visible
"slides": `ls` / `dir` would show one batch, then another batch, then the
prompt. `clear` could hide the prompt, clear the screen, and then show the
prompt later. Neovim also exposed this because prompt/status-line redraws
make missing or stale final rows obvious.

The first profiles were noisy because shell output could be redirected
into the profiling log when con itself was launched with PowerShell
redirection. After adding `CON_LOG_FILE` and preventing the ConPTY child
from inheriting con's redirected stdout/stderr handles, the profile showed
the real chain:

- `vt_snapshot` and D3D draw were usually sub-millisecond.
- dirty-row readback was working for small changes;
- command bursts still replayed stale completed readbacks after fresher VT
  snapshots had already been submitted;
- GPUI image handoff could still turn one changed row into a full-surface
  upload;
- some command output arrived 400-500 ms after Enter, outside the first
  short low-latency hint window.

Separately, shell comparison was not controlled. Windows Terminal opens
the configured `defaultProfile`; con used its own fallback order and the
user's logs showed `powershell.exe`. That can make prompt timing look like
renderer latency when the shells, profiles, or prompt frameworks differ.

## Root Cause

There was no single bottleneck. The visible latency came from several
small queueing mistakes across the end-to-end path:

1. **Profiling path contaminated the shell.** Redirecting con's stdio in
   PowerShell let the child shell inherit redirected handles unless
   `CreateProcessW` disabled inheritance. That could move prompt output
   from the pane into the log.
2. **Render readbacks behaved like a queue, not a mailbox.** Once fresher
   VT output was submitted, older completed readbacks could still be
   presented first, producing the visible "slide" effect.
3. **Small updates still paid full-image handoff cost.** Dirty-row D3D
   readback helped, but GPUI still received new full terminal images until
   the row-strip overlay path landed.
4. **Low-latency arming was too short for real commands.** The first
   latency-critical window covered immediate echo but not delayed command
   start / prompt redraw output.
5. **The comparison shell was not necessarily Windows Terminal's shell.**
   Without honoring Windows Terminal's `defaultProfile`, con could test
   against `powershell.exe` while Windows Terminal used PowerShell 7, cmd,
   WSL, or a custom profile.

## Fix Applied

- Added `CON_LOG_FILE` and changed ConPTY child creation to avoid inheriting
  redirected stdout/stderr handles.
- Preserved successive ConPTY wake signals instead of draining them into
  one repaint request.
- Changed staging readbacks to true mailbox semantics: when fresher VT
  output has been submitted, stale completed readbacks are discarded rather
  than presented.
- Derived exact changed VT rows and copied only those D3D pixel rows for
  small updates.
- Kept a full base GPUI image and layered row-strip patch images for
  row-local changes.
- Extended the interactive low-latency window so delayed command-start
  output can still take the freshest-frame path.
- Aligned the default Windows shell with Windows Terminal where possible:
  `CON_SHELL` still wins, then con reads Windows Terminal stable/preview
  settings and resolves `defaultProfile`, then it falls back to
  `pwsh.exe`, `powershell.exe`, `%COMSPEC%`, and `cmd.exe`.

## Validation Notes

The follow-up profile after default-profile shell resolution confirmed con
was no longer guessing the shell:

```text
using Windows Terminal default profile command ... settings.json:
C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe
```

The same run showed the intended steady-state shape:

- one-line interactive updates used row-strip readback / overlay handoff
  (`readback_rows=35`, `patches=1 overlays=1`) and usually landed around
  1-3 ms end-to-end inside con's measured render path;
- medium command output stayed row-local when possible (`readback_rows=70`
  or `875`, multiple patches/overlays) rather than rebuilding the full
  surface for every chunk;
- full or near-full redraws still cost several milliseconds because they
  necessarily travel through the temporary GPU→CPU→GPUI image path.

That matches the architecture: the user-visible "slides" were removed by
mailbox + row-patch behavior, while the remaining ceiling is the known
direct-composition follow-up.

## What We Learned

- Terminal performance has to be profiled as an end-to-end pipeline:
  key/input, ConPTY chunk cadence, VT snapshot, renderer submit/readback,
  image handoff, and GPUI repaint. Optimizing only one stage can move the
  bottleneck without changing what the user feels.
- For interactive command output, the freshest frame matters more than
  delivering every intermediate frame. Mailbox semantics are the right
  default for PTY-driven redraws.
- A dirty-row renderer is incomplete unless the UI handoff is also
  row-local. Otherwise one changed command line still becomes a full-frame
  CPU clone and upload.
- Performance comparisons must control the shell and prompt. PowerShell
  profiles, oh-my-posh/starship hooks, WSL startup, and `pwsh.exe` vs
  `powershell.exe` can easily account for hundreds of milliseconds before
  con's renderer is involved.
- The long-term architecture is still direct swap-chain composition into
  GPUI's DirectComposition tree. The row-patch path is the right tactical
  bridge because it removes the visible latency without changing terminal
  semantics or blocking the upstream composition work.
