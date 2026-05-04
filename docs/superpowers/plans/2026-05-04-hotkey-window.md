# Hotkey Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a macOS-only iTerm2-style Hotkey Window that opens a dedicated floating Con workspace with a global `Cmd+\\` shortcut, slides down from the top of the screen, hides without destroying state, and supports an optional always-on-top mode.

**Architecture:** Reuse the existing GPUI `ConWorkspace` window path for content, but introduce a dedicated macOS window-management layer for panel behavior and top-edge slide animation. Extend the current Carbon global-hotkey bridge with a second independent registration slot, then add a new Rust `hotkey_window` module that owns the dedicated window handle and invokes AppKit trampoline functions for configure/show/hide/level updates.

**Tech Stack:** Rust, GPUI, AppKit Objective-C trampolines, Carbon `RegisterEventHotKey`, `env_logger`, existing `con-core::Config` TOML persistence.

---

## File map

### Existing files to modify

- `crates/con-core/src/config.rs`
  - Add three new hotkey-window config fields, defaults, and tests.
- `crates/con-app/build.rs`
  - Compile the new Objective-C trampoline file.
- `crates/con-app/src/main.rs`
  - Register the new module/action and initialize hotkey-window support.
- `crates/con-app/src/global_hotkey.rs`
  - Add FFI declarations and second registration/update path for the new system-wide shortcut.
- `crates/con-app/src/objc/global_hotkey_trampoline.m`
  - Add a second Carbon hotkey slot so summon and hotkey window can coexist.
- `crates/con-app/src/settings_panel.rs`
  - Add the Hotkey Window card in Keys settings and wire live draft changes.
- `crates/con-app/src/workspace.rs`
  - Reapply hotkey-window registration after settings save.

### New files to create

- `crates/con-app/src/hotkey_window.rs`
  - Dedicated macOS-only controller for creation, visibility, and always-on-top updates.
- `crates/con-app/src/objc/hotkey_window_trampoline.m`
  - AppKit helpers for configuring the dedicated window and animating show/hide.

### Verification surface

- `cargo test -p con-core`
- `cargo test -p con-app`
- `cargo build -p con-app`
- Manual macOS verification: settings toggles, shortcut registration, slide-in/slide-out, persistence while hidden, always-on-top toggle.

---

### Task 1: Add config fields and tests

**Files:**
- Modify: `crates/con-core/src/config.rs`
- Test: `crates/con-core/src/config.rs`

- [ ] **Step 1: Write the failing tests for new keybinding defaults and TOML loading**

Add these tests to `crates/con-core/src/config.rs` inside the existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn default_keybindings_include_hotkey_window_fields() {
        let config = Config::default();
        assert!(!config.keybindings.hotkey_window_enabled);
        assert_eq!(config.keybindings.hotkey_window, "cmd-\\");
        assert!(!config.keybindings.hotkey_window_always_on_top);
    }

    #[test]
    fn legacy_configs_receive_hotkey_window_defaults() {
        let content = r#"
[keybindings]
global_summon_enabled = true
global_summon = "alt-space"
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(!config.keybindings.hotkey_window_enabled);
        assert_eq!(config.keybindings.hotkey_window, "cmd-\\");
        assert!(!config.keybindings.hotkey_window_always_on_top);
    }

    #[test]
    fn loaded_configs_preserve_explicit_hotkey_window_fields() {
        let content = r#"
[keybindings]
hotkey_window_enabled = true
hotkey_window = "cmd-\\"
hotkey_window_always_on_top = true
"#;
        let config: Config = toml::from_str(content).unwrap();

        assert!(config.keybindings.hotkey_window_enabled);
        assert_eq!(config.keybindings.hotkey_window, "cmd-\\");
        assert!(config.keybindings.hotkey_window_always_on_top);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
cargo test -p con-core default_keybindings_include_hotkey_window_fields legacy_configs_receive_hotkey_window_defaults loaded_configs_preserve_explicit_hotkey_window_fields
```

Expected: compile failure because `hotkey_window_enabled`, `hotkey_window`, and `hotkey_window_always_on_top` do not exist on `KeybindingConfig`.

- [ ] **Step 3: Add the new config fields and defaults**

In `crates/con-core/src/config.rs`, add these helper functions near the existing global summon defaults:

```rust
fn default_hotkey_window_enabled() -> bool {
    false
}

fn default_hotkey_window() -> String {
    "cmd-\\".into()
}

fn default_hotkey_window_always_on_top() -> bool {
    false
}
```

Then extend `KeybindingConfig`:

```rust
pub struct KeybindingConfig {
    pub toggle_agent: String,
    pub command_palette: String,
    pub new_window: String,
    pub new_tab: String,
    pub close_tab: String,
    pub close_pane: String,
    pub toggle_pane_zoom: String,
    pub next_tab: String,
    pub previous_tab: String,
    pub settings: String,
    pub quit: String,
    pub split_right: String,
    pub split_down: String,
    pub focus_input: String,
    pub cycle_input_mode: String,
    pub toggle_input_bar: String,
    pub toggle_pane_scope: String,
    pub toggle_vertical_tabs: String,
    pub new_surface: String,
    pub new_surface_split_right: String,
    pub new_surface_split_down: String,
    pub next_surface: String,
    pub previous_surface: String,
    pub rename_surface: String,
    pub close_surface: String,
    pub global_summon_enabled: bool,
    pub global_summon: String,
    pub hotkey_window_enabled: bool,
    pub hotkey_window: String,
    pub hotkey_window_always_on_top: bool,
}
```

And update the `Default` impl:

```rust
            global_summon_enabled: default_global_summon_enabled(),
            global_summon: default_global_summon(),
            hotkey_window_enabled: default_hotkey_window_enabled(),
            hotkey_window: default_hotkey_window(),
            hotkey_window_always_on_top: default_hotkey_window_always_on_top(),
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:

```bash
cargo test -p con-core default_keybindings_include_hotkey_window_fields legacy_configs_receive_hotkey_window_defaults loaded_configs_preserve_explicit_hotkey_window_fields
```

Expected: PASS.

- [ ] **Step 5: Commit the config changes**

```bash
git add crates/con-core/src/config.rs
git commit -m "feat: add hotkey window config"
```

---

### Task 2: Add the AppKit trampoline and second Carbon hotkey slot

**Files:**
- Create: `crates/con-app/src/objc/hotkey_window_trampoline.m`
- Modify: `crates/con-app/src/objc/global_hotkey_trampoline.m`
- Modify: `crates/con-app/build.rs`
- Test: `cargo build -p con-app`

- [ ] **Step 1: Write the failing build expectation by referencing the new Objective-C file in build.rs**

In `crates/con-app/build.rs`, extend the macOS build block like this:

```rust
        cc::Build::new()
            .file("src/objc/sparkle_trampoline.m")
            .file("src/objc/global_hotkey_trampoline.m")
            .file("src/objc/hotkey_window_trampoline.m")
            .flag("-fobjc-arc")
            .flag("-fmodules")
            .compile("con_objc_trampolines");

        println!("cargo:rerun-if-changed=src/objc/sparkle_trampoline.m");
        println!("cargo:rerun-if-changed=src/objc/global_hotkey_trampoline.m");
        println!("cargo:rerun-if-changed=src/objc/hotkey_window_trampoline.m");
        println!("cargo:rustc-link-lib=framework=Carbon");
```

- [ ] **Step 2: Run the build to verify it fails because the new file does not exist yet**

Run:

```bash
cargo build -p con-app
```

Expected: FAIL with a compiler/build-script error mentioning `src/objc/hotkey_window_trampoline.m` not found.

- [ ] **Step 3: Create the new AppKit trampoline file**

Create `crates/con-app/src/objc/hotkey_window_trampoline.m` with this implementation:

```objective-c
#import <AppKit/AppKit.h>
#include <stdbool.h>

static NSRect con_hotkey_window_hidden_frame(NSWindow *window) {
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    NSRect visible = screen.visibleFrame;
    NSRect frame = window.frame;
    frame.origin.x = NSMidX(visible) - (frame.size.width / 2.0);
    frame.origin.y = NSMaxY(visible);
    return frame;
}

static NSRect con_hotkey_window_visible_frame(NSWindow *window) {
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    NSRect visible = screen.visibleFrame;
    NSRect frame = window.frame;
    frame.origin.x = NSMidX(visible) - (frame.size.width / 2.0);
    frame.origin.y = NSMaxY(visible) - frame.size.height;
    return frame;
}

void con_hotkey_window_configure(void *window_ptr, bool always_on_top) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    window.styleMask = NSWindowStyleMaskBorderless;
    window.collectionBehavior =
        NSWindowCollectionBehaviorMoveToActiveSpace |
        NSWindowCollectionBehaviorTransient;
    window.opaque = NO;
    window.hasShadow = YES;
    window.hidesOnDeactivate = NO;
    window.releasedWhenClosed = NO;
    window.movable = NO;
    window.level = always_on_top ? NSFloatingWindowLevel : NSNormalWindowLevel;
    [window setFrame:con_hotkey_window_hidden_frame(window) display:NO];
}

void con_hotkey_window_set_level(void *window_ptr, bool always_on_top) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }
    window.level = always_on_top ? NSFloatingWindowLevel : NSNormalWindowLevel;
}

void con_hotkey_window_slide_in(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    [window setFrame:con_hotkey_window_hidden_frame(window) display:NO];
    [window orderFront:nil];

    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = 0.22;
        context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
        [[window animator] setFrame:con_hotkey_window_visible_frame(window) display:YES];
    } completionHandler:nil];
}

void con_hotkey_window_slide_out(void *window_ptr) {
    NSWindow *window = (__bridge NSWindow *)window_ptr;
    if (window == nil) {
        return;
    }

    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = 0.18;
        context.timingFunction = [CAMediaTimingFunction functionWithName:kCAMediaTimingFunctionEaseInEaseOut];
        [[window animator] setFrame:con_hotkey_window_hidden_frame(window) display:YES];
    } completionHandler:^{
        [window orderOut:nil];
    }];
}
```

- [ ] **Step 4: Extend the existing Carbon trampoline with a second independent hotkey registration slot**

In `crates/con-app/src/objc/global_hotkey_trampoline.m`, add these new globals near the existing hotkey globals:

```objective-c
static EventHotKeyRef g_con_hw_hotkey_ref = NULL;
static con_hotkey_callback_t g_con_hw_hotkey_callback = NULL;
```

Then replace the current event handler body with one that distinguishes hotkey ids:

```objective-c
static OSStatus con_hotkey_handler(EventHandlerCallRef nextHandler, EventRef event, void *userData) {
    (void)nextHandler;
    (void)userData;
    if (GetEventClass(event) != kEventClassKeyboard ||
        GetEventKind(event) != kEventHotKeyPressed) {
        return noErr;
    }

    EventHotKeyID hotkey_id = {0};
    GetEventParameter(event,
                      kEventParamDirectObject,
                      typeEventHotKeyID,
                      NULL,
                      sizeof(hotkey_id),
                      NULL,
                      &hotkey_id);

    if (hotkey_id.signature != 'conh') {
        return noErr;
    }

    if (hotkey_id.id == 1 && g_con_hotkey_callback != NULL) {
        g_con_hotkey_callback();
    } else if (hotkey_id.id == 2 && g_con_hw_hotkey_callback != NULL) {
        g_con_hw_hotkey_callback();
    }
    return noErr;
}
```

Add helper registration functions below `con_register_global_hotkey`:

```objective-c
bool con_register_hotkey_window_hotkey(
    uint32_t key_code,
    bool shift,
    bool control,
    bool alt,
    bool command,
    con_hotkey_callback_t callback
) {
    if (g_con_hw_hotkey_ref != NULL) {
        UnregisterEventHotKey(g_con_hw_hotkey_ref);
        g_con_hw_hotkey_ref = NULL;
    }

    g_con_hw_hotkey_callback = callback;

    UInt32 modifiers = 0;
    if (shift) {
        modifiers |= shiftKey;
    }
    if (control) {
        modifiers |= controlKey;
    }
    if (alt) {
        modifiers |= optionKey;
    }
    if (command) {
        modifiers |= cmdKey;
    }

    EventHotKeyID hotkey_id = {
        .signature = 'conh',
        .id = 2,
    };

    OSStatus status = RegisterEventHotKey(
        key_code,
        modifiers,
        hotkey_id,
        GetApplicationEventTarget(),
        0,
        &g_con_hw_hotkey_ref
    );

    return status == noErr;
}

void con_unregister_hotkey_window_hotkey(void) {
    if (g_con_hw_hotkey_ref != NULL) {
        UnregisterEventHotKey(g_con_hw_hotkey_ref);
        g_con_hw_hotkey_ref = NULL;
    }
    g_con_hw_hotkey_callback = NULL;
}
```
```

- [ ] **Step 5: Run the build to verify the Objective-C bridge compiles**

Run:

```bash
cargo build -p con-app
```

Expected: build may still fail on missing Rust FFI references later, but it should no longer fail because `hotkey_window_trampoline.m` is missing or malformed Objective-C syntax.

- [ ] **Step 6: Commit the trampoline changes**

```bash
git add crates/con-app/build.rs crates/con-app/src/objc/global_hotkey_trampoline.m crates/con-app/src/objc/hotkey_window_trampoline.m
git commit -m "feat: add macos hotkey window trampolines"
```

---

### Task 3: Add the Rust hotkey-window controller module

**Files:**
- Create: `crates/con-app/src/hotkey_window.rs`
- Modify: `crates/con-app/src/main.rs`
- Test: `crates/con-app/src/hotkey_window.rs`

- [ ] **Step 1: Write a failing unit test for the pure-Rust config gate**

Create `crates/con-app/src/hotkey_window.rs` with a small testable helper and failing test first:

```rust
fn hotkey_window_enabled(binding_enabled: bool, binding: &str) -> bool {
    binding_enabled && !binding.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::hotkey_window_enabled;

    #[test]
    fn disabled_or_empty_binding_disables_hotkey_window_registration() {
        assert!(!hotkey_window_enabled(false, "cmd-\\"));
        assert!(!hotkey_window_enabled(true, "   "));
        assert!(hotkey_window_enabled(true, "cmd-\\"));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails because the module is not wired into the crate yet**

Run:

```bash
cargo test -p con-app hotkey_window_enabled
```

Expected: compile failure because `main.rs` does not declare `mod hotkey_window;` yet or because the module is incomplete.

- [ ] **Step 3: Implement the hotkey-window controller module**

Replace `crates/con-app/src/hotkey_window.rs` with this implementation skeleton, keeping the tested helper at the bottom:

```rust
use con_core::config::KeybindingConfig;
use gpui::{App, AsyncApp};
use std::cell::RefCell;

unsafe extern "C" {
    fn con_hotkey_window_configure(window_ptr: *mut std::ffi::c_void, always_on_top: bool);
    fn con_hotkey_window_set_level(window_ptr: *mut std::ffi::c_void, always_on_top: bool);
    fn con_hotkey_window_slide_in(window_ptr: *mut std::ffi::c_void);
    fn con_hotkey_window_slide_out(window_ptr: *mut std::ffi::c_void);
}

thread_local! {
    static HOTKEY_WINDOW_APP: RefCell<Option<AsyncApp>> = const { RefCell::new(None) };
    static HOTKEY_WINDOW_RAW_PTR: RefCell<Option<usize>> = const { RefCell::new(None) };
    static HOTKEY_WINDOW_VISIBLE: RefCell<bool> = const { RefCell::new(false) };
}

pub fn init(cx: &App, keybindings: &KeybindingConfig) {
    HOTKEY_WINDOW_APP.with(|app| {
        *app.borrow_mut() = Some(cx.to_async());
    });
    update_from_keybindings(keybindings);
}

pub fn update_from_keybindings(_keybindings: &KeybindingConfig) {}

pub fn store_window_ptr(window_ptr: *mut std::ffi::c_void, always_on_top: bool) {
    HOTKEY_WINDOW_RAW_PTR.with(|slot| {
        *slot.borrow_mut() = Some(window_ptr as usize);
    });
    unsafe { con_hotkey_window_configure(window_ptr, always_on_top) };
}

pub fn toggle(_cx: &mut App) {
    HOTKEY_WINDOW_RAW_PTR.with(|slot| {
        let Some(window_ptr) = *slot.borrow() else {
            return;
        };
        HOTKEY_WINDOW_VISIBLE.with(|visible| {
            let mut visible = visible.borrow_mut();
            let raw = window_ptr as *mut std::ffi::c_void;
            if *visible {
                unsafe { con_hotkey_window_slide_out(raw) };
                *visible = false;
            } else {
                unsafe { con_hotkey_window_slide_in(raw) };
                *visible = true;
            }
        });
    });
}

pub fn set_always_on_top(always_on_top: bool) {
    HOTKEY_WINDOW_RAW_PTR.with(|slot| {
        if let Some(window_ptr) = *slot.borrow() {
            unsafe { con_hotkey_window_set_level(window_ptr as *mut std::ffi::c_void, always_on_top) };
        }
    });
}

pub fn mark_hidden() {
    HOTKEY_WINDOW_VISIBLE.with(|visible| *visible.borrow_mut() = false);
}

pub fn mark_visible() {
    HOTKEY_WINDOW_VISIBLE.with(|visible| *visible.borrow_mut() = true);
}

fn hotkey_window_enabled(binding_enabled: bool, binding: &str) -> bool {
    binding_enabled && !binding.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::hotkey_window_enabled;

    #[test]
    fn disabled_or_empty_binding_disables_hotkey_window_registration() {
        assert!(!hotkey_window_enabled(false, "cmd-\\"));
        assert!(!hotkey_window_enabled(true, "   "));
        assert!(hotkey_window_enabled(true, "cmd-\\"));
    }
}
```

Then wire the module in `crates/con-app/src/main.rs` near the existing macOS-only modules:

```rust
#[cfg(target_os = "macos")]
mod hotkey_window;
```

- [ ] **Step 4: Add the new action and initialization hooks in main.rs**

In `crates/con-app/src/main.rs`, extend the `actions!` list:

```rust
        ToggleSummon,
        ToggleHotkeyWindow,
        ToggleAgentPanel,
```

Add initialization after `global_hotkey::init`:

```rust
        #[cfg(target_os = "macos")]
        hotkey_window::init(cx, &config.keybindings);
```

Add the action handler near the existing summon handler:

```rust
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &ToggleHotkeyWindow, cx: &mut App| {
            hotkey_window::toggle(cx);
        });
```

And add a View menu item:

```rust
                    MenuItem::action("Hotkey Window", ToggleHotkeyWindow),
```

Place it just below `Toggle Input Bar` in the View menu.

- [ ] **Step 5: Run the test to verify the module compiles and the helper test passes**

Run:

```bash
cargo test -p con-app hotkey_window_enabled
```

Expected: PASS.

- [ ] **Step 6: Commit the controller module wiring**

```bash
git add crates/con-app/src/hotkey_window.rs crates/con-app/src/main.rs
git commit -m "feat: add hotkey window controller"
```

---

### Task 4: Wire global registration and runtime toggle behavior

**Files:**
- Modify: `crates/con-app/src/global_hotkey.rs`
- Modify: `crates/con-app/src/main.rs`
- Modify: `crates/con-app/src/workspace.rs`
- Test: `crates/con-app/src/global_hotkey.rs`

- [ ] **Step 1: Write a failing parser/registration test for the new default binding**

Add this test to `crates/con-app/src/global_hotkey.rs`:

```rust
    #[test]
    fn parses_default_hotkey_window_hotkey() {
        let keystroke = parse_global_hotkey("cmd-\\").expect("cmd-\\ should parse");
        assert!(keystroke.modifiers.platform);
        assert_eq!(keystroke.key, "\\");
    }
```

- [ ] **Step 2: Run the test to verify current behavior before wiring the second registration path**

Run:

```bash
cargo test -p con-app parses_default_hotkey_window_hotkey
```

Expected: PASS. This anchors the binding parser before FFI changes.

- [ ] **Step 3: Add FFI declarations and update logic for the second global hotkey**

In `crates/con-app/src/global_hotkey.rs`, add new externs:

```rust
    fn con_register_hotkey_window_hotkey(
        key_code: u32,
        shift: bool,
        control: bool,
        alt: bool,
        command: bool,
        callback: extern "C" fn(),
    ) -> bool;
    fn con_unregister_hotkey_window_hotkey();
```

Add this callback next to `on_global_hotkey_pressed`:

```rust
extern "C" fn on_hotkey_window_pressed() {
    GLOBAL_HOTKEY_APP.with(|app| {
        let Some(app) = app.borrow().clone() else {
            return;
        };

        app.update(|cx| {
            crate::hotkey_window::toggle(cx);
        });
    });
}
```

Split the updater into two helpers and call both from `init` / `update_from_keybindings`:

```rust
pub fn init(cx: &App, keybindings: &KeybindingConfig) {
    GLOBAL_HOTKEY_APP.with(|app| {
        *app.borrow_mut() = Some(cx.to_async());
    });
    update_from_keybindings(keybindings);
}

pub fn update_from_keybindings(keybindings: &KeybindingConfig) {
    update_summon_registration(keybindings);
    update_hotkey_window_registration(keybindings);
}

fn update_summon_registration(keybindings: &KeybindingConfig) {
    // move existing update_registration body here unchanged
}

fn update_hotkey_window_registration(keybindings: &KeybindingConfig) {
    if !keybindings.hotkey_window_enabled {
        unsafe { con_unregister_hotkey_window_hotkey() };
        return;
    }

    let binding = &keybindings.hotkey_window;
    let Some(keystroke) = parse_global_hotkey(binding) else {
        unsafe { con_unregister_hotkey_window_hotkey() };
        if !binding.trim().is_empty() {
            log::warn!("hotkey window: unsupported binding {:?}, disabling", binding);
        }
        return;
    };

    let Some(key_code) = gpui_key_to_keycode(&keystroke.key) else {
        unsafe { con_unregister_hotkey_window_hotkey() };
        log::warn!(
            "hotkey window: unsupported key {:?} in binding {:?}, disabling",
            keystroke.key,
            binding
        );
        return;
    };

    let ok = unsafe {
        con_register_hotkey_window_hotkey(
            key_code,
            keystroke.modifiers.shift,
            keystroke.modifiers.control,
            keystroke.modifiers.alt,
            keystroke.modifiers.platform,
            on_hotkey_window_pressed,
        )
    };

    if !ok {
        unsafe { con_unregister_hotkey_window_hotkey() };
        log::warn!("hotkey window: failed to register binding {:?}", binding);
    }
}
```

- [ ] **Step 4: Make settings saves reapply both hotkey registrations**

In `crates/con-app/src/workspace.rs`, keep the existing keybinding save path but rely on the updated `update_from_keybindings` behavior:

```rust
        let kb = full_config.keybindings.clone();
        crate::bind_app_keybindings(cx, &kb);
        #[cfg(target_os = "macos")]
        crate::global_hotkey::update_from_keybindings(&kb);
```

No behavior change needed here beyond verifying that the updated `global_hotkey::update_from_keybindings` now covers the hotkey window too.

- [ ] **Step 5: Run targeted tests for the parser and hotkey module**

Run:

```bash
cargo test -p con-app parses_default_hotkey_window_hotkey hotkey_window_enabled
```

Expected: PASS.

- [ ] **Step 6: Commit the hotkey registration wiring**

```bash
git add crates/con-app/src/global_hotkey.rs crates/con-app/src/workspace.rs
git commit -m "feat: register hotkey window global shortcut"
```

---

### Task 5: Build the dedicated window creation path and always-on-top updates

**Files:**
- Modify: `crates/con-app/src/main.rs`
- Modify: `crates/con-app/src/hotkey_window.rs`
- Test: `cargo build -p con-app`

- [ ] **Step 1: Write the failing build by calling a not-yet-defined helper from the hotkey-window module**

In `crates/con-app/src/hotkey_window.rs`, change `toggle` temporarily to call a helper that does not exist yet:

```rust
pub fn toggle(cx: &mut App) {
    ensure_hotkey_window(cx);
    HOTKEY_WINDOW_RAW_PTR.with(|slot| {
        let Some(window_ptr) = *slot.borrow() else {
            return;
        };
        HOTKEY_WINDOW_VISIBLE.with(|visible| {
            let mut visible = visible.borrow_mut();
            let raw = window_ptr as *mut std::ffi::c_void;
            if *visible {
                unsafe { con_hotkey_window_slide_out(raw) };
                *visible = false;
            } else {
                unsafe { con_hotkey_window_slide_in(raw) };
                *visible = true;
            }
        });
    });
}
```

- [ ] **Step 2: Run the build to verify it fails**

Run:

```bash
cargo build -p con-app
```

Expected: FAIL with `cannot find function ensure_hotkey_window`.

- [ ] **Step 3: Implement the window creation helper and dedicated open path**

In `crates/con-app/src/hotkey_window.rs`, add this helper:

```rust
pub fn ensure_hotkey_window(cx: &mut App) {
    let already_exists = HOTKEY_WINDOW_RAW_PTR.with(|slot| slot.borrow().is_some());
    if already_exists {
        return;
    }

    let config = con_core::Config::load().unwrap_or_default();
    crate::open_con_window(config.clone(), crate::fresh_window_session_with_history(), false, cx);
}
```

Then immediately improve the integration in `crates/con-app/src/main.rs` by adding a dedicated opener instead of reusing normal windows blindly. Add this new function next to `open_con_window`:

```rust
#[cfg(target_os = "macos")]
pub(crate) fn open_hotkey_window(
    config: con_core::Config,
    session: Session,
    cx: &mut App,
) {
    let mut window_options = default_window_options(&config, cx);
    window_options.titlebar = None;
    cx.spawn(async move |cx| {
        if let Err(err) = cx.open_window(window_options, |window, cx| {
            let restored_session = session.clone();
            let view = cx.new(|cx| {
                ConWorkspace::from_session(config.clone(), restored_session, window, cx)
            });
            crate::hotkey_window::store_window_ptr(
                window.window_handle().as_raw() as *mut std::ffi::c_void,
                config.keybindings.hotkey_window_always_on_top,
            );
            cx.new(|cx| gpui_component::Root::new(view, window, cx).bg(cx.theme().transparent))
        }) {
            log::error!("Failed to open hotkey window: {err}");
        }
    })
    .detach();
}
```

Then update `ensure_hotkey_window` to call the new dedicated opener:

```rust
pub fn ensure_hotkey_window(cx: &mut App) {
    let already_exists = HOTKEY_WINDOW_RAW_PTR.with(|slot| slot.borrow().is_some());
    if already_exists {
        return;
    }

    let config = con_core::Config::load().unwrap_or_default();
    crate::open_hotkey_window(config.clone(), crate::fresh_window_session_with_history(), cx);
}
```

Finally, add a visibility-safe always-on-top helper:

```rust
pub fn update_from_keybindings(keybindings: &KeybindingConfig) {
    set_always_on_top(keybindings.hotkey_window_always_on_top);
}
```

- [ ] **Step 4: Rebuild and fix raw handle extraction if needed using the actual GPUI API**

Run:

```bash
cargo build -p con-app
```

Expected: likely compile errors around `window.window_handle().as_raw()` because GPUI's API may require `raw_window_handle::HasWindowHandle` usage instead. Fix it in `main.rs` using this fallback pattern if needed:

```rust
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

let raw_ptr = HasWindowHandle::window_handle(window)
    .ok()
    .and_then(|handle| match handle.as_raw() {
        RawWindowHandle::AppKit(handle) => Some(handle.ns_window.as_ptr()),
        _ => None,
    });
if let Some(raw_ptr) = raw_ptr {
    crate::hotkey_window::store_window_ptr(raw_ptr.cast(), config.keybindings.hotkey_window_always_on_top);
}
```

Make the build green with the real handle API used elsewhere in the codebase.

- [ ] **Step 5: Run the build to verify the dedicated window path compiles**

Run:

```bash
cargo build -p con-app
```

Expected: PASS.

- [ ] **Step 6: Commit the dedicated window open path**

```bash
git add crates/con-app/src/main.rs crates/con-app/src/hotkey_window.rs
git commit -m "feat: add dedicated hotkey window"
```

---

### Task 6: Add Settings UI for enable/binding/always-on-top

**Files:**
- Modify: `crates/con-app/src/settings_panel.rs`
- Modify: `crates/con-app/src/hotkey_window.rs`
- Test: `cargo test -p con-app`

- [ ] **Step 1: Write the failing state-routing changes for the new binding field**

In `crates/con-app/src/settings_panel.rs`, add the missing match arms in `record_keystroke` and `binding_value`:

```rust
            "global_summon" => self.config.keybindings.global_summon = binding,
            "hotkey_window" => self.config.keybindings.hotkey_window = binding,
            "new_window" => self.config.keybindings.new_window = binding,
```

and

```rust
            "global_summon" => &self.config.keybindings.global_summon,
            "hotkey_window" => &self.config.keybindings.hotkey_window,
            "new_window" => &self.config.keybindings.new_window,
```

- [ ] **Step 2: Run the app tests to catch any exhaustive-match or unused-state issues**

Run:

```bash
cargo test -p con-app settings_panel
```

Expected: may FAIL on unrelated filter selection if there are no tests by that name; if so, rerun the crate tests in Step 5 instead. This step is only to surface compile issues immediately.

- [ ] **Step 3: Add the Hotkey Window card below Global Hotkey**

In `render_keys()`, mirror the existing `global_summon_card` pattern. Add local state reads near the existing summon locals:

```rust
        let hotkey_window_enabled = self.config.keybindings.hotkey_window_enabled;
        let hotkey_window_value = self.config.keybindings.hotkey_window.clone();
        let hotkey_window_always_on_top = self.config.keybindings.hotkey_window_always_on_top;
        let hotkey_window_recording = recording.as_deref() == Some("hotkey_window");
```

Add the badge builder:

```rust
        let hotkey_window_badge = if hotkey_window_recording {
            div()
                .min_h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .bg(theme.primary.opacity(0.10))
                .text_color(theme.primary)
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .child("Press shortcut…")
                .into_any_element()
        } else if !hotkey_window_value.trim().is_empty() {
            crate::keycaps::keycaps_for_binding(&hotkey_window_value, theme)
        } else {
            div()
                .min_h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(8.0))
                .bg(theme.muted.opacity(0.08))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child("Not set")
                .into_any_element()
        };
```

Then add a card with three rows:

```rust
        let hotkey_window_card = card(theme, card_opacity)
            .child(
                div()
                    .px(px(16.0))
                    .py(px(13.0))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .flex_1()
                            .max_w(px(430.0))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Hotkey Window"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .line_height(px(17.0))
                                    .text_color(theme.muted_foreground.opacity(0.68))
                                    .child("Show a dedicated floating Con window that slides down from the top of the screen."),
                            ),
                    )
                    .child(
                        div().pt(px(1.0)).child(
                            Switch::new("hotkey-window-enabled")
                                .checked(hotkey_window_enabled)
                                .small()
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.config.keybindings.hotkey_window_enabled = *checked;
                                    if *checked && this.config.keybindings.hotkey_window.trim().is_empty() {
                                        this.config.keybindings.hotkey_window = "cmd-\\".to_string();
                                    }
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .child(row_separator(theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.0))
                    .px(px(16.0))
                    .py(px(11.0))
                    .text_color(if hotkey_window_enabled { theme.foreground } else { theme.muted_foreground })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).child("Shortcut"))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .line_height(px(15.0))
                                    .text_color(theme.muted_foreground.opacity(0.62))
                                    .child("Use a low-conflict macOS shortcut. Cmd-Backslash matches the requested default."),
                            ),
                    )
                    .child(
                        div()
                            .id("key-badge-hotkey-window")
                            .min_w(px(112.0))
                            .flex()
                            .justify_end()
                            .opacity(if hotkey_window_enabled { 1.0 } else { 0.45 })
                            .cursor_pointer()
                            .rounded(px(7.0))
                            .px(px(4.0))
                            .py(px(3.0))
                            .hover(|s| s.bg(theme.muted.opacity(0.08)))
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                if this.config.keybindings.hotkey_window_enabled {
                                    this.recording_key = Some("hotkey_window".to_string());
                                    cx.notify();
                                }
                            }))
                            .child(hotkey_window_badge),
                    ),
            )
            .child(row_separator(theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.0))
                    .px(px(16.0))
                    .py(px(11.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(div().text_size(px(11.5)).font_weight(FontWeight::MEDIUM).child("Always on top"))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .line_height(px(15.0))
                                    .text_color(theme.muted_foreground.opacity(0.62))
                                    .child("Keep the hotkey window above other apps while it is visible."),
                            ),
                    )
                    .child(
                        Switch::new("hotkey-window-always-on-top")
                            .checked(hotkey_window_always_on_top)
                            .disabled(!hotkey_window_enabled)
                            .small()
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                this.config.keybindings.hotkey_window_always_on_top = *checked;
                                crate::hotkey_window::set_always_on_top(*checked);
                                cx.notify();
                            })),
                    ),
            );
```

Finally insert it in the Global group:

```rust
                .child(group_label("Global", &theme))
                .child(global_summon_card)
                .child(hotkey_window_card)
                .child(div().h(px(8.0)))
```

- [ ] **Step 4: Keep runtime updates live when keybindings are saved**

In `crates/con-app/src/hotkey_window.rs`, ensure `update_from_keybindings` applies the level immediately:

```rust
pub fn update_from_keybindings(keybindings: &KeybindingConfig) {
    set_always_on_top(keybindings.hotkey_window_always_on_top);
}
```

And in `crates/con-app/src/workspace.rs`, after `crate::global_hotkey::update_from_keybindings(&kb);`, add:

```rust
        #[cfg(target_os = "macos")]
        crate::hotkey_window::update_from_keybindings(&kb);
```

- [ ] **Step 5: Run the crate tests and build**

Run:

```bash
cargo test -p con-app
cargo build -p con-app
```

Expected: PASS.

- [ ] **Step 6: Commit the settings UI wiring**

```bash
git add crates/con-app/src/settings_panel.rs crates/con-app/src/workspace.rs crates/con-app/src/hotkey_window.rs
git commit -m "feat: add hotkey window settings"
```

---

### Task 7: Manual macOS verification and cleanup

**Files:**
- Verify: `crates/con-app/src/main.rs`
- Verify: `crates/con-app/src/hotkey_window.rs`
- Verify: `crates/con-app/src/objc/hotkey_window_trampoline.m`

- [ ] **Step 1: Launch the app with a clean config state**

Run:

```bash
cargo run -p con-app
```

Expected: the normal main Con window opens; no hotkey window is visible by default.

- [ ] **Step 2: Enable the feature in Settings and verify registration**

Manual check:

1. Open **Settings → Keys**.
2. Turn on **Hotkey Window**.
3. Confirm the shortcut badge shows **Cmd + \\**.
4. Leave **Always on top** off initially.
5. Save settings.

Expected: no errors in logs; pressing `Cmd+\\` should now summon a second window.

- [ ] **Step 3: Verify slide-in / slide-out and workspace persistence**

Manual check:

1. Press `Cmd+\\`.
2. Confirm a dedicated floating window slides down from the top edge.
3. Open a new tab inside that window.
4. Press `Cmd+\\` again to hide it.
5. Press `Cmd+\\` a third time.

Expected:
- The hotkey window slides out and disappears instead of closing.
- When reopened, the tab you created is still there.
- The normal main window is unaffected.

- [ ] **Step 4: Verify always-on-top live update**

Manual check:

1. With the hotkey window visible, go back to **Settings → Keys**.
2. Turn on **Always on top**.
3. Bring another app to the front.
4. Turn it back off and repeat.

Expected:
- With Always on top enabled, the hotkey window stays above the other app.
- With it disabled, normal window ordering resumes.
- No restart is required.

- [ ] **Step 5: Run final automated verification**

Run:

```bash
cargo test -p con-core
cargo test -p con-app
cargo build -p con-app
```

Expected: all commands PASS.

- [ ] **Step 6: Commit the verified final state**

```bash
git add crates/con-core/src/config.rs crates/con-app/build.rs crates/con-app/src/main.rs crates/con-app/src/global_hotkey.rs crates/con-app/src/hotkey_window.rs crates/con-app/src/settings_panel.rs crates/con-app/src/workspace.rs crates/con-app/src/objc/global_hotkey_trampoline.m crates/con-app/src/objc/hotkey_window_trampoline.m
git commit -m "feat: add macos hotkey window"
```

---

## Spec coverage check

- Dedicated separate window: Task 5
- macOS-only scope: Tasks 2–5 use macOS-only ObjC/AppKit paths
- Global `Cmd+\\` shortcut: Tasks 1, 4, 6
- Full `ConWorkspace` contents: Task 5 reuses normal workspace construction
- Slide from top: Task 2 AppKit animation
- Floating / optional always-on-top: Tasks 2, 5, 6
- Persist while hidden: Tasks 3, 5, 7
- Settings UI: Task 6

## Placeholder scan

- No `TODO` / `TBD` markers remain.
- Every code-changing step includes concrete code blocks.
- Every verification step includes concrete commands or manual checks.

## Type consistency check

- Config field names are consistent across all tasks: `hotkey_window_enabled`, `hotkey_window`, `hotkey_window_always_on_top`.
- Public Rust controller API is consistent across tasks: `init`, `update_from_keybindings`, `toggle`, `store_window_ptr`, `set_always_on_top`, `ensure_hotkey_window`.
- Objective-C exported names are consistent across tasks: `con_hotkey_window_configure`, `con_hotkey_window_set_level`, `con_hotkey_window_slide_in`, `con_hotkey_window_slide_out`, `con_register_hotkey_window_hotkey`, `con_unregister_hotkey_window_hotkey`.
