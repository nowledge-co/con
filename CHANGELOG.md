# Changelog

All notable changes to con are documented here.

con is still pre-release, so entries may group related beta work while the product shape is stabilizing.

## [Unreleased]

### Improved

**Terminal — Windows Backend (preview)**
- Made Windows command rendering feel substantially closer to native terminals by closing the full PTY-to-paint loop: ConPTY output wakes are preserved, stale readbacks are discarded with mailbox semantics, row-local D3D readbacks stay row-local through GPUI image handoff, and delayed command-start output remains latency-critical long enough to present fresh frames.
- Aligned the default Windows shell choice with Windows Terminal when possible by reading its configured default profile before falling back to `pwsh.exe`, `powershell.exe`, `%COMSPEC%`, and `cmd.exe`; `CON_SHELL` remains the explicit override.
- Added `CON_LOG_FILE` so Windows terminal profiling can write con's own logs directly to a file without PowerShell `*>` redirection, avoiding redirected standard handles that can steal shell output away from the ConPTY pane.
- Fixed Windows ConPTY child process creation so shells do not inherit con's redirected stdout/stderr handles during profiling, which could send the PowerShell banner/prompt into `con-profile.log` instead of the terminal pane.
- Reduced redundant Windows pre-echo repaint work by letting handled keyboard input wait for actual VT/ConPTY progress instead of forcing a speculative GPUI repaint on every key press.
- Reduced Windows output batching by preserving successive ConPTY wake signals instead of draining them into a single repaint request before GPUI has had a chance to present intermediate progress.
- Reduced Windows slideshow-style command redraws by treating the staging ring as a true mailbox during PTY-driven output: once a fresher VT snapshot is submitted, older completed readbacks are no longer presented ahead of it.
- Tightened Windows staging mailbox behavior further so command bursts discard older in-flight readback cache entries and present the newest completed frame instead of replaying intermediate full-screen snapshots.
- Extended Windows interactive low-latency mode long enough to cover delayed command-start output, so shells and TUIs that begin painting a few hundred milliseconds after Enter can still take the freshest-frame path instead of falling back to delayed non-blocking readback.
- Reduced Windows input/readback latency by deriving exact changed VT rows even when libghostty-vt reports a full render-state dirty flag, then copying only those pixel rows from the D3D render target into the staging readback texture for small terminal updates.
- Reduced Windows readback cost with dirty-row D3D copies while preserving translucent-terminal correctness by replacing dirty rows in a CPU backing frame instead of alpha-blending row overlays over stale pixels.
- Expanded Windows profiling behind `CON_GHOSTTY_PROFILE` so one run now captures ConPTY read-chunk cadence, renderer sub-stage timings (drain/draw/submit/block-drain), `RenderSession::render_frame`, and the GPUI image-wrap stage alongside the shared VT snapshot timings; idle unchanged frames are filtered by default, with `CON_GHOSTTY_PROFILE_VERBOSE=1` available for every-frame traces.

## **v0.1.0-beta.38** - 2026-04-24

### Fixed

**Interface**
- Completed pane-close keyboard handling instead of only showing dead UI. `Close Pane` is now a real configurable shortcut in Settings, with app-level defaults that do not collide with terminal EOF (`Cmd+Opt+W` on macOS, `Alt+Shift+W` on Windows/Linux). Windows/Linux pane split defaults now use `Alt+D` / `Alt+Shift+D` instead of fragile symbol-based bindings.
- `Close Pane` now escalates cleanly when nothing smaller remains to close: last pane closes the tab, and the last tab quits the app.
- Fixed the last-pane quit path so quitting through `Close Pane` or terminal-exit escalation uses the same session save and surface teardown path as the normal app quit action.
- Fixed the command palette scroll behavior on all platforms. Mouse-wheel scrolling and scrollbar dragging no longer snap back to the selected row on every repaint.

**Terminal — Windows Backend (preview)**
- Reduced the first-frame flash when splitting a new pane on Windows. Brand-new panes now paint their configured terminal background while the first renderer image is still warming up, instead of briefly showing a transparent hole.

## **v0.1.0-beta.37** - 2026-04-24

### Improved

**Terminal — Linux Backend (preview)**
- Reduced Linux preview renderer CPU cost by caching per-row `StyledText` text/runs in `linux_view` and only rebuilding rows marked dirty by the VT snapshot, plus cursor-affected rows.
- Reduced Linux command-to-paint latency by waking the terminal view directly from PTY output instead of waiting for the workspace idle poll loop to discover new output.
- Added shared VT snapshot timing instrumentation behind `CON_GHOSTTY_PROFILE` so large command-start redraw costs on Windows/Linux can be measured directly.
- Documented the remaining Linux performance constraint explicitly: after the row-cache and direct-wake changes, the longer-term fix remains the planned glyph-atlas grid renderer and a lighter-weight shared VT snapshot contract.

### Fixed

**Release pipeline — multi-platform race**
- Fixed `irm https://nowled.ge/con-ps1 | iex` (and the `install.sh` equivalents) failing with `✗ no ZIP found for windows-x86_64` when the Linux release job won the create-and-upload race ahead of the Windows / macOS jobs. Each `release-{linux,macos,windows}.yml` now creates the GitHub release as `--draft`, so `/releases/latest` and the public REST API stay blind to the tag while assets are uploading. A new `release-finalize.yml` workflow watches all three platform workflows via `workflow_run` and atomically promotes the draft to public only once every platform reports a `success` conclusion for the same `head_sha`. If a platform fails, the draft stays drafted on purpose — better to block than to ship an incomplete release.

## **v0.1.0-beta.36** - 2026-04-24

### Fixed

- Windows con no longer be laggy!!!

## **v0.1.0-beta.35** - 2026-04-23

### Fixed

- Fixed Linux con cannot support older GLIBC

## **v0.1.0-beta.34** - 2026-04-23

### Added

**Terminal — Linux Backend (preview)**
- Added the first real Linux terminal pane: Unix PTY, shared `libghostty-vt` parser, GPUI styled-cell paint path, and the same app shell/control socket as macOS and Windows.
- Added Linux styled-cell rendering for SGR colors, bold, italic, underline, strikethrough, inverse, and block cursor state.
- Added Linux theme propagation from app settings into the VT parser at spawn time and during live theme changes.
- Added Linux client-side decorations, transparent rounded windows, and KWin Wayland backdrop blur where the compositor exposes it.

**Release and Packaging**
- Linux now has the same one-liner installer + auto-update story as macOS / Windows. The single `install.sh` (`curl -fsSL https://con-releases.nowledge.co/install.sh | sh`) detects the host OS and routes Darwin to the existing DMG flow or Linux to a new tarball flow that drops `con` into `~/.local/bin`, registers a `.desktop` launcher entry, and installs the 256x256 hicolor icon.
- A new `scripts/linux/release.sh` builds the artifact (`cargo build --release` with `CON_RELEASE_VERSION` / `CON_RELEASE_CHANNEL` baked in via `option_env!`, `--strip-debug`, stage + tar.gz + sha256), and a new `.github/workflows/release-linux.yml` mirrors the Windows release workflow: builds on `ubuntu-latest`, publishes the tarball to the same GitHub release the macOS / Windows jobs share, and updates the Sparkle-shaped appcast at `https://con-releases.nowledge.co/appcast/{channel}-linux-x86_64.xml` so the in-app notify-only updater can discover new builds.
- Added a notify-only Linux updater that polls the same Sparkle-shaped appcast XML the Windows backend uses, surfaces "Update now" in Settings → Updates, and re-runs `install.sh` with `CON_INSTALL_VERSION` pinned to the appcast's chosen version so beta-channel users do not silently get downgraded to stable when GitHub's `/releases/latest` skips prereleases. Sparkle itself stays macOS-only — Linux follows the Windows model (notify-only checker against the shared XML schema, then re-runs `install.sh` to apply).
- All three release workflows (`release-macos.yml`, `release-windows.yml`, `release-linux.yml`) now mark `v*-dev.*` tags as `--prerelease` when calling `gh release create`. Dev tags are internal smoke tests and must not appear in `/releases/latest`. Beta tags stay as regular releases for now because con is still in an all-betas era — the latest beta is what every fresh install should download. When stable v0.1.0 ships, `*-beta.*` will move under the same `--prerelease` rule so beta and stable channels stop colliding on `/releases/latest`.

### Improved

**Terminal — Linux Backend (preview)**
- Resolved IoskeleyMono font-family lookup on Linux so the embedded mono font is used instead of a proportional fallback.
- Reduced Linux terminal latency by replacing a full snapshot equality check with a generation-counter comparison and tightening the idle poll loop.

**Terminal — Windows Backend (preview)**
- Reduced Windows renderer hot-path work by skipping redundant default-background blank-cell instances, avoiding full-frame zero-fill before D3D readback, and only sorting the instance stream when a frame contains overflowing wide glyphs.
- Reduced Windows interactive terminal latency by marking user-driven updates latency-critical so the freshest submitted frame wins over stale readback, while keeping resize/fullscreen frames on the non-blocking staging path.

### Fixed

**Terminal — Linux Backend (preview)**
- Stopped the Linux "Waiting for shell prompt..." placeholder from flashing when long-lived shells enter alt-screen TUIs.

**Terminal — Windows Backend (preview)**
- Fixed Windows settings, theme, session, history, OAuth, conversation, and global skills paths so they no longer use `con` as a filesystem segment, avoiding Win32 reserved-device-name failures such as `os error 267`.
- Fixed Windows `con-ghostty` builds that failed in Ghostty/uucode with `uucode_build_tables.exe: FileNotFound` by defaulting Zig's global cache to a short path when `ZIG_GLOBAL_CACHE_DIR` is unset.
- Hardened Windows Zig cache selection so `con-ghostty` only auto-selects a short cache path when the chosen directory is actually writable, falling back cleanly instead of failing later inside Zig.
- Fixed Windows fullscreen/maximize redraws in alt-screen apps such as Neovim by forcing a full VT snapshot after resize/full invalidation instead of trusting per-row dirty flags for newly exposed rows.
- Fixed a Windows maximize/fullscreen hang where the first interactive redraw after a large resize could block the UI thread trying to rescue stale readback slots; the staging ring now uses mailbox semantics, stays non-blocking under true backlog, and still allows low-latency interactive presents when a clean slot is available.
- Fixed Windows resize/render-state drift by snapshotting Ghostty's actual render-state rows/cols during asynchronous resize catch-up and by including snapshot geometry in the renderer invalidation key so the corrected frame is not skipped as "unchanged".
- Fixed Windows VT resize to report Ghostty the renderer's real cell metrics instead of re-deriving per-cell pixels from pane size and grid count.
- Fixed a Windows input-latency hole where the low-latency hint could be consumed before ConPTY echo/prompt output advanced the VT; input-driven fast presents now stay armed until the next VT generation arrives.
- Fixed a Windows typing-latency cadence issue where sustained local input could fall back to stale-frame presents mid-burst; interactive low-latency mode now stays armed across a short typing/paste burst instead of only a single echoed generation.
- Fixed Windows terminal-pane bounds capture to measure a dedicated full-size wrapper during prepaint, avoiding zoom/maximize cases where sizing from image/layout children could stop the image quad short and hide the bottom command row.
- Reduced Windows steady-state input latency by moving terminal image generation out of the prepaint callback and into the main render path, removing an extra frame where freshly rendered images previously only became visible on the following paint.
- Reduced a Windows staging-ring latency edge where mailbox submission could miss an already-clean slot and unnecessarily disqualify the freshest-frame path during interactive typing.

**Interface**
- Fixed the macOS 12 fallback layering path. Monterey keeps an opaque top-level window to prevent desktop bleed-through, but the GPUI root above the embedded Ghostty `NSView` stays transparent so the terminal surface is not painted over by the fallback background.

### Documentation

- Updated the Linux implementation notes, tracker mirror, and postmortem for the first real Linux terminal preview.
- Updated build-system and Windows-port notes to document the shared `con-paths` crate and the current Windows renderer performance track.

## **v0.1.0-beta.33** - 2026-04-22

### Improved

**Terminal — Windows Backend (preview)**
- Tuned low-hanging renderer performance issues in the Windows D3D11/DirectWrite path.
- Wired terminal theme colors, opacity, and transparency into the Windows renderer.
- Hardened Windows resize/readback behavior, theme switching clear colors, and DWM backdrop behavior across the beta feedback loop.

## **v0.1.0-beta.32** - 2026-04-21

### Added

**Release and Packaging**
- Added Windows in-place auto-update flow and appcast support.

### Fixed

**Release and Packaging**
- Preserved the full beta tag in Windows release/appcast version metadata so beta update comparisons do not collapse to `0.1.0`.
- Made the PowerShell installer ASCII-safe for `irm | iex` usage.
- Updated release/download documentation for the renamed `con-terminal` repository.

## **v0.1.0-beta.31** - 2026-04-21

### Added

**Interface**
- Added a titlebar gear button on Windows and Linux for opening Settings.

**Release and Packaging**
- Added Windows PowerShell installer and release palette entries.

### Fixed

**Terminal — Windows Backend (preview)**
- Switched Windows builds to the console subsystem behavior needed for terminal process startup.
- Baked the full release channel/version into the Windows binary for update polling and user-visible version display.

## **v0.1.0-beta.30** - 2026-04-21

### Added

**Terminal — Windows Backend (preview)**
- Shipped the runtime-validated Phase 3b Windows glyph-atlas renderer: ConPTY plus `libghostty-vt`, D3D11/DirectWrite rendering, HLSL shaders, glyph atlas packing, Nerd Font/PUA handling, wide-glyph ordering, and cursor z-order handling.
- Promoted Windows from planned work to the first beta surface in the docs.

### Fixed

**Release and Packaging**
- Hardened the Windows release workflow against Defender interference, Zig/uucode spawn flakes, Windows runner instability, and MAX_PATH-sensitive cache paths.
- Renamed in-tree repository URLs from `nowledge-co/con` to `nowledge-co/con-terminal`.

## **v0.1.0-beta.29** - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Changed
- Renamed repository URLs to `nowledge-co/con-terminal`.

## **v0.1.0-beta.28** - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Fixed
- Pinned the Windows release runner to `windows-2022` and added on-failure diagnostics.

## **v0.1.0-beta.27** - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Fixed
- Retried Windows release builds around intermittent Zig/uucode spawn failures.

## **v0.1.0-beta.26** - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Fixed
- Disabled Defender real-time scanning during Windows release builds.

## **v0.1.0-beta.25** - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Added
- Advanced the Windows beta port toward the first published Windows preview.

## **v0.1.0-beta.24** - 2026-04-20

### Improved
- Shipped CI fixes, documentation updates, and broad UI/terminal polish before the Windows beta push.

## **v0.1.0-beta.23** - 2026-04-20

### Added
- Landed Windows support phase 0: non-macOS build scaffolding, portability gates, and an initial Windows-renderable terminal path.

## **v0.1.0-beta.22** - 2026-04-16

### Improved
- Shipped a broad optimization pass across terminal/app behavior.

## **v0.1.0-beta.21** - 2026-04-16

### Added
- Added Kimi provider support for coding workflows.

### Fixed
- Fixed Homebrew promotion and release workflow YAML issues.

## **v0.1.0-beta.20** - 2026-04-15

### Added
- Added Homebrew cask and a one-line install path.

### Improved
- Shipped another beta optimization pass and Zig CI hardening.

## **v0.1.0-beta.19** - 2026-04-15

### Fixed
- Polished terminal cursor behavior for CJK IME input.

## **v0.1.0-beta.18** - 2026-04-15

### Added
- Added early installer work and beta feature polish.

### Fixed
- Fixed Vim quit handling, IME text input issues, macOS hotkeys, terminal flashing, and Ghostty Zig CI pinning.

## **v0.1.0-beta.14** - 2026-04-14

### Improved
- Optimized settings, markdown table rendering, and CJK rendering paths.

## **v0.1.0-beta.3** - 2026-04-14

### Added
- Added configurable UI font size.

### Fixed
- Fixed a markdown code-block rendering performance issue.

## **v0.1.0-beta.1** - 2026-04-14

### Added
- Initial public beta of con.
- Added the first release workflow, app bundle/release infrastructure, documentation, and upstream dependency cleanup.

### Fixed
- Fixed early Sparkle startup/release-upload issues and ordered-list wrapping alignment.
