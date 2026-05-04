# Hotkey Window — Design Spec

**Date:** 2026-05-04  
**Branch:** `wey-gu/hotkey-window`  
**Platform scope:** macOS only (this PR)

---

## Overview

A dedicated floating terminal window that can be summoned and dismissed from anywhere on the system with a single global hotkey (`Cmd+\` by default). Modelled on iTerm2's Hotkey Window and classic Quake-style terminals:

- Slides down from the top edge of the screen
- Floats above other apps (optional, configurable)
- Does not appear in the Dock or Cmd+Tab switcher
- Contains a full `ConWorkspace` (tabs, agent panel, input bar)
- Persists for the lifetime of the app — never destroyed, only shown/hidden

---

## Config

Three new fields added to `KeybindingConfig` in `con-core/src/config.rs`:

```toml
[keybindings]
hotkey_window_enabled       = false        # off by default
hotkey_window               = "cmd-\\"    # Cmd+Backslash
hotkey_window_always_on_top = false        # NSFloatingWindowLevel when true
```

Defaults:

```rust
fn default_hotkey_window_enabled() -> bool { false }
fn default_hotkey_window() -> String { "cmd-\\".into() }
fn default_hotkey_window_always_on_top() -> bool { false }
```

These fields are serialised/deserialised by the existing `Config::save` / `Config::load` TOML path. No migration needed — `#[serde(default)]` on `KeybindingConfig` handles missing fields in existing configs.

---

## New files

### `crates/con-app/src/objc/hotkey_window_trampoline.m`

Pure ObjC/AppKit. Exposes four C functions to Rust:

| Function | Purpose |
|---|---|
| `con_hotkey_window_configure(NSWindow*, bool always_on_top)` | One-time setup: set `NSWindowStyleMaskBorderless`, opt out of Spaces/Exposé cycling (`NSWindowCollectionBehaviorMoveToActiveSpace | NSWindowCollectionBehaviorTransient`), hide from Dock, set initial window level |
| `con_hotkey_window_set_level(NSWindow*, bool always_on_top)` | Live toggle between `NSFloatingWindowLevel` and `NSNormalWindowLevel` |
| `con_hotkey_window_slide_in(NSWindow*, CGFloat screen_height, CGFloat window_height)` | Position window off-screen above top edge, `orderFront`, then animate frame down to `y = screen_height - window_height` via `NSAnimationContext` (0.22 s, ease-in-out) |
| `con_hotkey_window_slide_out(NSWindow*, CGFloat screen_height)` | Animate frame back above top edge, then call `orderOut:nil` on completion |

The slide animation runs entirely in ObjC via `NSAnimationContext` because the window frame lives outside GPUI's render tree. No GPUI `MotionValue` is used for the window-level animation.

A second Carbon hotkey slot is added to the **existing** `global_hotkey_trampoline.m` (new static globals `g_con_hw_hotkey_ref`, `g_con_hw_hotkey_handler`, `g_con_hw_hotkey_callback`) with new C functions:

```c
bool con_register_hotkey_window_hotkey(uint32_t key_code, bool shift, bool control,
                                        bool alt, bool command,
                                        con_hotkey_callback_t callback);
void con_unregister_hotkey_window_hotkey(void);
```

This keeps both hotkeys independent — summon and hotkey-window can be registered simultaneously.

### `crates/con-app/src/hotkey_window.rs`

Rust module. `#[cfg(target_os = "macos")]` gated.

**State** (module-level statics via `OnceLock` / `RefCell`, same pattern as `global_hotkey.rs`):

```rust
static HOTKEY_WINDOW_APP: RefCell<Option<AsyncApp>>
static HOTKEY_WINDOW_HANDLE: RefCell<Option<WindowHandle<gpui_component::Root>>>
static HOTKEY_WINDOW_VISIBLE: RefCell<bool>
```

**Public API:**

```rust
pub fn init(cx: &App, keybindings: &KeybindingConfig)
pub fn update_from_keybindings(keybindings: &KeybindingConfig)
pub fn toggle(cx: &mut App)
pub fn update_level(window: &WindowHandle<...>, always_on_top: bool)
```

**`toggle(cx)`** logic:

1. If no window handle exists → call `open_hotkey_window(cx)` (creates the GPUI window, calls `con_hotkey_window_configure`, stores handle), then slide in.
2. If window exists and visible → slide out, set `HOTKEY_WINDOW_VISIBLE = false`.
3. If window exists and hidden → `cx.activate(true)`, slide in, set `HOTKEY_WINDOW_VISIBLE = true`.

**`open_hotkey_window(cx)`:**

Calls `open_con_window` with `hotkey_window_options(cx)`:

```rust
fn hotkey_window_options(cx: &App) -> WindowOptions {
    // 60% of primary screen width, 40% of screen height
    // positioned off-screen above top edge (y = screen_height)
    // no titlebar, transparent background
    WindowOptions {
        window_bounds: Some(WindowBounds::Fixed(off_screen_rect)),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Blurred,
        ..Default::default()
    }
}
```

After the window opens, retrieves the raw `NSWindow*` via `window_handle()` and calls `con_hotkey_window_configure`.

---

## Modified files

### `crates/con-core/src/config.rs`

Add three fields to `KeybindingConfig`:

```rust
pub hotkey_window_enabled: bool,
pub hotkey_window: String,
pub hotkey_window_always_on_top: bool,
```

Add corresponding `default_*` functions and wire into `Default` impl.

### `crates/con-app/src/main.rs`

- Add `#[cfg(target_os = "macos")] mod hotkey_window;`
- In `app.run`: call `hotkey_window::init(cx, &config.keybindings)` after `global_hotkey::init`
- Add `ToggleHotkeyWindow` to the `actions!` macro
- Register `cx.on_action(|_: &ToggleHotkeyWindow, cx| hotkey_window::toggle(cx))`
- Add "Hotkey Window" menu item under View menu

### `crates/con-app/src/global_hotkey.rs`

Add `update_hotkey_window_registration(keybindings: &KeybindingConfig)` — mirrors `update_registration` but calls `con_register_hotkey_window_hotkey` / `con_unregister_hotkey_window_hotkey` and fires `hotkey_window::toggle` via the stored `AsyncApp`.

### `crates/con-app/src/settings_panel.rs`

Add a new "Hotkey Window" card in `render_keys()`, below the existing Global Hotkey card. Card contains:

1. Enable switch (`hotkey_window_enabled`) — same pattern as `global_summon_enabled`
2. Keystroke recorder for `hotkey_window` binding — same pattern as `global_summon`
3. Always on top switch (`hotkey_window_always_on_top`) — `Switch` component, only active when enabled

When `hotkey_window_always_on_top` changes and the window exists, call `hotkey_window::update_level(...)` immediately (live update, no restart needed).

`record_keystroke` match arm: add `"hotkey_window" => self.config.keybindings.hotkey_window = binding`.

`keybinding_value` match arm: add `"hotkey_window" => &self.config.keybindings.hotkey_window`.

When settings are saved (`SaveSettings`), call `hotkey_window::update_from_keybindings(&config.keybindings)`.

### `crates/con-app/build.rs`

Add `hotkey_window_trampoline.m` to the `cc::Build` compilation and `rerun-if-changed` entry.

---

## Window properties

| Property | Value |
|---|---|
| Style mask | `NSWindowStyleMaskBorderless` |
| Window level | `NSNormalWindowLevel` (default) / `NSFloatingWindowLevel` (always on top) |
| Collection behavior | `NSWindowCollectionBehaviorMoveToActiveSpace \| NSWindowCollectionBehaviorTransient` |
| Dock visibility | Hidden (no `NSWindowCollectionBehaviorCanJoinAllSpaces`, no activation policy change needed — panel style handles it) |
| Size | 60% primary screen width × 40% screen height |
| Position (visible) | Top of primary screen, horizontally centered |
| Position (hidden) | Off-screen above top edge |
| Background | Blurred (matches main window default) |
| Titlebar | None |
| Animation | `NSAnimationContext` 0.22 s ease-in-out slide |

---

## Hotkey registration

The hotkey window uses the same Carbon `RegisterEventHotKey` infrastructure as the existing global summon, but in a separate slot. Both can be active simultaneously. The `gpui_key_to_keycode` table in `global_hotkey.rs` already covers `\\` (keycode `0x2A`) — no additions needed.

Default binding: `cmd-\\` → `command=true`, `key_code=0x2A`.

---

## Settings UI layout (Keys section)

```
┌─ Global Hotkey ──────────────────────────────────┐
│  [existing card — unchanged]                      │
└───────────────────────────────────────────────────┘

┌─ Hotkey Window ──────────────────────────────────┐
│  Enable          [switch]                         │
│  Shortcut        [Cmd \]  (click to record)       │
│  Always on top   [switch]                         │
└───────────────────────────────────────────────────┘
```

---

## Crate boundaries

- `con-core/config.rs` — config fields only, no UI or AppKit deps
- `hotkey_window.rs` — `#[cfg(target_os = "macos")]`, lives in `con-app`
- `hotkey_window_trampoline.m` — ObjC, compiled by `build.rs` into `con_objc_trampolines`
- `global_hotkey_trampoline.m` — extended with second hotkey slot, no new file needed for the Carbon registration

---

## Out of scope (this PR)

- Windows / Linux support (stub or future work)
- Per-screen positioning (always uses primary screen)
- Hotkey window size configurability in Settings UI (fixed 60×40% for now)
- Remembering hotkey window position across restarts
- Separate session persistence for the hotkey window (uses `fresh_window_session_with_history`)
