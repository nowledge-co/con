## What happened

CI failed while building `con-ghostty` because its build script assumed a local Ghostty checkout at `3pp/ghostty`.

That violated the repository rule that `3pp/` is read-only reference material and not part of the real build graph.

## Root cause

`crates/con-ghostty/build.rs` hardcoded:

- `../../3pp/ghostty`
- source change tracking against `../../3pp/ghostty/src`
- error messages instructing developers to build from `3pp/ghostty`

That worked on a workstation with a manual local checkout, but it was not reproducible in CI or any clean environment.

## Fix applied

- Removed the hard dependency on `3pp/ghostty`.
- `con-ghostty` now resolves Ghostty source in this order:
  1. `CON_GHOSTTY_SOURCE_DIR` if explicitly provided
  2. otherwise clone the pinned upstream Ghostty revision into `OUT_DIR`
- Kept the Ghostty revision pinned for reproducibility.
- Verified the full app builds without any local `3pp` Ghostty checkout.

## What we learned

- Read-only source mirrors under `3pp/` must never be treated as build inputs.
- If a non-Rust dependency must be built from source, the build script needs either:
  - an explicit environment override, or
  - a deterministic upstream fetch path
- CI is the quickest way to detect when “local reference only” has quietly turned into an undeclared build dependency.
