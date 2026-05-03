# Release Installer Gates

## What Happened

`con-cli` became part of the normal install contract, but the release pipeline
only verified parts of that contract. A platform workflow could succeed with a
bad artifact shape, or the shared draft release could be promoted before the
assets, appcasts, and gh-pages installer scripts were all aligned.

## Root Cause

The pipeline relied on several independent checks:

- macOS verified the app bundle, but Linux and Windows did not verify the
  installed artifact shape before upload.
- The finalizer only checked that the three platform workflows succeeded. It did
  not independently verify that the draft release and appcast state were safe
  for fresh installs and in-app updates.
- PR CI did not run for release-workflow/script-only changes unless crate files
  changed too.

## Fix Applied

- Added `scripts/release/verify-artifacts.sh` for macOS/Linux artifact contract
  checks.
- Added a Windows ZIP contract check in `release-windows.yml`.
- Added `scripts/release/verify-release-gate.sh`, called by
  `release-finalize.yml` before publishing a draft.
- Extended portable CI path filters and added script/parser checks so release
  workflow and installer script changes receive CI coverage in PRs.

## What We Learned

Release safety needs a final promotion gate, not just platform-local checks. The
public release boundary is where fresh installers and older clients converge, so
that boundary must verify the exact assets and appcast entries users will
consume.
