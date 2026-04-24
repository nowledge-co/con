# Multi-platform release race — fresh installs see a tag with only one platform's assets

Date: 2026-04-23
Tag affected: v0.1.0-beta.35 (and every prior multi-platform release; previously masked by the Windows / macOS runs typically finishing within seconds of Linux)
Branch: `perf`

## What happened

A Windows user ran the documented one-liner
```
irm https://nowled.ge/con-ps1 | iex
```
shortly after `v0.1.0-beta.35` was tagged and the GitHub Actions release runs started. The installer printed:

```
   █▀▀ █▀█ █▄ █
   █▄▄ █▄█ █ ▀█

   ✗  no ZIP found for windows-x86_64
```

`install.ps1` resolves the latest tag through GitHub's `/releases/latest` API and then looks for a release asset whose name matches `con-*-windows-x86_64.zip`. The API returned the tag for `v0.1.0-beta.35`, but the only asset attached at that moment was the Linux tarball (`con-0.1.0-beta.35-linux-x86_64.tar.gz`).

## Root cause

The three release workflows — `.github/workflows/release-{linux,macos,windows}.yml` — are independent. Each is triggered by `push: tags: v*` and each `publish` job ran:

```bash
if ! gh release view "$tag" >/dev/null 2>&1; then
  gh release create "$tag" --title "$tag" --generate-notes "${prerelease_args[@]}"
fi
gh release upload "$tag" "$file" --clobber
```

The first platform to finish its build won the create call, and the release was published immediately as a *public, non-draft* release. From that instant, GitHub's `/releases/latest` returned the tag and `install.ps1` / `install.sh` happily resolved it — even though the other two platforms were still building, packaging, signing, and uploading their assets.

For most beta releases the slowest platform (Windows) finishes within a couple of minutes of the fastest (Linux), so the race window is tight. On `v0.1.0-beta.35` it just happened to land between the Linux upload completing and the Windows upload completing, and a real user hit it.

The Linux build job runs on `ubuntu-22.04`, has a small artifact, and consistently finishes first. Windows runs on `windows-latest`, signs and packages, and consistently finishes last. So the bug was always going to bite — it just bit on `v0.1.0-beta.35` specifically.

## Fix

Switch from "the fastest platform publishes the release" to "the release is staged as a draft and only promoted to public once all three platforms have succeeded".

1. `release-{linux,macos,windows}.yml` now create the release with `--draft`:
   ```bash
   gh release create "$tag" --title "$tag" --generate-notes \
     --draft "${prerelease_args[@]}"
   ```
   Drafts are invisible to `/releases/latest`, the public REST API, and the public UI listing. `install.sh` / `install.ps1` behave as if the tag does not exist, which is the correct answer while the release is still being assembled.

2. New workflow `.github/workflows/release-finalize.yml` triggers on `workflow_run.completed` for any of the three release workflows. For each completion it:
   - Filters out non-tag-push events.
   - Queries `repos/.../actions/workflows/release-{linux,macos,windows}.yml/runs?head_sha=<sha>&event=push` for each sibling and counts runs with `conclusion == "success"`.
   - If all three siblings have at least one success for the same `head_sha`, calls `gh release edit "$tag" --draft=false`.
   - Otherwise exits cleanly — the next sibling's completion will fire another finalize attempt.

3. Concurrency is bounded with a per-tag concurrency group so two near-simultaneous sibling completions don't race the gate computation. `gh release edit --draft=false` is itself idempotent on an already-published release, so this is defence-in-depth, not correctness.

## Why not "delay the publish step"?

A simpler-looking fix is to add a "wait for all sibling workflows to complete" loop inside one of the existing publish jobs. That doesn't work because each workflow only knows about its own runs at the time it's running — there is no place inside `release-windows.yml` where it can correlate against `release-linux.yml`'s run ID without opening a separate API client and polling, which is exactly what `workflow_run` does for free at the platform level.

It also doesn't compose: if the slowest platform fails, the other two would block forever waiting for assets that will never appear, while today's draft-then-promote shape just leaves the release as a draft and lets the maintainer decide whether to re-run or hand-promote.

## Why not "make `install.ps1` tolerate missing assets"?

The installer would have to walk `/releases` (plural), filter by per-platform asset presence, and pick the newest release that has the right asset. That trades one bug for several:
- Adds API pagination cost on every install.
- Lets a partial release become "the latest install" forever if the platform with the missing asset never re-runs successfully (silent skew between what shows on GitHub vs. what new users get).
- Doesn't match user expectation: if the GitHub release page shows a tag, the one-liner should install that tag.

The draft-then-promote approach makes "what the user sees" and "what the installer resolves" the same thing again.

## What we learned

- Treat the moment a release becomes visible at `/releases/latest` as a public commitment, not a side effect of whichever CI job finishes first.
- `gh release create --draft` is the right primitive for staging multi-platform releases. It's invisible to the public API surface (`/releases/latest`, REST `/releases`, the GitHub UI release list) without being mislabelled as a "Pre-release", which has different downstream semantics (Sparkle channels, RSS readers, anyone keying off `prerelease == true`).
- `workflow_run` is the right primitive for cross-workflow coordination on the same tag/sha. The constraint that it only fires on the default branch is fine for our case: tags only get pushed against published code, and the finalize workflow file lives in the default branch.
- Treat "incomplete release stays as a draft" as the correct failure mode: better to block a broken release than to ship one and have the one-liner explode for half the audience.
