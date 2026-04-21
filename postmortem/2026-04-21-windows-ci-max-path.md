# Windows CI release — uucode_build_tables.exe `FileNotFound`

## What happened

The `Release Windows` workflow failed on every free GHA runner (both
`windows-latest`/Server 2025 and `windows-2022`) while building the
`con-ghostty` crate. Zig surfaced the error as:

```
error: failed to spawn and capture stdio from
  .\..\..\..\..\..\..\..\src\repo\target\release\build\
  con-ghostty-<hash>\out\ghostty-src\.zig-cache\o\<hash>\
  uucode_build_tables.exe: FileNotFound
```

Three separate attempts in a retry loop all failed with the same
error on the same cached exe hash, ruling out a visibility race.

## Root cause

`CreateProcessW` on Windows still honors the legacy `MAX_PATH` (260
char) limit unless the process is opted into long paths via both the
system registry **and** a manifest. Zig's build-host `build.exe` isn't
manifested for long paths in 0.15.2, so spawn calls whose composed
(CWD + relative path) crosses 260 bytes get rejected and surfaced as
`FileNotFound`.

In this pipeline the spawn was:

- CWD (set by uucode's `build.zig` via `run.setCwd(b.path(""))`):
  `C:\Users\runneradmin\AppData\Local\zig\p\uucode-0.2.0-<49-char-hash>`
  ≈ **96 chars**
- Relative exe path:
  `.\..\..\..\..\..\..\..\src\repo\target\release\build\con-ghostty-<16hex>\out\ghostty-src\.zig-cache\o\<32hex>\uucode_build_tables.exe`
  ≈ **167 chars**
- Composed length: **~264 chars** — just over the limit.

A diagnostic step re-ran the same exe by absolute path via `cmd /c`
and got a clean stack trace (it panicked later on a hardcoded
relative `ucd/UnicodeData.txt`), confirming the exe itself is fine
and the problem was purely in the spawn step path resolution.

The local Windows dev machine doesn't reproduce because its username
is shorter (`C:\Users\WeyGu\...` vs `C:\Users\runneradmin\...`),
putting composed spawn paths under 260.

## Fix applied

Set `ZIG_GLOBAL_CACHE_DIR=C:\zc` at the job level. That drops the CWD
from ~96 chars to ~55, bringing composed spawn paths comfortably
below `MAX_PATH`.

Also removed the retry loop (not a race, no reason to retry) and
relaxed the `windows-2022` runner comment (the pin isn't load-bearing
against this failure; it was pinned while we were chasing a Zig 0.15
vs Defender on Server 2025 theory that turned out wrong).

## What we learned

- "failed to spawn … FileNotFound" from zig on Windows is almost
  always `CreateProcessW` returning `ERROR_PATH_NOT_FOUND` /
  `ERROR_FILE_NOT_FOUND`, which can be triggered by path length even
  when the file exists and is executable.
- The on-failure diagnostic step (re-spawning the missing exe via
  `cmd /c` by absolute path, and dumping Defender/VS/OS state) was
  what made the root cause obvious — keep that pattern around for
  future Windows build regressions.
- Keep the Zig global cache short on CI. `%LOCALAPPDATA%` is too deep
  once you factor in the `p\<package>-<hash>\` layer plus any relative
  walks zig synthesizes from inside a dependency's build graph.
