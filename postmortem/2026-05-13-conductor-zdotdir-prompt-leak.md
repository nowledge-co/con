# Conductor ZDOTDIR Prompt Leak

## What happened

A development build of Con launched from a Conductor-managed environment opened
new terminal panes with the bare zsh `%` prompt instead of the user's configured
prompt. Restarting the app from a clean environment restored normal behavior.

## Root cause

The app process inherited Conductor's temporary zsh integration `ZDOTDIR`. Con
then loads the login-shell environment during startup and Ghostty child shells
inherit the resulting process environment. That allowed a wrapper app's shell
bootstrap directory to become Con's terminal `ZDOTDIR`, so new panes could start
with the wrong shell initialization context.

## Fix applied

Con now sanitizes only this specific inherited Conductor `ZDOTDIR` case during
startup, before and after login-shell environment loading. When
`ZDOTDIR == CONDUCTOR_INTEGRATION_ZDOTDIR`, Con restores
`CONDUCTOR_USER_ZDOTDIR` or `CONDUCTOR_ORIGINAL_ZDOTDIR`; if neither exists, it
removes `ZDOTDIR`. A user's intentional custom `ZDOTDIR` is left untouched.

## What we learned

Loading a GUI app's login-shell environment is useful, but terminal apps must be
careful about inheriting shell-integration variables from the launcher itself.
Wrapper-owned bootstrap paths are control-plane state, not user shell
configuration, and should not be propagated into terminal child shells.
