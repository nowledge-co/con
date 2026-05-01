# Changelog

All notable changes to con are documented here.

con is still pre-release, so entries may group related beta work while the product shape is stabilizing.

## `origin/main`

## `v0.1.0-beta.50` - 2026-05-01

**Terminal, macOS**

- Now zoom window can be done double-click on titlebar
- Zoomed window fixed buttom one pixel spacing leaked

### Fixed

## `v0.1.0-beta.49` - 2026-05-01

### Fixed

**Terminal, macOS**

- Fixed macOS window zoom/fullscreen behavior so Con no longer leaves a bottom gap from terminal cell-sized AppKit resize increments, and restored double-click titlebar zoom behavior on the in-app tab bar.
- Improved macOS Monterey fallback for the embedded Ghostty terminal. On macOS 12 and older, Con now explicitly keeps Ghostty's hosted IOSurface layer geometry synchronized with the native surface view, addressing reports where old macOS showed an opaque black terminal area but no visible output. Modern macOS keeps Ghostty's existing layer ownership unchanged. Tracks [#20](https://github.com/nowledge-co/con-terminal/issues/20).

## `v0.1.0-beta.48` - 2026-05-01

### Fixed

**Terminal, Linux Backend (preview)**

- Fixed Linux release artifacts so `libghostty-vt` is built for a generic Zig target instead of the CI runner's native CPU. This avoids `Illegal instruction` startup crashes on WSL and older x86_64 hosts whose CPUs do not expose the same instruction-set extensions as the release builder. Fixes [#97](https://github.com/nowledge-co/con-terminal/issues/97).

**Agent Panel**

- Fixed inline LaTeX in markdown responses so `$...$` formulas are typeset through the same cached SVG pipeline as block math instead of being shown as raw formula text. Inline formulas now inherit the surrounding text color and sit inside the prose line box for cleaner spacing. Fixes [#88](https://github.com/nowledge-co/con-terminal/issues/88).

## `v0.1.0-beta.47` - 2026-05-01

### Added

**Terminal, Windows and Linux Backends**

- Added CJK IME text input for Windows and Linux terminal panes. IME commits now enter through GPUI's platform text-input path, preedit state tracks candidate positioning at the terminal cursor, and terminal-local control / alt / special key handling remains unchanged. The macOS embedded Ghostty path is unchanged.

**Workspace**

- Added a quick vertical-tabs toggle in the top bar, Command Palette, and Keyboard Shortcuts. Defaults: Cmd+B on macOS, Ctrl+Shift+B on Windows and Linux.
- Added draggable resizing for the pinned vertical-tabs panel. The resized width is persisted in session state and restored across launches.

**Control Plane**

- Added pane-local terminal surfaces for external orchestrators. Existing `panes.*` APIs, built-in agent tools, and benchmarks keep the active-surface pane contract, while new `tree.get` and `surfaces.*` RPC/CLI commands can create, split, wait for readiness, focus, rename, drive, read, and close terminal sessions inside a pane.

### Fixed

**Terminal, macOS**

- Reduced transparent-window flashes along moving chrome seams. Agent-panel transitions, input-bar transitions, the top-bar transition, the vertical-tabs edge, the input-bar edge, and pane dividers now use tiny opaque terminal-colored seam covers on macOS instead of exposing a transparent gap or UI-colored matte.
- Restored visible macOS terminal pane dividers without reopening the transparent-window leak path. Split separators are now a subtle opaque foreground tint precomposed over the terminal background, so users can read pane boundaries while every separator pixel still stays fully opaque.
- Polished the standalone Settings window's `Save Changes` action so it reads as compact header chrome: visible enough to find, but no longer a loud primary-blue slab.
- Further hardened macOS transparent-window composition by precomposing chrome surfaces over the terminal color and letting the native Ghostty host backing slightly overdraw under GPUI seams. Fast sidebar, agent-panel, input-bar, split, and zoom motion should no longer reveal bright desktop pixels through clear backing gaps.
- Stopped continuously reflowing the macOS terminal layout during the right agent-panel hide/show animation. The panel content still animates, but the terminal/panel boundary snaps to a stable geometry so fast toggles do not expose clear backing between GPUI and the native Ghostty view.
- Added a temporary native underlay below visible macOS Ghostty surfaces during chrome transitions. It catches clear-window backing during rapid input-bar, agent-panel, tab-strip, and vertical-tabs toggles without drawing a matte over terminal text.

**Control Plane**

- Hardened pane-local surface lifecycle behavior so owned ephemeral worker panes can close when their final surface closes, inactive surfaces keep correct pane membership, replacement surfaces are materialized before focus moves to them, and `surfaces.wait_ready` only reports `ready` once the terminal is live with shell integration.

## `v0.1.0-beta.46` - 2026-04-30

### Added

**Agent Providers**

- Updated Rig to 0.36.0 and added DeepSeek V4 model support. DeepSeek now defaults to `deepseek-v4-flash`, keeps `deepseek-v4-pro` available in the model picker, and preserves the legacy `deepseek-chat` and `deepseek-reasoner` aliases.

**Settings**

- Settings now opens as a separate window. You can adjust appearance, shortcuts, and provider configuration, save changes, and keep the settings window open while checking the terminal.
- Appearance controls now preview immediately while Settings stays open. Transparency, blur, background image strength, image layout, terminal font, UI font, and cursor style update live; Save Changes persists the current values.
- Polished the Settings save action so it uses Con's active theme colors instead of the generic primary button treatment.
- OpenAI-compatible providers can now fetch available models from the provider's `/models` endpoint when a Base URL and API key are configured.

**Keyboard**

- Added direct tab selection shortcuts: Cmd+1 through Cmd+9 on macOS, Ctrl+1 through Ctrl+9 on Windows and Linux.
- Added pane zoom for the focused split pane. Use Cmd+Shift+Enter on macOS or Alt+Shift+Enter on Windows and Linux to let one pane fill the tab's terminal area, then press it again to restore the split layout.
- Added macOS window cycling with Cmd+Backtick and Cmd+Shift+Backtick.
- Fixed macOS Cmd+Backtick window cycling when the terminal surface has focus.
- Fixed macOS Cmd+Backtick window cycling at the native window-event layer so it works even when the embedded terminal NSView is first responder.
- Fixed macOS Cmd+Backtick window cycling to also handle GPUI's shifted and localized symbol forms (`~`, `<`, `>`), matching the platform's keyboard-layout-aware shortcut behavior.
- Pane picker shortcuts are now scoped to the picker: open it with the configured pane-scope shortcut, then use bare 1-9 to toggle panes, A for all panes, and F for focused pane. A/F are consumed before the input bar sees them, and global Cmd/Ctrl+1-9 remains reserved for tab switching outside the picker.
- Fixed closing the last pane in one window so it closes only that window instead of quitting Con and killing sibling windows.

### Fixed

**Terminal, macOS**

- Fixed fast trackpad scrolling in macOS terminal panes so precise scroll events are sent through Ghostty's precision-scroll path instead of being treated as coarse wheel ticks.
- Reduced macOS terminal scroll-path overhead by syncing the native Ghostty scroll container only for visible tab surfaces while still draining background-tab title and process events.
- Fixed macOS split, nested split, zoom, and unzoom operations that could leave a blank or transparent pane region until the divider was manually resized.

**Workspace**

- Fixed the bottom input bar layout so it spans only the terminal area, staying out of the vertical tab sidebar and right agent panel.

**Settings**

- Fixed Settings live preview dismissal so unsaved appearance and theme changes are rolled back when the standalone Settings window is closed.
- Fixed OpenAI-compatible model discovery so fetched model lists are scoped to the configured Base URL instead of leaking across custom endpoints.
- Fixed OpenAI-compatible model discovery so newly fetched models immediately refresh all related Settings pickers, including active and suggestion model selectors.
- Fixed OpenAI-compatible model discovery for Base URLs with required query parameters, preserving the query while deriving the `/models` endpoint.
- Clarified OpenAI-compatible provider setup when `/models` is unavailable: fetching is optional, and users can type the model ID manually.
- Fixed standalone Settings cleanup so closing the last workspace window also closes Settings on Windows and Linux instead of leaving an orphaned process.
- Hardened OpenAI-compatible model discovery URL normalization so incomplete `/chat/completions` paths fail clearly instead of becoming relative `/models` URLs.

**Keyboard**

- Fixed direct tab selection shortcuts so terminal panes hand Cmd/Ctrl+1-9 back to the app instead of forwarding them to the shell.
- Fixed direct tab selection shortcuts so Cmd/Ctrl+1-9 keeps switching tabs while the pane picker is open; pane selection remains on bare 1-9 inside the picker.

**Windowing**

- Fixed new Con windows opening exactly on top of the previous one. New workspace windows now cascade from the active window while staying within the visible display area when display bounds are available, and still cascade when the platform cannot report bounds.
- Fixed new-window cascade wrapping so the 28px stagger is preserved on the axis that still fits when the other axis wraps to the display edge.
- Unified macOS Window menu cycling with the same native AppKit path used by Cmd+Backtick, so menu actions and keyboard shortcuts share one ordering model.

**Terminal, Windows Backend (preview)**

- Fixed Windows terminal text rendering to avoid RGB color fringing around glyphs. The DirectWrite atlas now uses grayscale antialiasing with neutral coverage compositing, making CJK and mono text look cleaner in screenshots, scaled displays, remote review, and transparent windows.

## `v0.1.0-beta.44` - 2026-04-29

### Added

**Terminal**

- Added terminal file-drop and file-path clipboard paste. Dropped files now paste quoted paths into the active terminal pane instead of being ignored.
- Added image clipboard forwarding for TUI agents such as Codex. When the clipboard contains image bytes, Con forwards a native Ctrl+V keypress so the TUI can attach the image through its own OS-clipboard workflow.
- Added conservative file-URI clipboard parsing so Linux file-manager copies exposed as `text/uri-list` paste as quoted file paths instead of raw `file://` text.
- Hardened terminal Copy/Paste action handling on Windows and Linux so menu/command-dispatched paste uses the same path as the terminal keyboard shortcut.

## `v0.1.0-beta.43` - 2026-04-28

### Added

**Terminal**

- Added terminal URL opening by modifier-clicking visible links: Cmd-click on macOS, Ctrl-click on Windows and Linux. macOS uses libghostty's native link action, while Windows and Linux use a bounded visible-row detector with pointer-cursor feedback so ordinary terminal rendering remains off the hot path.

### Fixed

**Agent Panel**

- Fixed generic Markdown code fences that contain shell prompts, including compact prompt forms like `$amp --version`, so they infer bash highlighting instead of rendering as unhighlighted `code` blocks.

**Terminal**

- Fixed terminal font selection accepting GPUI-only pseudo font families such as `.ZedMono`, which could make macOS Ghostty text render with uneven spacing or overlapping glyphs. Existing bad configs now fall back to the default terminal font, and the Terminal Font picker hides those pseudo families.

## `v0.1.0-beta.42` - 2026-04-27

### Fixed

**Terminal, Windows Backend (preview)**

- Fixed Windows terminal scroll direction so wheel and touchpad gestures follow the platform scroll intent instead of feeling reversed against classic scrolling setups.
- Fixed Tab and Shift+Tab in the Windows terminal so shells and TUIs receive completion/navigation keys instead of GPUI focus navigation swallowing them.
- Fixed Windows terminal scrollbar rendering to cache libghostty-vt scrollbar state by terminal generation instead of polling the expensive scrollbar query during every paint.
- Fixed Windows terminal scrollbar dragging so stale drag state is cleared if the mouse is released outside the terminal pane.

**Terminal, Linux Backend (preview)**

- Fixed Tab and Shift+Tab in the Linux terminal preview with the same terminal-local key capture used on macOS and Windows, preventing GPUI focus navigation from swallowing shell completion keys.

## `v0.1.0-beta.41` - 2026-04-27

### Fixed

**Terminal, Windows Backend (preview)**

- Fixed custom terminal fonts on Windows resolving through the wrong DirectWrite collection, which could make glyphs appear incomplete or spaced apart when a system font such as JetBrains Maple Mono was selected.
- Fixed Windows fallback rendering for CJK and missing default fonts by reserving Unicode-derived two-cell atlas slots for wide glyphs and falling back to installed monospace system fonts before proportional UI fonts.
- Fixed new Windows terminals starting in con's install directory. New panes now honor a valid explicit cwd and otherwise start from the user's home directory instead of inheriting the launcher directory.
- Fixed Windows terminal scrollback controls by adding a visible draggable scrollbar and wiring wheel gestures to libghostty-vt's viewport scrollback path when the shell has not requested mouse tracking.

------

## `v0.1.0-beta.40` - 2026-04-26

### Added

**Agent Panel**
- Added first-class rich Markdown blocks for Mermaid diagrams and LaTeX-style math. Mermaid code fences and display math now render off the UI thread into cached GPUI images with source fallbacks, while inline math uses dedicated math typography instead of being flattened into generic code.
- Mermaid diagrams now render with light/dark-aware colors so diagrams remain readable in both light and dark themes.

**Keyboard**
- Changed the input focus shortcut into a true Input / Terminal toggle. On macOS, Cmd+I now switches from the terminal to the input surface, then back to the first terminal pane when pressed from the input bar or agent-panel input.

### Fixed

**Agent Panel**
- Fixed Mermaid and display math inside nested Markdown containers such as blockquotes and lists so they use the same cached rich renderer as top-level blocks instead of falling back to source text.
- Fixed inline math detection so hyphenated prose and dollar ranges such as `$end-to-end$` or `$3-5$` are not misread as math.
- Fixed inline math rendering for identifier-style formulas such as `$theta$` and `$velocity$`.
- Fixed dark-theme Mermaid diagrams with custom light node fills so missing Mermaid `color:` styles are inferred from fill luminance instead of leaving white text on light shapes.
- Fixed display math SVGs in dark mode so formulas use theme-aware foreground colors and re-render correctly after theme switches.
- Fixed rich Mermaid and math render caching during streaming so replacing a parsed Markdown document with equivalent block content no longer discards already-rendered SVG images.
- Fixed repeated rich-render source-string allocation during ordinary repaint, reducing avoidable memory churn when scrolling chats with diagrams or formulas.
- Fixed display-math fallback styling so it uses the neutral block math style instead of inline-math chips inside an already styled block.
- Consolidated blockquote and list layout rendering for cached rich blocks and fallback Markdown blocks so future spacing and typography changes cannot drift between the two paths.

**Terminal**
- Fixed macOS IME English-mode commits in the terminal so direct ASCII input keeps the normal shell key path and does not disable shell ghost suggestions, while marked CJK composition still uses the IME text path. Buffered ASCII commits are now replayed as per-character key events instead of one synthetic multi-character key.

**Keyboard**
- Fixed the Input / Terminal toggle fallback so Cmd+I never focuses a hidden input surface. If neither the agent-panel input nor the bottom input bar is visible, Cmd+I now reveals and focuses the bottom input bar.

## `v0.1.0-beta.39` - 2026-04-26

### Added

**Workspace — Vertical Tabs**
- Added a vertical-tabs layout for the workspace tab strip. Toggle in *Settings → Appearance → Tabs → Vertical Tabs*.
- Two runtime states:
  - **Collapsed (rail)** — narrow icon rail (~44 px). Smart icon per tab. Hovering an icon pops a small **floating tab card** anchored to the cursor (name, optional subtitle, pane count, SSH / unread badges). The card is informational only — it never displaces the rail or the terminal pane and dismisses the moment the cursor leaves the icon. Drag an icon directly to reorder.
  - **Pinned panel** — full panel (~240 px) with two-line rows (name in system font, optional cwd / `user@host` subtitle in mono). Persisted across restart via `session.vertical_tabs_pinned`.
- Smart per-tab naming. Priority: **user override → AI summary → SSH host → focused process** (parsed from the OSC-set terminal title — `vim README.md`, `htop`, `less log.txt`, etc.) **→ cwd basename → shell**. Bare shells fall through so a row never reads as "bash" when there's something more useful.
- AI labels and icons via a new `TabSummaryEngine` in `con-core`. Each tab's `(cwd, recent commands, OSC title, recent terminal scrollback)` is summarized by the user's already-configured suggestion model into a 1–3 word label and one icon from a closed Phosphor set: `terminal`, `code`, `pulse`, `book-open`, `file-code`, `globe`.
- The AI summarizer uses a JSON response shape (`{"label": "...", "icon": "..."}`) with a tolerant bracket-balanced parser instead of the original `LABEL|ICON` plain-text format, so reasoning models and Markdown-fenced answers are handled without rendering malformed labels.
- Periodic AI re-summarization now polls `request_tab_summaries` every 3 s. The engine stays gated on `agent.suggestion_model.enabled`, short-circuits SSH tabs to host + globe with no LLM call, and uses a per-tab context cache plus 5 s success budget so re-asks stay bounded.
- Smart per-tab icons keyed off the focused process when no AI is available: `terminal` for shells, `globe` for SSH, `</>` for editors (vim/nvim/nano/emacs/helix/code), `pulse` for monitors (htop/top/btop/k9s), `book-open` for pagers (less/more/man/bat), `file-code` for git tools (git/lazygit/tig/gh). User-labelled tabs still get a smart icon picked from the live process / SSH signal — or from the AI summary when available.
- No emoji anywhere in the panel — every glyph is a Phosphor SVG.
- Long paths collapse to `…/parent/last`; `$HOME` collapses to `~`.
- Inline rename: double-click a row's label, or right-click → Rename. Enter commits, Esc cancels. The label is persisted via a new `user_label` field on `TabState`. Reset Name in the context menu clears the override and falls back to smart auto-naming.
- Right-click context menu per row: Rename / Duplicate / Reset Name (if user-labelled) / Move Up / Move Down / Close Tab / Close Other Tabs.
- Drag-to-reorder works in both rail and pinned modes. The dragged tab follows the cursor as a small floating chip; a 2-px primary-color line marks the drop target between rows in pinned mode; the rail uses a hover-bg pill to signal the drop target. Drop fires `SidebarReorder`, which the workspace applies in-place and persists.
- Visual hierarchy follows the design language: a single selection signal (elevated white pill — no accent bar duplication), action affordances (rename pencil, close X) hover-only on every row including the active one, surface separation via opacity blending (no borders, no shadows), system font for labels (terminal-chrome consistency reserved for the row icon and subtitle).
- Width tweens 220 ms ease-out cubic between rail and pinned; floating card appears instantly (Apple tooltip pattern, no transition).
- Pinned/collapsed state and the orientation choice persist across restarts (`vertical_tabs_pinned` in session, `appearance.tabs_orientation` in config). Horizontal tabs remain the default for backward compatibility with every shipped beta. Switching orientation at runtime takes effect immediately.

### Improved

**Agent Panel**
- Reworked long restored agent replies to cache rendered Markdown by block, so typing, scrolling, and revealing more content no longer recreate every paragraph/table/code layout inside a large assistant response.
- Fixed wide Markdown table scrolling so vertical transcript scroll no longer gets hijacked into sideways table movement, while preserving a structured rich-table layout with a native horizontal scrollbar for oversized tables.
- Added more deliberate transcript gutters so user and assistant messages no longer collide with the panel edge or scrollbar.
- Normalized con's embedded mono font family for GPUI-rendered code blocks and terminal chrome so Markdown code uses the intended IoskeleyMono face instead of platform font fallback.
- Hardened restored-session Markdown caching so stale assistant views are cleared when switching or clearing conversations, and in-flight Markdown parses cannot attach to the wrong message after reruns or state swaps.

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

## `v0.1.0-beta.38` - 2026-04-24

### Fixed

**Interface**
- Completed pane-close keyboard handling instead of only showing dead UI. `Close Pane` is now a real configurable shortcut in Settings, with app-level defaults that do not collide with terminal EOF (`Cmd+Opt+W` on macOS, `Alt+Shift+W` on Windows/Linux). Windows/Linux pane split defaults now use `Alt+D` / `Alt+Shift+D` instead of fragile symbol-based bindings.
- `Close Pane` now escalates cleanly when nothing smaller remains to close: last pane closes the tab, and the last tab quits the app.
- Fixed the last-pane quit path so quitting through `Close Pane` or terminal-exit escalation uses the same session save and surface teardown path as the normal app quit action.
- Fixed the command palette scroll behavior on all platforms. Mouse-wheel scrolling and scrollbar dragging no longer snap back to the selected row on every repaint.
- Reduced agent-panel stalls on session-open, long live responses, and oversized reply expansion by making message markdown and persisted reasoning parse lazily, rendering in-flight replies as cheap plain-text previews, parsing full reply markdown off the UI thread when opened, and caching expensive inline markdown text-run transforms so repeated rich-render passes do less work.
- Reduced severe hangs when expanding long real-world agent replies in markdown. Inline code inside long prose/table cells now renders through single `StyledText` runs instead of exploding into large flex-wrapped chip trees.
- Reduced another major markdown-render cost for long agent replies. Assistant message bodies now live behind their own markdown-document entities, and fenced code blocks render as one highlighted text layout per block instead of one GPUI row per line.
- Reduced agent-panel rerender churn further by isolating whole assistant rows behind their own entities. Old restored sessions and live token streaming no longer force the parent panel loop to rebuild every assistant message row on each update.
- Moved the main agent transcript off a single flex-column scroll surface and onto GPUI's variable-height `ListState` path, so old long sessions no longer require the whole conversation tree to be measured and repainted as one document during scroll.
- Reduced restored-session interaction lag by stopping ordinary composer keystrokes from notifying the whole agent panel, keeping long restored markdown from hydrating in the background, and rendering agent-panel inputs with the terminal mono font.

**Terminal — Windows Backend (preview)**
- Reduced the first-frame flash when splitting a new pane on Windows. Brand-new panes now paint their configured terminal background while the first renderer image is still warming up, instead of briefly showing a transparent hole.

## `v0.1.0-beta.37` - 2026-04-24

### Improved

**Terminal — Linux Backend (preview)**
- Reduced Linux preview renderer CPU cost by caching per-row `StyledText` text/runs in `linux_view` and only rebuilding rows marked dirty by the VT snapshot, plus cursor-affected rows.
- Reduced Linux command-to-paint latency by waking the terminal view directly from PTY output instead of waiting for the workspace idle poll loop to discover new output.
- Added shared VT snapshot timing instrumentation behind `CON_GHOSTTY_PROFILE` so large command-start redraw costs on Windows/Linux can be measured directly.
- Documented the remaining Linux performance constraint explicitly: after the row-cache and direct-wake changes, the longer-term fix remains the planned glyph-atlas grid renderer and a lighter-weight shared VT snapshot contract.

### Fixed

**Release pipeline — multi-platform race**
- Fixed `irm https://nowled.ge/con-ps1 | iex` (and the `install.sh` equivalents) failing with `✗ no ZIP found for windows-x86_64` when the Linux release job won the create-and-upload race ahead of the Windows / macOS jobs. Each `release-{linux,macos,windows}.yml` now creates the GitHub release as `--draft`, so `/releases/latest` and the public REST API stay blind to the tag while assets are uploading. A new `release-finalize.yml` workflow watches all three platform workflows via `workflow_run` and atomically promotes the draft to public only once every platform reports a `success` conclusion for the same `head_sha`. If a platform fails, the draft stays drafted on purpose — better to block than to ship an incomplete release.

## `v0.1.0-beta.36` - 2026-04-24

### Fixed

- Windows con no longer be laggy!!!

## `v0.1.0-beta.35` - 2026-04-23

### Fixed

- Fixed Linux con cannot support older GLIBC

## `v0.1.0-beta.34` - 2026-04-23

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

## `v0.1.0-beta.33` - 2026-04-22

### Improved

**Terminal — Windows Backend (preview)**
- Tuned low-hanging renderer performance issues in the Windows D3D11/DirectWrite path.
- Wired terminal theme colors, opacity, and transparency into the Windows renderer.
- Hardened Windows resize/readback behavior, theme switching clear colors, and DWM backdrop behavior across the beta feedback loop.

## `v0.1.0-beta.32` - 2026-04-21

### Added

**Release and Packaging**
- Added Windows in-place auto-update flow and appcast support.

### Fixed

**Release and Packaging**
- Preserved the full beta tag in Windows release/appcast version metadata so beta update comparisons do not collapse to `0.1.0`.
- Made the PowerShell installer ASCII-safe for `irm | iex` usage.
- Updated release/download documentation for the renamed `con-terminal` repository.

## `v0.1.0-beta.31` - 2026-04-21

### Added

**Interface**
- Added a titlebar gear button on Windows and Linux for opening Settings.

**Release and Packaging**
- Added Windows PowerShell installer and release palette entries.

### Fixed

**Terminal — Windows Backend (preview)**
- Switched Windows builds to the console subsystem behavior needed for terminal process startup.
- Baked the full release channel/version into the Windows binary for update polling and user-visible version display.

## `v0.1.0-beta.30` - 2026-04-21

### Added

**Terminal — Windows Backend (preview)**
- Shipped the runtime-validated Phase 3b Windows glyph-atlas renderer: ConPTY plus `libghostty-vt`, D3D11/DirectWrite rendering, HLSL shaders, glyph atlas packing, Nerd Font/PUA handling, wide-glyph ordering, and cursor z-order handling.
- Promoted Windows from planned work to the first beta surface in the docs.

### Fixed

**Release and Packaging**
- Hardened the Windows release workflow against Defender interference, Zig/uucode spawn flakes, Windows runner instability, and MAX_PATH-sensitive cache paths.
- Renamed in-tree repository URLs from `nowledge-co/con` to `nowledge-co/con-terminal`.

## `v0.1.0-beta.29` - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Changed
- Renamed repository URLs to `nowledge-co/con-terminal`.

## `v0.1.0-beta.28` - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Fixed
- Pinned the Windows release runner to `windows-2022` and added on-failure diagnostics.

## `v0.1.0-beta.27` - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Fixed
- Retried Windows release builds around intermittent Zig/uucode spawn failures.

## `v0.1.0-beta.26` - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Fixed
- Disabled Defender real-time scanning during Windows release builds.

## `v0.1.0-beta.25` - 2026-04-21

Tag-only release workflow iteration folded into the `v0.1.0-beta.30` GitHub release notes.

### Added
- Advanced the Windows beta port toward the first published Windows preview.

## `v0.1.0-beta.24` - 2026-04-20

### Improved
- Shipped CI fixes, documentation updates, and broad UI/terminal polish before the Windows beta push.

## `v0.1.0-beta.23` - 2026-04-20

### Added
- Landed Windows support phase 0: non-macOS build scaffolding, portability gates, and an initial Windows-renderable terminal path.

## `v0.1.0-beta.22` - 2026-04-16

### Improved
- Shipped a broad optimization pass across terminal/app behavior.

## `v0.1.0-beta.21` - 2026-04-16

### Added
- Added Kimi provider support for coding workflows.

### Fixed
- Fixed Homebrew promotion and release workflow YAML issues.

## `v0.1.0-beta.20` - 2026-04-15

### Added
- Added Homebrew cask and a one-line install path.

### Improved
- Shipped another beta optimization pass and Zig CI hardening.

## `v0.1.0-beta.19` - 2026-04-15

### Fixed
- Polished terminal cursor behavior for CJK IME input.

## `v0.1.0-beta.18` - 2026-04-15

### Added
- Added early installer work and beta feature polish.

### Fixed
- Fixed Vim quit handling, IME text input issues, macOS hotkeys, terminal flashing, and Ghostty Zig CI pinning.

## `v0.1.0-beta.14` - 2026-04-14

### Improved
- Optimized settings, markdown table rendering, and CJK rendering paths.

## `v0.1.0-beta.3` - 2026-04-14

### Added
- Added configurable UI font size.

### Fixed
- Fixed a markdown code-block rendering performance issue.

## `v0.1.0-beta.1` - 2026-04-14

### Added
- Initial public beta of con.
- Added the first release workflow, app bundle/release infrastructure, documentation, and upstream dependency cleanup.

### Fixed
- Fixed early Sparkle startup/release-upload issues and ordered-list wrapping alignment.
