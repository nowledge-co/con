# Sparkle Auto-Update Release Pipeline — First Release Issues

**Date:** 2026-04-14
**Severity:** Release-blocking
**Affected:** v0.1.0-beta.1 (first CI release)

## What happened

The first beta release (`v0.1.0-beta.1`) hit four sequential failures before producing a working build:

1. **Ghostty DockTilePlugin build failure** — Zig build invoked `xcodebuild -target Ghostty` which compiled Swift targets we don't need. DockTilePlugin.swift failed on Xcode 16.4.
2. **Publish job missing git context** — `gh release create` failed because the publish job didn't check out the repo.
3. **Sparkle framework symlink dereferencing** — The app downloaded from CI artifacts (zip) had a broken Sparkle.framework with symlinks replaced by real directories, causing Gatekeeper to reject it as "ambiguous bundle format."
4. **App crash on launch** — The broken Sparkle framework caused an unrecoverable panic during startup that propagated across the ObjC FFI boundary.

## Root causes

### Ghostty xcodebuild
`-Dxcframework-target=native` triggers xcodebuild for the full Ghostty macOS app target, including Swift UI code. We only need `libghostty-fat.a` which is produced by the libtool step before xcodebuild runs.

**Fix:** Add `-Demit-macos-app=false` to the zig build args.

### Missing checkout
The publish job ran on `ubuntu-latest` with only `actions/download-artifact` — no `actions/checkout`. The `gh` CLI needs a git repo to know which GitHub repo to operate on.

**Fix:** Add `actions/checkout@v4` to the publish job.

### Symlink dereferencing
`actions/upload-artifact` preserves files but can dereference symlinks. The zip and DMG files themselves are correct (created before upload), but extracting the zip with macOS Archive Utility also dereferences symlinks. Users must use the DMG for installation.

Framework symlinks are a macOS requirement: `Sparkle.framework/Sparkle` must be a symlink to `Versions/Current/Sparkle`, and `Versions/Current` must be a symlink to `Versions/B`. Without this structure, Gatekeeper sees an ambiguous bundle.

**Fix:** DMG is the primary distribution format; zip is for CI/automation only. Additionally hardened the updater init to never crash.

### FFI panic propagation
The Sparkle init code ran inside GPUI's `did_finish_launching` callback. A panic in this context crosses the ObjC/Rust FFI boundary, turning into `panic_cannot_unwind` → `SIGABRT`.

**Fix:** Wrapped `updater::init()` in `catch_unwind`. Also added nil checks on every ObjC return value (alloc, updater property).

## Additional issues found during review

- `sign_update -s` flag is deprecated in Sparkle 2.9.x and doesn't work with newly generated keys. Switched to `--ed-key-file -` (stdin).
- `decode_base64_to_file()` had a TOCTOU race: wrote the file before setting `chmod 600`. Fixed to pre-create with `install -m 600`.
- CI appcast step had no validation that the Ed25519 signature was actually extracted from `sign_update` output.
- gh-pages push had no pull-rebase, risking failures on concurrent releases.

## What we learned

1. **Never trust xcodebuild in library builds.** When building a C library from a project that also has a macOS app, explicitly opt out of the app target. Zig's `-Demit-macos-app=false` was the right lever.
2. **DMG is the only reliable macOS distribution format.** Zip files lose framework symlinks. Serve DMG to users; zip is for programmatic consumption only.
3. **ObjC FFI must be nil-checked at every level.** `catch_unwind` doesn't catch ObjC exceptions — the only defense is defensive nil checking on every `msg_send!` return.
4. **First-release CI always has latent failures** — each job should be individually testable. The publish and update-appcast jobs should have been tested with a dry-run tag before the real release.
5. **Sparkle's deprecated flags still appear in examples.** Always check `--help` on the actual downloaded binary, not old blog posts.
