# Windows release Zig help probe package fetch failure

## What happened

The `v0.1.0-beta.75` Windows release workflow failed while building
`con-ghostty`. The build script ran `zig build -h` to detect the pinned
Ghostty checkout's libghostty-vt build surface, but Zig attempted to resolve a
package dependency during help generation and failed with:

```text
invalid HTTP response: HttpConnectionClosing
```

The build script then treated the failed help output as normal help text,
could not find the libghostty-vt option/step, and panicked with the misleading
"couldn't find a libghostty-vt build knob" message.

## Root Cause

`pick_vt_invocation` only handled spawn failures for `zig build -h`; it did not
check whether the command exited successfully. Any non-zero help probe output
was parsed as if it were valid help. That made transient Zig package-fetch
failures look like upstream Ghostty had removed or renamed the VT build knob.

The macOS full-libghostty build already retried after prefetching Zig package
dependencies, but the Windows/Linux libghostty-vt path did not use that retry
strategy for either the help probe or the actual VT build.

## Fix Applied

- `zig build -h` now checks the exit status.
- A failed help probe prefetches Zig package dependencies into the active global
  cache, then retries once.
- If the retry still fails, the panic now reports a help-probe/package
  resolution failure instead of claiming the VT build knob is missing.
- Windows/Linux libghostty-vt builds now also retry once after the same package
  cache prefetch.

## What We Learned

Zig help generation can touch package resolution for upstream projects, so
release build probes must be treated as real build-system commands with
network/cache failure modes. Probe failures should be classified before parsing
their output; otherwise a transient dependency fetch turns into a false design
diagnosis and blocks release triage.
