## What happened

`just macos-install` failed while building `con-ghostty`. The macOS build script
cloned the pinned Ghostty source successfully, but `zig build` failed while
fetching Ghostty's Zig package dependencies from `deps.files.ghostty.org`:

```text
error: bad HTTP response code: '400 Bad Request'
```

The first failing package was `uucode`, and subsequent retries showed the same
failure pattern for other Ghostty `build.zig.zon` URL dependencies.

## Root cause

The dependency URL itself was valid: `curl -I -L` returned `200 OK`, and
downloading the tarball with `curl` followed by `zig fetch <local-file>`
successfully inserted the package into Zig's global cache.

The failure was specific to Zig's direct HTTP package fetch path in this network
environment. Once the package cache was prewarmed from local files, the same
Ghostty `zig build` progressed past dependency resolution.

Ghostty also has nested Zig package dependencies. The root packages were not
enough; `vaxis` introduced a GitHub tarball dependency and a `git+https`
dependency that also needed to be cached before retrying the build.

## Fix applied

`crates/con-ghostty/build.rs` now retries macOS libghostty builds after an
automatic Zig package cache prefetch step:

- Scan Ghostty `build.zig.zon` files for URL dependencies.
- Download HTTP(S) tarballs with `curl`.
- Convert `git+https://repo#rev` package refs into temporary git archives.
- Insert each local archive into Zig's global package cache via `zig fetch`.
- Recursively scan fetched packages for nested `build.zig.zon` dependencies.
- Retry the original `zig build` after prefetching.

## What we learned

Zig package URLs can be valid while Zig's built-in fetcher still fails in a
specific proxy/CDN environment. For this build path, treating `curl` plus local
`zig fetch` as a retry fallback is more reproducible than asking developers to
manually prewarm `~/.cache/zig`.
