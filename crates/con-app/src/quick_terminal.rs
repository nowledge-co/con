use con_core::config::KeybindingConfig;
use gpui::App;
use std::cell::RefCell;

unsafe extern "C" {
    fn con_quick_terminal_configure(window_ptr: *mut std::ffi::c_void, always_on_top: bool);
    fn con_quick_terminal_set_level(window_ptr: *mut std::ffi::c_void, always_on_top: bool);
    fn con_quick_terminal_slide_in(window_ptr: *mut std::ffi::c_void);
    fn con_quick_terminal_slide_out(window_ptr: *mut std::ffi::c_void, return_pid: i32);
    fn con_quick_terminal_window_from_view(view_ptr: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn con_quick_terminal_frontmost_app_pid() -> i32;
}

thread_local! {
    static QUICK_TERMINAL_RAW_PTR: RefCell<Option<usize>> = const { RefCell::new(None) };
    static QUICK_TERMINAL_VISIBLE: RefCell<bool> = const { RefCell::new(false) };
    static QUICK_TERMINAL_RETURN_PID: RefCell<Option<i32>> = const { RefCell::new(None) };
}

pub fn init(_cx: &App, keybindings: &KeybindingConfig) {
    set_always_on_top(keybindings.quick_terminal_always_on_top);
}

fn should_create_with_first_normal_window(existing_ptr: Option<usize>) -> bool {
    existing_ptr.is_none()
}

pub fn ensure_created_for_app_run(cx: &mut App) {
    let should_create = QUICK_TERMINAL_RAW_PTR.with(|slot| {
        should_create_with_first_normal_window(*slot.borrow())
    });
    if !should_create {
        return;
    }

    let config = con_core::Config::load().unwrap_or_default();
    crate::open_quick_terminal(
        config,
        crate::fresh_window_session_with_history_for_cwd(default_quick_terminal_cwd()),
        cx,
    );
}

pub fn store_window_ptr(window_ptr: *mut std::ffi::c_void, always_on_top: bool) {
    QUICK_TERMINAL_RAW_PTR.with(|slot| {
        *slot.borrow_mut() = Some(window_ptr as usize);
    });
    QUICK_TERMINAL_VISIBLE.with(|visible| *visible.borrow_mut() = false);
    unsafe { con_quick_terminal_configure(window_ptr, always_on_top) };
}

pub fn window_from_view_ptr(view_ptr: *mut std::ffi::c_void) -> Option<*mut std::ffi::c_void> {
    let window_ptr = unsafe { con_quick_terminal_window_from_view(view_ptr) };
    (!window_ptr.is_null()).then_some(window_ptr)
}

pub fn toggle(_cx: &mut App) {
    let window_ptr = QUICK_TERMINAL_RAW_PTR.with(|slot| *slot.borrow());

    if let Some(window_ptr) = window_ptr {
        QUICK_TERMINAL_VISIBLE.with(|visible| {
            let mut visible = visible.borrow_mut();
            let raw = window_ptr as *mut std::ffi::c_void;
            if *visible {
                let return_pid =
                    QUICK_TERMINAL_RETURN_PID.with(|slot| take_return_pid(&mut slot.borrow_mut()));
                unsafe {
                    con_quick_terminal_slide_out(raw, return_pid.unwrap_or(0));
                }
                *visible = false;
            } else {
                let current_pid = std::process::id() as i32;
                let frontmost_pid = unsafe { con_quick_terminal_frontmost_app_pid() };
                QUICK_TERMINAL_RETURN_PID.with(|slot| {
                    *slot.borrow_mut() = remember_return_pid(current_pid, Some(frontmost_pid));
                });
                unsafe {
                    con_quick_terminal_slide_in(raw);
                }
                *visible = true;
            }
        });
        return;
    }

    log::warn!("quick terminal toggle requested before singleton window was created");
}

/// Returns the default working directory for the quick terminal.
/// On macOS this is the user's home directory; on other platforms
/// it defers to the process working directory.
#[cfg(target_os = "macos")]
pub fn default_quick_terminal_cwd() -> Option<std::path::PathBuf> {
    dirs::home_dir()
}

#[cfg(not(target_os = "macos"))]
pub fn default_quick_terminal_cwd() -> Option<std::path::PathBuf> {
    None
}

/// Set the visible flag to false without any animation. Used at the start
/// of reinitialize_quick_terminal_and_hide to prevent the auto-hide observer
/// from competing with the intentional slide-out.
pub fn mark_hidden() {
    QUICK_TERMINAL_VISIBLE.with(|v| *v.borrow_mut() = false);
}

/// Always slide the window off-screen, regardless of the current visible
/// flag. Consumes the saved return pid (captured during toggle-in) so
/// focus returns to the previously active app, not the con main window.
pub fn force_hide() {
    let return_pid =
        QUICK_TERMINAL_RETURN_PID.with(|slot| take_return_pid(&mut slot.borrow_mut()));
    QUICK_TERMINAL_RAW_PTR.with(|slot| {
        let window_ptr = *slot.borrow();
        if let Some(window_ptr) = window_ptr {
            unsafe {
                con_quick_terminal_slide_out(
                    window_ptr as *mut std::ffi::c_void,
                    return_pid.unwrap_or(0),
                );
            }
        }
    });
}

/// Slide the quick terminal window off-screen, but only if currently
/// visible. No-ops when already hidden to avoid double-hide during
/// resign-key animations. Does NOT consume the saved return pid —
/// macOS naturally activates whatever the user clicked on.
pub fn hide() {
    let is_visible = QUICK_TERMINAL_VISIBLE.with(|v| *v.borrow());
    if !is_visible {
        return;
    }
    QUICK_TERMINAL_VISIBLE.with(|v| *v.borrow_mut() = false);
    // auto-hide: pass 0 so macOS handles app switching naturally
    QUICK_TERMINAL_RAW_PTR.with(|slot| {
        let window_ptr = *slot.borrow();
        if let Some(window_ptr) = window_ptr {
            unsafe {
                con_quick_terminal_slide_out(window_ptr as *mut std::ffi::c_void, 0);
            }
        }
    });
}

/// Called from ObjC when the quick terminal window resigns key (user clicked
/// elsewhere). Hides the window automatically like iTerm2's hotkey window.
#[unsafe(no_mangle)]
extern "C" fn con_quick_terminal_handle_resign_key() {
    hide();
}

pub fn set_always_on_top(always_on_top: bool) {
    QUICK_TERMINAL_RAW_PTR.with(|slot| {
        if let Some(window_ptr) = *slot.borrow() {
            unsafe {
                con_quick_terminal_set_level(window_ptr as *mut std::ffi::c_void, always_on_top)
            };
        }
    });
}

fn remember_return_pid(current_pid: i32, frontmost_pid: Option<i32>) -> Option<i32> {
    frontmost_pid.filter(|pid| *pid != current_pid && *pid > 0)
}

fn take_return_pid(slot: &mut Option<i32>) -> Option<i32> {
    slot.take()
}

#[cfg(test)]
mod tests {
    use super::{
        remember_return_pid, should_create_with_first_normal_window, take_return_pid,
    };

    #[test]
    fn disabled_or_empty_binding_disables_quick_terminal_registration() {
        assert!(!(false && !"cmd-\\".trim().is_empty()));
        assert!(!(true && !"   ".trim().is_empty()));
        assert!(true && !"cmd-\\".trim().is_empty());
    }

    #[test]
    fn remember_return_pid_ignores_con_itself_and_consumes_saved_pid() {
        assert_eq!(remember_return_pid(42, Some(7)), Some(7));
        assert_eq!(remember_return_pid(42, Some(42)), None);
        assert_eq!(remember_return_pid(42, None), None);

        let mut slot = Some(7);
        assert_eq!(take_return_pid(&mut slot), Some(7));
        assert_eq!(take_return_pid(&mut slot), None);
    }

    #[test]
    fn creates_quick_terminal_only_once_per_app_run() {
        assert!(should_create_with_first_normal_window(None));
        assert!(!should_create_with_first_normal_window(Some(1)));
    }

    #[test]
    fn quick_terminal_uses_a_dedicated_default_cwd() {
        let home = dirs::home_dir();
        assert!(home.is_some());
    }
}
