# Hotkey Window Implementation Plan

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
- `cargo test -p con`
- `cargo build -p con`
- Manual macOS verification: settings toggles, shortcut registration, slide-in/slide-out, persistence while hidden, always-on-top toggle.

---

### Task 1: Add config fields and tests

**Files:**
- Modify: `crates/con-core/src/config.rs`
- Test: `crates/con-core/src/config.rs`

- [x] **Step 1: Write the failing tests for new keybinding defaults and TOML loading**
- [x] **Step 2: Run the tests to verify they fail**
- [x] **Step 3: Add the new config fields and defaults**
- [x] **Step 4: Run the tests to verify they pass**
- [x] **Step 5: Commit the config changes**

Notes:
- We used `cargo test -p con-core hotkey_window` because `cargo test` accepts a single filter token before `--`.
- Commit completed: `feat: add hotkey window config`

---

### Task 2: Add the AppKit trampoline and second Carbon hotkey slot

**Files:**
- Create: `crates/con-app/src/objc/hotkey_window_trampoline.m`
- Modify: `crates/con-app/src/objc/global_hotkey_trampoline.m`
- Modify: `crates/con-app/build.rs`
- Test: `cargo build -p con`

- [x] **Step 1: Write the failing build expectation by referencing the new Objective-C file in build.rs**
- [x] **Step 2: Run the build to verify it fails because the new file does not exist yet**
- [x] **Step 3: Create the new AppKit trampoline file**
- [x] **Step 4: Extend the existing Carbon trampoline with a second independent hotkey registration slot**
- [x] **Step 5: Run the build to verify the Objective-C bridge compiles**
- [x] **Step 6: Commit the trampoline changes**

Notes:
- The correct Cargo package name is `con`, not `con-app`.
- Commit completed: `feat: add macos hotkey window trampolines`

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
cargo test -p con hotkey_window_enabled
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
cargo test -p con hotkey_window_enabled
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
- [ ] **Step 2: Run the test to verify current behavior before wiring the second registration path**
- [ ] **Step 3: Add FFI declarations and update logic for the second global hotkey**
- [ ] **Step 4: Make settings saves reapply both hotkey registrations**
- [ ] **Step 5: Run targeted tests for the parser and hotkey module**
- [ ] **Step 6: Commit the hotkey registration wiring**

---

### Task 5: Build the dedicated window creation path and always-on-top updates

**Files:**
- Modify: `crates/con-app/src/main.rs`
- Modify: `crates/con-app/src/hotkey_window.rs`
- Test: `cargo build -p con`

- [ ] **Step 1: Write the failing build by calling a not-yet-defined helper from the hotkey-window module**
- [ ] **Step 2: Run the build to verify it fails**
- [ ] **Step 3: Implement the window creation helper and dedicated open path**
- [ ] **Step 4: Rebuild and fix raw handle extraction if needed using the actual GPUI API**
- [ ] **Step 5: Run the build to verify the dedicated window path compiles**
- [ ] **Step 6: Commit the dedicated window open path**

---

### Task 6: Add Settings UI for enable/binding/always-on-top

**Files:**
- Modify: `crates/con-app/src/settings_panel.rs`
- Modify: `crates/con-app/src/hotkey_window.rs`
- Test: `cargo test -p con`

- [ ] **Step 1: Write the failing state-routing changes for the new binding field**
- [ ] **Step 2: Run the app tests to catch any exhaustive-match or unused-state issues**
- [ ] **Step 3: Add the Hotkey Window card below Global Hotkey**
- [ ] **Step 4: Keep runtime updates live when keybindings are saved**
- [ ] **Step 5: Run the crate tests and build**
- [ ] **Step 6: Commit the settings UI wiring**

---

### Task 7: Manual macOS verification and cleanup

**Files:**
- Verify: `crates/con-app/src/main.rs`
- Verify: `crates/con-app/src/hotkey_window.rs`
- Verify: `crates/con-app/src/objc/hotkey_window_trampoline.m`

- [ ] **Step 1: Launch the app with a clean config state**
- [ ] **Step 2: Enable the feature in Settings and verify registration**
- [ ] **Step 3: Verify slide-in / slide-out and workspace persistence**
- [ ] **Step 4: Verify always-on-top live update**
- [ ] **Step 5: Run final automated verification**
- [ ] **Step 6: Commit the verified final state**
