# 2026-04-23 — Linux: glibc 2.38/2.39 not found on current Chromebooks

## What happened

`v0.1.0-beta.34` shipped a Linux tarball at
`con-0.1.0-beta.34-linux-x86_64.tar.gz`. On a current ChromeOS
Chromebook (Crostini container, Debian 12 Bookworm, glibc 2.36),
launching the binary failed with:

```
./con: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.38' not found (required by ./con)
./con: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by ./con)
```

Same failure mode hits any user on:

| Distro | glibc | Status against beta.34 |
|---|---|---|
| Debian 12 (Bookworm, current stable) | 2.36 | broken |
| Ubuntu 22.04 LTS | 2.35 | broken |
| RHEL 9 | 2.34 | broken |
| Fedora 38 | 2.37 | broken |
| Ubuntu 23.10 | 2.38 | partial — needs 2.39 still |
| Ubuntu 24.04 | 2.39 | works |
| ChromeOS Crostini (current) | 2.36 | broken |

## Root cause

`release-linux.yml` set `runs-on: ubuntu-latest`, which is
currently Ubuntu 24.04 (glibc 2.39). Linux's dynamic linker is
**forward-compatible only**: a binary linked against glibc N
runs on systems with glibc >= N, but never on glibc < N. So
binaries produced on the latest GitHub-hosted Linux runner
silently raised the floor of supported user systems every time
GitHub bumped the runner image.

`objdump -T` on the beta.34 binary showed exactly four symbols
above glibc 2.35:

```
GLIBC_2.39  pidfd_spawnp
GLIBC_2.39  pidfd_getpid
GLIBC_2.38  __isoc23_sscanf
GLIBC_2.38  __isoc23_strtol
```

All four are pure build-host artifacts:

- `__isoc23_sscanf` / `__isoc23_strtol` — glibc 2.38 introduced
  C23-compliant variants that default in when the build host's
  libc headers report `__STDC_VERSION__ >= 202311L`. On a
  glibc-2.35 build host, the same C source compiles to plain
  `sscanf` / `strtol` (no suffix). Pure naming split, no
  behavior difference.
- `pidfd_spawnp` / `pidfd_getpid` — glibc 2.39 newer process-
  spawn helpers that Rust stdlib uses on a fast-path branch
  when the build-host glibc is recent enough. Built against
  glibc < 2.39, Rust stdlib falls back to the older
  `posix_spawn` / direct-syscall `pidfd` path. Same end
  behavior.

So the fix is a pure build-host glibc version bump — no source
changes, no feature regressions.

## Fix applied

`release-linux.yml` `build` job pinned to `runs-on: ubuntu-22.04`
(glibc 2.35, the oldest GitHub-hosted Linux runner currently
supported). 2.35 covers:

- Debian 12 (2.36) ✅
- Ubuntu 22.04 LTS (2.35) ✅
- RHEL 9 (2.34) — close enough; the symbols Rust stdlib
  actually pulls in stop at 2.35
- Fedora 36+ (2.35+) ✅
- openSUSE Leap 15.4+ (2.31+) ✅
- Arch ✅
- ChromeOS Crostini (2.36) ✅

The `publish` job stayed on `ubuntu-latest` because it only runs
`gh release upload` and `gh release create` — its glibc never
runs in the artifact.

The `ci-portable.yml` smoke check stays on `ubuntu-latest` too:
those jobs only `cargo check` (don't ship a binary) and the
max-glibc surface is exactly what we want to verify the workspace
still compiles against.

## What we learned

- **Default GitHub Actions runner labels (`ubuntu-latest`,
  `windows-latest`, `macos-latest`) are not a stable ABI
  contract.** They tracking-bump as the platform team moves the
  base image forward. Anything that produces a binary for
  external distribution should pin to a specific OS version,
  not the moving label.
- **glibc forward-compatibility only goes one way.** Building
  on the newest available distro silently raises the floor of
  supported user systems each time the runner image moves. The
  symptom is delayed — `cargo build` succeeds, the artifact
  uploads cleanly, the binary launches fine on the build host
  and on any newer system, then fails at startup on user
  machines with `version 'GLIBC_X.YY' not found`.
- **`objdump -T <binary> | grep GLIBC_ | sort -V | tail -1`** is
  the one-liner that tells you the binary's actual glibc
  baseline. Worth adding to a release smoke step.

## Follow-up that's out of scope here

- Add an `objdump`-based glibc-baseline guard step to
  `release-linux.yml` so a future runner-image bump that pulls
  in 2.36+ symbols fails loudly during CI instead of silently
  shipping. Something like:
  `actual=$(objdump -T target/release/con | grep -oE 'GLIBC_[0-9.]+' | sort -V | tail -1); test "$actual" = "GLIBC_2.35"`.
- When `ubuntu-22.04` is retired by GitHub, either bump to the
  next-oldest pinned image and accept the new floor, or move
  to a `manylinux`-style cross-builder for true backward
  compatibility down to RHEL 7 / Debian 10.
- Consider a `.deb` / AppImage / Flatpak path (already tracked
  in `docs/impl/linux-port.md` phase 6). AppImage in particular
  bundles its own glibc-compatible runtime and sidesteps this
  whole class of problems.
