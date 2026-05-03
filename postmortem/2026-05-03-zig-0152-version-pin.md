# Zig 0.15.2 Must Be Treated as a Pin

## What happened

Issue #117 reported that `HACKING.md` documented Zig as `0.15.2+`.
That made a fresh contributor reasonably install the latest Zig
(`0.16.0` at the time), but the pinned Ghostty revision does not build
with Zig 0.16's updated build APIs.

The symptom is a `con-ghostty` build failure in Ghostty's `build.zig`
or `src/build/zig.zig`, even though the installed Zig is newer than
the documented minimum.

## Root cause

`0.15.2+` was written as if Ghostty's `minimum_zig_version = "0.15.2"`
meant semver-compatible forward support. Zig build APIs are not stable
that way. For this pinned Ghostty revision, `0.15.2` is not just a
minimum; it is the version we have validated in CI and release builds.

CI already encoded the correct behavior:

- `release-macos.yml` installs Zig 0.15.2.
- `release-linux.yml` installs Zig 0.15.2.
- `release-windows.yml` installs Zig 0.15.2.
- `ci-portable.yml` installs Zig 0.15.2 for the Linux smoke check.
- Windows PR smoke checks skip `libghostty-vt` with
  `CON_SKIP_GHOSTTY_VT=1`, so they do not validate local full-build
  Zig compatibility.

The contributor guide drifted from CI and release reality.

## Fix applied

`HACKING.md` now says to use Zig 0.15.2 exactly, explains that Zig
0.16.0 is currently incompatible, and points contributors to the
official 0.15.2 archive plus `CON_ZIG_BIN` for non-default installs.

`CLAUDE.md` and `docs/impl/windows-port.md` were aligned so the
project development guides do not keep advertising `0.15.2+`.

## What we learned

- Toolchain docs should describe the version CI actually runs, not the
  broadest version a third-party project appears to declare.
- Zig should be treated as a pinned build-tool dependency for Ghostty
  until we intentionally bump Ghostty and validate a newer Zig.
- Windows PR `cargo check` is intentionally faster than a full release
  build because it skips `libghostty-vt`; HACKING needs to make that
  limitation explicit so contributors do not infer release readiness
  from that smoke check alone.
