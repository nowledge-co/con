# Windows Config Save Reserved Name

## What happened

On Windows, saving settings failed with `os error 267` ("The directory name is invalid"). The visible failure was the settings panel banner when writing `config.toml`.

## Root cause

Several per-user storage paths used a literal `con` path segment, such as `%APPDATA%\con\config.toml` and `%LOCALAPPDATA%\con\session.json`. `CON` is a reserved DOS device name, so Windows path APIs can reject that segment even when it appears inside a longer absolute path.

## Fix applied

Windows now uses `con-terminal` for per-user app directories. macOS and Linux keep the existing `con` paths for compatibility.

The path policy lives in the shared `con-paths` crate so new call sites do not need to rediscover the Windows reserved-name rule. Updated storage paths include config, session state, global history, saved agent conversations, OAuth tokens, and user terminal themes. The settings panel custom-theme save path and skills preset also use the platform-safe directory.

## What we learned

Renaming the GitHub repository and Windows executable was not enough. Any filesystem path segment named `con` can still trigger reserved-name behavior on Windows, so new Windows storage paths must go through the shared app-path helper instead of hard-coding directory names.
