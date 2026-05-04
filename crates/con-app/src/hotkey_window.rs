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
            unsafe {
                con_hotkey_window_set_level(window_ptr as *mut std::ffi::c_void, always_on_top)
            };
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
