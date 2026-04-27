# Windows ConPTY start directory

## What Happened

Windows terminals launched from the installed app could start in Con's install directory instead of the user's normal shell home.

## Root Cause

The Windows backend created the ConPTY child with a null `lpCurrentDirectory`, so `CreateProcessW` inherited Con's process cwd. That is acceptable for developer runs, but wrong for installed app launches where the process cwd can be `AppData\Local\Programs\con-terminal`.

## Fix Applied

- Threaded the pane's requested cwd through `GhosttyView`, `RenderSession`, and `ConPty::spawn`.
- Validated explicit cwd values before passing them to `CreateProcessW`.
- Required fallback cwd values to be absolute directories, then fell back to the user's home directory and finally the system temp directory instead of inheriting the install dir.

## What We Learned

Windows process cwd is product-visible terminal state. Passing `NULL` to `CreateProcessW` is not neutral for a terminal emulator; it inherits launcher context, which may be an implementation directory rather than a human workspace.
